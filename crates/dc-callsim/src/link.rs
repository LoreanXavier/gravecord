//! The network between a participant and the simulated server: per direction a bottleneck of a given capacity with a
//! drop-tail queue, a one-way delay, jitter, independent random loss and occasional reordering.
//!
//! The random source and the "datagram in flight, earliest first" heap come from `crates/dc-voice/src/video/lossy.rs`
//! (its `Random` and `Flight`); capacity, the queue bound and reordering are added here, because a real path
//! congests before it loses. Datagrams leave from one thread that sleeps with `std::thread::sleep` (a high-resolution
//! waitable timer on Windows), not on tokio's timer, which rounds every wait up to the 15.6 ms system tick there.
use std::collections::BinaryHeap;
use std::net::{SocketAddr, UdpSocket};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

/// What one direction of a path does to datagrams. The default passes everything at once.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shape {
    /// Bottleneck rate in bits per second; `None` is unlimited.
    pub capacity_bps: Option<u64>,
    pub delay_ms: u64,
    /// Each datagram gets an extra delay drawn evenly from 0 to this; it never overtakes the one before it unless it is
    /// reordered.
    pub jitter_ms: u64,
    pub loss_percent: f64,
    /// Share of datagrams that are held back by an extra `delay + jitter + 5 ms` and so overtaken.
    pub reorder_percent: f64,
    /// The longest a datagram waits in the bottleneck queue before it is dropped (a router's buffer).
    pub queue_ms: u64,
}

impl Default for Shape {
    fn default() -> Self {
        Self {
            capacity_bps: None,
            delay_ms: 0,
            jitter_ms: 0,
            loss_percent: 0.0,
            reorder_percent: 0.0,
            queue_ms: 300,
        }
    }
}

impl Shape {
    pub fn describe(&self) -> String {
        format!(
            "capacity {} delay {} ms jitter {} ms loss {}% reorder {}% queue {} ms",
            self.capacity_bps
                .map_or("unlimited".to_owned(), |bps| format!("{} kb/s", bps / 1000)),
            self.delay_ms,
            self.jitter_ms,
            self.loss_percent,
            self.reorder_percent,
            self.queue_ms
        )
    }
}

/// xorshift32, as `lossy.rs` uses it.
#[derive(Clone)]
pub(crate) struct Random(u32);

impl Random {
    pub(crate) fn new(seed: u32) -> Self {
        Self(seed.max(1))
    }
    pub(crate) fn next(&mut self) -> u32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        self.0
    }
    pub(crate) fn chance(&mut self, percent: f64) -> bool {
        percent > 0.0 && f64::from(self.next() % 100_000) < percent * 1000.0
    }
}

/// One direction of one path: its shape and where its bottleneck stands.
pub(crate) struct Direction {
    pub shape: Shape,
    busy_until: Option<Instant>,
    last_arrival: Option<Instant>,
    random: Random,
    pub sent: u64,
    pub lost: u64,
    pub dropped: u64,
    pub bytes: u64,
}

impl Direction {
    pub(crate) fn new(seed: u32) -> Self {
        Self {
            shape: Shape::default(),
            busy_until: None,
            last_arrival: None,
            random: Random::new(seed),
            sent: 0,
            lost: 0,
            dropped: 0,
            bytes: 0,
        }
    }

    /// When a datagram of `bytes` handed in at `now` arrives, or `None` when it is lost or the queue is full.
    pub(crate) fn admit(&mut self, bytes: usize, now: Instant) -> Option<Instant> {
        self.sent += 1;
        let shape = self.shape;
        if self.random.chance(shape.loss_percent) {
            self.lost += 1;
            return None;
        }
        let mut leaves = now;
        if let Some(bps) = shape.capacity_bps.filter(|bps| *bps > 0) {
            // UDP and IP headers count against the bottleneck too.
            let wire = (bytes as u64 + 28) * 8;
            let serialize = Duration::from_nanos(wire * 1_000_000_000 / bps);
            let start = self.busy_until.filter(|busy| *busy > now).unwrap_or(now);
            if start.saturating_duration_since(now) > Duration::from_millis(shape.queue_ms) {
                self.dropped += 1;
                return None;
            }
            leaves = start + serialize;
            self.busy_until = Some(leaves);
        }
        let jitter = if shape.jitter_ms > 0 {
            Duration::from_micros(u64::from(self.random.next()) % (shape.jitter_ms * 1000 + 1))
        } else {
            Duration::ZERO
        };
        let mut arrives = leaves + Duration::from_millis(shape.delay_ms) + jitter;
        if self.random.chance(shape.reorder_percent) {
            arrives += Duration::from_millis(shape.delay_ms + shape.jitter_ms + 5);
        } else if let Some(last) = self.last_arrival {
            arrives = arrives.max(last);
            self.last_arrival = Some(arrives);
        } else {
            self.last_arrival = Some(arrives);
        }
        self.bytes += bytes as u64;
        Some(arrives)
    }
}

/// What the network thread does when a datagram is due.
pub(crate) enum Delivery {
    /// Out of the server's socket to a participant.
    Out(SocketAddr),
    /// Into the server, as if it had just arrived from this address.
    In(SocketAddr),
}

struct Flight {
    at: Instant,
    order: u64,
    delivery: Delivery,
    bytes: Vec<u8>,
}

