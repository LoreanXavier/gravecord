//! Capture-only performance counters for the perf harness steps: process memory, CPU time, thread count, the moments the
//! first content showed, frame-time statistics and cache sizes. Nothing here is built into a normal build.

use gpui::RenderImage;
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use windows::Win32::Foundation::{FILETIME, HANDLE};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, TH32CS_SNAPTHREAD, THREADENTRY32, Thread32First, Thread32Next,
};
use windows::Win32::System::ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
use windows::Win32::System::SystemInformation::GetSystemTimeAsFileTime;
use windows::Win32::System::Threading::{GetCurrentProcess, GetCurrentProcessId, GetProcessTimes};

/// Milliseconds since the process started when the first messages were in, and when the loading screen was gone.
/// Zero until it happened.
pub static CONTENT_MS: AtomicU64 = AtomicU64::new(0);
pub static SPLASH_DONE_MS: AtomicU64 = AtomicU64::new(0);
/// The fonts loaded, the window opened and the gateway's READY applied, the same way.
pub static FONTS_MS: AtomicU64 = AtomicU64::new(0);
pub static WINDOW_MS: AtomicU64 = AtomicU64::new(0);
pub static READY_MS: AtomicU64 = AtomicU64::new(0);
/// Counts the bytes the Rust heap holds, so a run can tell its own allocations from what the graphics driver and the
/// system libraries keep in the process. Only the capture build uses it.
pub struct CountingAlloc;

#[global_allocator]
static ALLOCATOR: CountingAlloc = CountingAlloc;

static HEAP_LIVE: AtomicUsize = AtomicUsize::new(0);
static HEAP_PEAK: AtomicUsize = AtomicUsize::new(0);

/// With `GRAVECORD_HEAP_PROFILE=1` the allocator remembers a sample of live allocations with the stack that made
/// them: one for about every `INTERVAL` bytes allocated, and every allocation of at least that size, until it is
/// freed. A sample stands for `max(size, INTERVAL)` bytes.
const INTERVAL: usize = 32 * 1024;

static PROFILE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

type SampleMap =
    std::sync::Mutex<Option<std::collections::HashMap<usize, (usize, std::backtrace::Backtrace)>>>;
static SAMPLES: SampleMap = std::sync::Mutex::new(None);

thread_local! {
    /// Set while the allocator itself allocates (the stack capture and the table), so that is not sampled again.
    static INSIDE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static CREDIT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Starts the heap profile when the environment asks for it.
pub fn start_heap_profile() {
    if std::env::var_os("GRAVECORD_HEAP_PROFILE").is_some() {
        PROFILE.store(true, Ordering::Relaxed);
    }
}

fn track_big(pointer: *mut u8, size: usize) {
    if !PROFILE.load(Ordering::Relaxed) {
        return;
    }
    let weight = if size >= INTERVAL {
        size
    } else {
        let due = CREDIT
            .try_with(|credit| {
                let total = credit.get() + size;
                if total >= INTERVAL {
                    credit.set(total - INTERVAL);
                    true
                } else {
                    credit.set(total);
                    false
                }
            })
            .unwrap_or(false);
        if !due {
            return;
        }
        INTERVAL
    };
    if INSIDE
        .try_with(|inside| inside.replace(true))
        .unwrap_or(true)
    {
        return;
    }
    let stack = std::backtrace::Backtrace::force_capture();
    if let Ok(mut samples) = SAMPLES.lock() {
        samples
            .get_or_insert_with(Default::default)
            .insert(pointer as usize, (weight, stack));
    }
    let _ = INSIDE.try_with(|inside| inside.set(false));
}

fn untrack_big(pointer: *mut u8, _size: usize) {
    if !PROFILE.load(Ordering::Relaxed) {
        return;
    }
    if INSIDE
        .try_with(|inside| inside.replace(true))
        .unwrap_or(true)
    {
        return;
    }
    if let Ok(mut samples) = SAMPLES.lock()
        && let Some(samples) = samples.as_mut()
    {
        drop(samples.remove(&(pointer as usize)));
    }
    let _ = INSIDE.try_with(|inside| inside.set(false));
}

unsafe impl GlobalAlloc for CountingAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let pointer = unsafe { System.alloc(layout) };
        if !pointer.is_null() {
            let now = HEAP_LIVE.fetch_add(layout.size(), Ordering::Relaxed) + layout.size();
            HEAP_PEAK.fetch_max(now, Ordering::Relaxed);
            track_big(pointer, layout.size());
        }
        pointer
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let pointer = unsafe { System.alloc_zeroed(layout) };
        if !pointer.is_null() {
            let now = HEAP_LIVE.fetch_add(layout.size(), Ordering::Relaxed) + layout.size();
            HEAP_PEAK.fetch_max(now, Ordering::Relaxed);
            track_big(pointer, layout.size());
        }
        pointer
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        untrack_big(pointer, layout.size());
        unsafe { System.dealloc(pointer, layout) };
        HEAP_LIVE.fetch_sub(layout.size(), Ordering::Relaxed);
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        untrack_big(pointer, layout.size());
        let moved = unsafe { System.realloc(pointer, layout, new_size) };
        if moved.is_null() {
            track_big(pointer, layout.size());
        } else {
            HEAP_LIVE.fetch_sub(layout.size(), Ordering::Relaxed);
            let now = HEAP_LIVE.fetch_add(new_size, Ordering::Relaxed) + new_size;
            HEAP_PEAK.fetch_max(now, Ordering::Relaxed);
            track_big(moved, new_size);
        }
        moved
    }
}

