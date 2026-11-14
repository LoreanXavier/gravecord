//! Bounded persistent worker pool for row-banded frame conversions.
//!
//! Keeps at most three background workers alive so multi-megapixel format conversions run across up to four CPU bands
//! without spawning OS threads or re-allocating scratch buffers every frame.

use std::cell::RefCell;
use std::sync::{Arc, Mutex, OnceLock, mpsc};

use crate::convert::YuvBand;
use crate::picture::BgraBand;

const MAX_BANDS: usize = 4;

/// Reusable scratch storage for one band execution, avoiding per-frame heap allocations.
#[derive(Default)]
pub(crate) struct Scratch {
    pub(crate) yuv_top: Vec<[u32; 3]>,
    pub(crate) yuv_bottom: Vec<[u32; 3]>,
    pub(crate) chroma_line: Vec<(i32, i32)>,
}

/// Source pixel input passed into [`dispatch`].
pub(crate) enum Input<'a> {
    /// Borrowed slice; copied into the pool's persistent staging buffer only when work is split across workers.
    Slice(&'a [u8]),
    /// Caller-owned pixel buffer shared across workers via `Arc` without copying bytes and returned on completion.
    Vec(&'a mut Vec<u8>),
}

/// Per-band conversion descriptor dispatched to pool workers.
pub(crate) enum Task {
    Yuv420(YuvBand),
    Bgra(BgraBand),
}

impl Task {
    fn out_len(&self) -> usize {
        match self {
            Self::Yuv420(band) => band.out_len(),
            Self::Bgra(band) => band.out_len(),
        }
    }

    fn run(&self, input: &[u8], out: &mut [u8], scratch: &mut Scratch) {
        match self {
            Self::Yuv420(band) => crate::convert::run_band(input, out, scratch, band),
            Self::Bgra(band) => crate::picture::run_band(input, out, scratch, band),
        }
    }
}

/// Optimal number of row bands (`1..=4`) for a picture of `height` rows.
pub(crate) fn band_count(height: usize) -> usize {
    static THREADS: OnceLock<usize> = OnceLock::new();
    let max_threads = *THREADS.get_or_init(|| {
        std::thread::available_parallelism().map_or(1, |count| count.get().clamp(1, MAX_BANDS))
    });
    max_threads.min(height / 64).max(1)
}

thread_local! {
    static CALLER_SCRATCH: RefCell<Scratch> = RefCell::new(Scratch::default());
}

fn with_caller_scratch(f: impl FnOnce(&mut Scratch)) {
    CALLER_SCRATCH.with(|cell| {
        if let Ok(mut scratch) = cell.try_borrow_mut() {
            f(&mut scratch);
        } else {
            let mut scratch = Scratch::default();
            f(&mut scratch);
        }
    });
}

struct WorkerRequest {
    input: Arc<Vec<u8>>,
    out: Vec<u8>,
    task: Task,
}

struct WorkerSlot {
    req: mpsc::SyncSender<WorkerRequest>,
    resp: mpsc::Receiver<Vec<u8>>,
    out: Vec<u8>,
}

struct Pool {
    workers: Vec<WorkerSlot>,
    staging: Option<Arc<Vec<u8>>>,
}

impl Pool {
    fn new() -> Self {
        let count = band_count(usize::MAX).saturating_sub(1);
        let mut workers = Vec::with_capacity(count);
        for index in 0..count {
            let (req_tx, req_rx) = mpsc::sync_channel::<WorkerRequest>(1);
            let (resp_tx, resp_rx) = mpsc::sync_channel::<Vec<u8>>(1);
            let builder = std::thread::Builder::new().name(format!("dc-media-band-{index}"));
            if builder
                .spawn(move || worker_loop(&req_rx, &resp_tx))
                .is_ok()
            {
                workers.push(WorkerSlot {
                    req: req_tx,
                    resp: resp_rx,
                    out: Vec::new(),
                });
            }
        }
        Self {
            workers,
            staging: Some(Arc::new(Vec::new())),
        }
    }
}

fn worker_loop(req_rx: &mpsc::Receiver<WorkerRequest>, resp_tx: &mpsc::SyncSender<Vec<u8>>) {
    let _guard = crate::throttle::Guard::acquire();
    let mut scratch = Scratch::default();
    while let Ok(mut req) = req_rx.recv() {
        let needed = req.task.out_len();
        if req.out.len() != needed {
            req.out.resize(needed, 0);
        }
        req.task.run(&req.input, &mut req.out, &mut scratch);
        // Drop the shared input handle before signalling completion so the caller can reclaim ownership cleanly.
        drop(req.input);
        if resp_tx.send(req.out).is_err() {
            break;
        }
    }
}

fn pool() -> &'static Mutex<Pool> {
    static POOL: OnceLock<Mutex<Pool>> = OnceLock::new();
    POOL.get_or_init(|| Mutex::new(Pool::new()))
}

fn run_inline(
    slice: &[u8],
    tasks: impl Iterator<Item = (usize, Task)>,
    run_caller: impl FnOnce(&[u8], &mut Scratch),
    collect: &mut impl FnMut(usize, &[u8]),
) {
    let mut out = Vec::new();
    with_caller_scratch(|scratch| {
        run_caller(slice, scratch);
        for (index, task) in tasks {
            let needed = task.out_len();
            if out.len() != needed {
                out.resize(needed, 0);
            }
            task.run(slice, &mut out, scratch);
            collect(index, &out);
        }
    });
}