impl PartialEq for Flight {
    fn eq(&self, other: &Self) -> bool {
        (self.at, self.order) == (other.at, other.order)
    }
}
impl Eq for Flight {}
impl Ord for Flight {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        (other.at, other.order).cmp(&(self.at, self.order))
    }
}
impl PartialOrd for Flight {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

struct Queue {
    heap: BinaryHeap<Flight>,
    order: u64,
    stop: bool,
}

/// Datagrams in flight, and the thread that delivers them when due.
pub(crate) struct Network {
    queue: Arc<(Mutex<Queue>, Condvar)>,
}

/// The longest the delivery thread sleeps before it looks at the queue again, while something is waiting.
const POLL: Duration = Duration::from_millis(1);

impl Network {
    /// `socket` sends what leaves; `ingress` takes what arrives at the server after its uplink.
    pub(crate) fn start(
        socket: UdpSocket,
        ingress: Arc<dyn Fn(SocketAddr, Vec<u8>) + Send + Sync>,
    ) -> std::io::Result<Self> {
        let queue = Arc::new((
            Mutex::new(Queue {
                heap: BinaryHeap::new(),
                order: 0,
                stop: false,
            }),
            Condvar::new(),
        ));
        let worker = queue.clone();
        std::thread::Builder::new()
            .name("dc-callsim-network".into())
            .spawn(move || {
                loop {
                    let due = {
                        let (lock, changed) = &*worker;
                        let mut queue = lock.lock().unwrap_or_else(|e| e.into_inner());
                        while queue.heap.is_empty() && !queue.stop {
                            queue = changed.wait(queue).unwrap_or_else(|e| e.into_inner());
                        }
                        if queue.stop {
                            return;
                        }
                        let now = Instant::now();
                        let mut due = Vec::new();
                        while queue.heap.peek().is_some_and(|top| top.at <= now) {
                            if let Some(flight) = queue.heap.pop() {
                                due.push(flight);
                            }
                        }
                        due
                    };
                    if due.is_empty() {
                        std::thread::sleep(POLL);
                        continue;
                    }
                    for flight in due {
                        match flight.delivery {
                            // A full socket buffer drops the datagram, as a real network would.
                            Delivery::Out(to) => {
                                let _ = socket.send_to(&flight.bytes, to);
                            }
                            Delivery::In(from) => ingress(from, flight.bytes),
                        }
                    }
                }
            })?;
        Ok(Self { queue })
    }

    pub(crate) fn schedule(&self, at: Instant, delivery: Delivery, bytes: Vec<u8>) {
        let (lock, changed) = &*self.queue;
        let mut queue = lock.lock().unwrap_or_else(|e| e.into_inner());
        queue.order += 1;
        let order = queue.order;
        queue.heap.push(Flight {
            at,
            order,
            delivery,
            bytes,
        });
        changed.notify_one();
    }
}

impl Drop for Network {
    fn drop(&mut self) {
        let (lock, changed) = &*self.queue;
        lock.lock().unwrap_or_else(|e| e.into_inner()).stop = true;
        changed.notify_one();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_clean_direction_passes_at_once_and_in_order() {
        let mut direction = Direction::new(7);
        let now = Instant::now();
        for _ in 0..100 {
            assert_eq!(direction.admit(1200, now), Some(now));
        }
    }

    #[test]
    fn capacity_spaces_datagrams_and_the_queue_bound_drops() {
        let mut direction = Direction::new(7);
        direction.shape.capacity_bps = Some(1_000_000);
        direction.shape.queue_ms = 50;
        let now = Instant::now();
        // 1228 bytes on the wire at 1 Mb/s: 9.8 ms each.
        let first = direction.admit(1200, now).unwrap();
        let second = direction.admit(1200, now).unwrap();
        let spacing = second.duration_since(first).as_micros();
        assert!((9_700..=9_900).contains(&spacing), "{spacing} us");
        let mut admitted = 2;
        while direction.admit(1200, now).is_some() {
            admitted += 1;
        }
        assert!(
            (6..=8).contains(&admitted),
            "50 ms of queue holds about six: {admitted}"
        );
        assert_eq!(direction.dropped, 1);
    }

    #[test]
    fn loss_delay_jitter_and_reordering_are_what_was_asked() {
        let mut direction = Direction::new(99);
        direction.shape = Shape {
            delay_ms: 20,
            jitter_ms: 10,
            loss_percent: 5.0,
            reorder_percent: 1.0,
            ..Shape::default()
        };
        let now = Instant::now();
        let mut arrivals = Vec::new();
        for i in 0..20_000u64 {
            if let Some(at) = direction.admit(100, now + Duration::from_millis(i)) {
                arrivals.push((i, at));
            }
        }
        let lost = 20_000 - arrivals.len();
        assert!((800..1200).contains(&lost), "about 5% lost: {lost}");
        let delays: Vec<u128> = arrivals
            .iter()
            .map(|(i, at)| {
                at.duration_since(now + Duration::from_millis(*i))
                    .as_millis()
            })
            .collect();
        assert!(
            delays.iter().all(|d| *d >= 20),
            "never sooner than the delay"
        );
        let overtaken = arrivals.windows(2).filter(|w| w[1].1 < w[0].1).count();
        assert!(
            (100..300).contains(&overtaken),
            "about 1% reordered: {overtaken}"
        );
    }
}