/// The sampled live allocations grouped by the first frames of the stack that made them, largest first:
/// `bytes x samples: frame < frame < frame`. Empty unless the heap profile is on.
pub fn big_allocations() -> Vec<String> {
    let entries: Vec<(usize, String)> = {
        let Ok(samples) = SAMPLES.lock() else {
            return Vec::new();
        };
        let Some(samples) = samples.as_ref() else {
            return Vec::new();
        };
        let _ = INSIDE.try_with(|inside| inside.set(true));
        let rendered = samples
            .values()
            .map(|(weight, stack)| (*weight, stack.to_string()))
            .collect();
        let _ = INSIDE.try_with(|inside| inside.set(false));
        rendered
    };
    let _ = INSIDE.try_with(|inside| inside.set(true));
    let mut groups: std::collections::HashMap<String, (usize, usize)> = Default::default();
    for (size, text) in entries {
        let frames: Vec<&str> = text
            .lines()
            .filter(|line| !line.trim_start().starts_with("at "))
            .map(|line| line.split_once(':').map_or(line, |(_, rest)| rest).trim())
            .filter(|frame| {
                !(frame.starts_with("gravecord::perf::")
                    || frame.starts_with("<gravecord::perf::")
                    || frame.starts_with("alloc::")
                    || frame.starts_with("core::")
                    || frame.starts_with("std::")
                    || frame.starts_with("<alloc::")
                    || frame.starts_with("<core::")
                    || frame.starts_with("<std::")
                    || frame.starts_with("hashbrown::")
                    || frame.starts_with("__rust")
                    || frame.starts_with("<hashbrown::")
                    || frame.starts_with("_RNv")
                    || frame.starts_with("<T as"))
            })
            .take(6)
            .collect();
        let entry = groups.entry(frames.join(" < ")).or_default();
        entry.0 += size;
        entry.1 += 1;
    }
    let mut rows: Vec<(String, (usize, usize))> = groups.into_iter().collect();
    rows.sort_by_key(|(_, (bytes, _))| std::cmp::Reverse(*bytes));
    let lines = rows
        .into_iter()
        .take(40)
        .map(|(stack, (bytes, count))| format!("{bytes} x{count}: {stack}"))
        .collect();
    let _ = INSIDE.try_with(|inside| inside.set(false));
    lines
}

/// The working set split by what backs it: the largest mapped modules, files and private memory, as
/// `megabytes kind name` lines. Tells the process's own memory from what the system libraries and the driver keep.
pub fn working_set_report() -> Vec<String> {
    use windows::Win32::System::Memory::{
        MEM_COMMIT, MEM_IMAGE, MEM_MAPPED, MEMORY_BASIC_INFORMATION, VirtualQuery,
    };
    use windows::Win32::System::ProcessStatus::{GetMappedFileNameW, QueryWorkingSet};
    let process = unsafe { GetCurrentProcess() };
    // Every committed region: start, size, kind (0 private, 1 image, 2 mapped).
    let mut regions: Vec<(usize, usize, u8)> = Vec::new();
    let mut address = 0usize;
    loop {
        let mut info = MEMORY_BASIC_INFORMATION::default();
        let read = unsafe {
            VirtualQuery(
                Some(address as *const _),
                &mut info,
                std::mem::size_of::<MEMORY_BASIC_INFORMATION>(),
            )
        };
        if read == 0 {
            break;
        }
        if info.State == MEM_COMMIT {
            let kind = if info.Type == MEM_IMAGE {
                1
            } else if info.Type == MEM_MAPPED {
                2
            } else {
                0
            };
            regions.push((info.BaseAddress as usize, info.RegionSize, kind));
        }
        let next = (info.BaseAddress as usize).saturating_add(info.RegionSize);
        if next <= address {
            break;
        }
        address = next;
    }
    let mut buffer = vec![0usize; 1 << 21];
    let bytes = (buffer.len() * std::mem::size_of::<usize>()) as u32;
    if unsafe { QueryWorkingSet(process, buffer.as_mut_ptr().cast(), bytes) }.is_err() {
        return vec!["working set query failed".to_owned()];
    }
    let count = buffer[0].min(buffer.len() - 1);
    let mut by_name: std::collections::HashMap<String, usize> = Default::default();
    let mut names: std::collections::HashMap<usize, String> = Default::default();
    for flags in &buffer[1..=count] {
        let page = (flags >> 12) << 12;
        let at = regions.partition_point(|region| region.0 + region.1 <= page);
        let Some(&(base, _, kind)) = regions.get(at).filter(|region| region.0 <= page) else {
            continue;
        };
        let name = match kind {
            0 => "private".to_owned(),
            _ => names
                .entry(base)
                .or_insert_with(|| {
                    let mut path = [0u16; 512];
                    let length = unsafe { GetMappedFileNameW(process, base as *const _, &mut path) }
                        as usize;
                    let text = String::from_utf16_lossy(&path[..length]);
                    let file = text.rsplit('\\').next().unwrap_or("").to_owned();
                    if file.is_empty() {
                        if kind == 1 {
                            "image".to_owned()
                        } else {
                            "mapped".to_owned()
                        }
                    } else {
                        file
                    }
                })
                .clone(),
        };
        *by_name.entry(name).or_default() += 4096;
    }
    let mut rows: Vec<(String, usize)> = by_name.into_iter().collect();
    rows.sort_by_key(|(_, bytes)| std::cmp::Reverse(*bytes));
    rows.into_iter()
        .take(16)
        .map(|(name, bytes)| format!("{:.1} {name}", bytes as f64 / 1048576.))
        .collect()
}