/// Runs `worker_tasks` in parallel on persistent pool workers and `run_caller` inline on the caller thread.
///
/// `collect` receives `(band_index, worker_bytes)` for every completed worker band so the caller can copy each band
/// into its destination plane slices.
pub(crate) fn dispatch(
    input: Input<'_>,
    worker_tasks: Vec<Task>,
    run_caller: impl FnOnce(&[u8], &mut Scratch),
    mut collect: impl FnMut(usize, &[u8]),
) {
    if worker_tasks.is_empty() {
        match input {
            Input::Slice(slice) => with_caller_scratch(|scratch| run_caller(slice, scratch)),
            Input::Vec(vec) => with_caller_scratch(|scratch| run_caller(vec, scratch)),
        }
        return;
    }

    let mut guard = pool().lock().unwrap_or_else(|e| e.into_inner());
    if guard.workers.len() < worker_tasks.len() {
        drop(guard);
        let slice = match &input {
            Input::Slice(slice) => *slice,
            Input::Vec(vec) => vec.as_slice(),
        };
        run_inline(
            slice,
            worker_tasks.into_iter().enumerate(),
            run_caller,
            &mut collect,
        );
        return;
    }

    let (shared_arc, was_vec) = match input {
        Input::Slice(slice) => {
            let mut arc = guard
                .staging
                .take()
                .unwrap_or_else(|| Arc::new(Vec::new()));
            let staging = Arc::make_mut(&mut arc);
            staging.clear();
            staging.extend_from_slice(slice);
            (arc, None)
        }
        Input::Vec(vec_mut) => {
            let owned = std::mem::take(vec_mut);
            (Arc::new(owned), Some(vec_mut))
        }
    };

    let mut dispatched = Vec::with_capacity(worker_tasks.len());
    let mut unsent = Vec::new();
    for (index, task) in worker_tasks.into_iter().enumerate() {
        let out = std::mem::take(&mut guard.workers[index].out);
        let req = WorkerRequest {
            input: Arc::clone(&shared_arc),
            out,
            task,
        };
        if let Err(err) = guard.workers[index].req.send(req) {
            let rejected = err.0;
            guard.workers[index].out = rejected.out;
            unsent.push((index, rejected.task));
        } else {
            dispatched.push(index);
        }
    }

    with_caller_scratch(|scratch| {
        run_caller(&shared_arc, scratch);
        if !unsent.is_empty() {
            let mut fallback_out = Vec::new();
            for (index, task) in unsent {
                let needed = task.out_len();
                if fallback_out.len() != needed {
                    fallback_out.resize(needed, 0);
                }
                task.run(&shared_arc, &mut fallback_out, scratch);
                collect(index, &fallback_out);
            }
        }
    });

    for index in dispatched {
        if let Ok(out) = guard.workers[index].resp.recv() {
            collect(index, &out);
            guard.workers[index].out = out;
        }
    }

    match was_vec {
        Some(vec_mut) => match Arc::try_unwrap(shared_arc) {
            Ok(owned) => *vec_mut = owned,
            Err(still_shared) => *vec_mut = (*still_shared).clone(),
        },
        None => {
            guard.staging = Some(shared_arc);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn band_count_is_bounded_between_one_and_four() {
        assert_eq!(band_count(1), 1);
        assert_eq!(band_count(63), 1);
        assert!(band_count(1440) <= MAX_BANDS);
        assert!(band_count(1440) >= 1);
    }

    #[test]
    fn shared_vec_returns_original_allocation_after_parallel_dispatch() {
        let width = 256;
        let height = 256;
        let mut owned = vec![128u8; width * height * 3 / 2];
        let orig_ptr = owned.as_ptr();
        let orig_cap = owned.capacity();

        let bands = band_count(height);
        if bands > 1 {
            let band_rows = height.div_ceil(bands);
            let workers: Vec<Task> = (0..bands - 1)
                .map(|i| {
                    Task::Bgra(BgraBand {
                        width,
                        height,
                        interleaved: true,
                        first: i * band_rows,
                        rows: band_rows,
                    })
                })
                .collect();
            let last_first = (bands - 1) * band_rows;
            let last_rows = height - last_first;
            let mut out = vec![0u8; width * height * 4];
            let (worker_out, caller_out) = out.split_at_mut(last_first * width * 4);
            let mut worker_slices: Vec<&mut [u8]> = worker_out
                .chunks_exact_mut(band_rows * width * 4)
                .collect();
            dispatch(
                Input::Vec(&mut owned),
                workers,
                |src, scratch| {
                    crate::picture::run_band(
                        src,
                        caller_out,
                        scratch,
                        &BgraBand {
                            width,
                            height,
                            interleaved: true,
                            first: last_first,
                            rows: last_rows,
                        },
                    );
                },
                |index, bytes| worker_slices[index].copy_from_slice(bytes),
            );
            assert_eq!(owned.as_ptr(), orig_ptr);
            assert_eq!(owned.capacity(), orig_cap);
            assert!(out.chunks_exact(4).all(|px| px[3] == 255));
        }
    }
}