/// Heap and working set at named moments of the start, read by `perf start`.
static CHECKPOINTS: std::sync::Mutex<Vec<(&'static str, usize, u64)>> =
    std::sync::Mutex::new(Vec::new());

pub fn checkpoint(name: &'static str) {
    let (working, _) = memory();
    if let Ok(mut list) = CHECKPOINTS.lock() {
        list.push((name, heap().0, working));
    }
}

pub fn checkpoints() -> String {
    let Ok(list) = CHECKPOINTS.lock() else {
        return String::new();
    };
    list.iter()
        .map(|(name, heap, working)| format!("cp_{name}_heap={heap} cp_{name}_ws={working}"))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Bytes the Rust heap holds now, and its high-water mark.
pub fn heap() -> (usize, usize) {
    (
        HEAP_LIVE.load(Ordering::Relaxed),
        HEAP_PEAK.load(Ordering::Relaxed),
    )
}

/// Committed bytes of the address space by kind: executable images, mapped files and sections, private memory. The
/// first three are the committed totals; the last is the largest private regions, `size@kind`.
pub fn committed() -> (u64, u64, u64) {
    use windows::Win32::System::Memory::{
        MEM_COMMIT, MEM_IMAGE, MEM_MAPPED, MEMORY_BASIC_INFORMATION, VirtualQuery,
    };
    let (mut image, mut mapped, mut private) = (0u64, 0u64, 0u64);
    let mut address = 0usize;
    loop {
        let mut info = MEMORY_BASIC_INFORMATION::default();
        let read = unsafe {
            VirtualQuery(
                Some(address as *const _),
                &mut info,
                std::mem::size_of::<MEMORY_BASIC_INFORMATION>(),
            )
        };
        if read == 0 {
            break;
        }
        if info.State == MEM_COMMIT {
            let size = info.RegionSize as u64;
            if info.Type == MEM_IMAGE {
                image += size;
            } else if info.Type == MEM_MAPPED {
                mapped += size;
            } else {
                private += size;
            }
        }
        let next = (info.BaseAddress as usize).saturating_add(info.RegionSize);
        if next <= address {
            break;
        }
        address = next;
    }
    (image, mapped, private)
}

/// Whether this run is a perf measurement (its script starts with `perf`): it behaves as the app does where a capture
/// build would keep something only for later captures.
static MEASURING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn measuring() -> bool {
    MEASURING.load(Ordering::Relaxed)
}

pub fn set_measuring(script: &str) {
    MEASURING.store(script.trim_start().starts_with("perf"), Ordering::Relaxed);
}

/// Root renders so far: the count between two samples says whether an idle window keeps redrawing.
pub static RENDERS: AtomicU64 = AtomicU64::new(0);

/// Whether the run asked for the behaviour a fix replaced (`GRAVECORD_PERF_LEGACY=members,blink,badge`), so a
/// before and an after can be measured with one binary.
pub fn legacy(name: &str) -> bool {
    static LIST: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    LIST.get_or_init(|| std::env::var("GRAVECORD_PERF_LEGACY").unwrap_or_default())
        .split(',')
        .any(|item| item == name)
}

fn ticks(time: FILETIME) -> u64 {
    (u64::from(time.dwHighDateTime) << 32) | u64::from(time.dwLowDateTime)
}

/// Milliseconds since this process was created, from the operating system's own creation time.
pub fn since_process_start_ms() -> u64 {
    let (mut created, mut exit, mut kernel, mut user) = (
        FILETIME::default(),
        FILETIME::default(),
        FILETIME::default(),
        FILETIME::default(),
    );
    let started = unsafe {
        GetProcessTimes(
            GetCurrentProcess(),
            &mut created,
            &mut exit,
            &mut kernel,
            &mut user,
        )
    };
    if started.is_err() {
        return 0;
    }
    let now = ticks(unsafe { GetSystemTimeAsFileTime() });
    now.saturating_sub(ticks(created)) / 10_000
}

/// Records `slot` once, the first time it is asked.
pub fn mark(slot: &AtomicU64) {
    if slot.load(Ordering::Relaxed) == 0 {
        slot.store(since_process_start_ms().max(1), Ordering::Relaxed);
    }
}

/// CPU time of the process, kernel and user together, in milliseconds.
pub fn cpu_ms() -> u64 {
    let (mut created, mut exit, mut kernel, mut user) = (
        FILETIME::default(),
        FILETIME::default(),
        FILETIME::default(),
        FILETIME::default(),
    );
    if unsafe {
        GetProcessTimes(
            GetCurrentProcess(),
            &mut created,
            &mut exit,
            &mut kernel,
            &mut user,
        )
    }
    .is_err()
    {
        return 0;
    }
    (ticks(kernel) + ticks(user)) / 10_000
}

/// Working set and private bytes of the process.
pub fn memory() -> (u64, u64) {
    let mut counters = PROCESS_MEMORY_COUNTERS {
        cb: std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32,
        ..Default::default()
    };
    let read = unsafe { GetProcessMemoryInfo(GetCurrentProcess(), &mut counters, counters.cb) };
    if read.is_err() {
        return (0, 0);
    }
    // The pagefile usage of a process is its private (committed) bytes.
    (
        counters.WorkingSetSize as u64,
        counters.PagefileUsage as u64,
    )
}

/// Threads in this process.
pub fn threads() -> usize {
    let process = unsafe { GetCurrentProcessId() };
    let Ok(snapshot) = (unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) }) else {
        return 0;
    };
    let mut entry = THREADENTRY32 {
        dwSize: std::mem::size_of::<THREADENTRY32>() as u32,
        ..Default::default()
    };
    let mut count = 0;
    let mut more = unsafe { Thread32First(snapshot, &mut entry) }.is_ok();
    while more {
        if entry.th32OwnerProcessID == process {
            count += 1;
        }
        more = unsafe { Thread32Next(snapshot, &mut entry) }.is_ok();
    }
    let _ = unsafe { windows::Win32::Foundation::CloseHandle(HANDLE(snapshot.0)) };
    count
}

/// Bytes of a decoded picture as the atlas holds it: 4 per pixel and frame.
pub fn render_bytes(image: &RenderImage) -> usize {
    (0..image.frame_count())
        .map(|frame| {
            let size = image.size(frame);
            size.width.0.max(0) as usize * size.height.0.max(0) as usize * 4
        })
        .sum()
}

/// The count and byte size of one cache, as one log field.
#[derive(Default, Clone, Copy)]
pub struct Size {
    pub items: usize,
    pub bytes: usize,
}

impl Size {
    pub fn add(&mut self, items: usize, bytes: usize) {
        self.items += items;
        self.bytes += bytes;
    }
}

impl std::fmt::Display for Size {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(out, "{}/{}", self.items, self.bytes)
    }
}

/// Median, 95th percentile and worst of `samples`, in milliseconds, with the count over `limit`.
pub fn summary(samples: &mut [f64], limit: f64) -> String {
    if samples.is_empty() {
        return "n=0".to_owned();
    }
    samples.sort_by(f64::total_cmp);
    let at = |share: f64| samples[((samples.len() - 1) as f64 * share).round() as usize];
    let over = samples.iter().filter(|value| **value > limit).count();
    format!(
        "n={} median_ms={:.2} p95_ms={:.2} worst_ms={:.2} over_{limit:.1}ms={over}",
        samples.len(),
        at(0.5),
        at(0.95),
        samples[samples.len() - 1],
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summary_reads_the_percentiles() {
        let mut values: Vec<f64> = (1..=100).map(f64::from).collect();
        let line = summary(&mut values, 50.);
        assert!(line.contains("n=100"));
        assert!(line.contains("worst_ms=100.00"));
        assert!(line.contains("over_50.0ms=50"));
    }

    #[test]
    fn counters_read_something() {
        let (working, private) = memory();
        assert!(working > 0 && private > 0);
        assert!(threads() >= 1);
    }
}
