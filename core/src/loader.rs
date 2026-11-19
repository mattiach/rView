//! Background loader: priority queue + W worker threads, one worker always free for the
//! current image, cancellation tokens checked between phases, shared byte-budget view cache.
//! UI-free: results go to a callback (the app wraps `EventLoopProxy::send_event`).

use crate::cache::Lru;
use crate::decode::{self, DecodeOptions, Header};
use crate::formats::Format;
use std::collections::{BinaryHeap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering as AtOrd};
use std::sync::{Arc, Condvar, Mutex};
use std::time::SystemTime;

/// Above this, a cheap reduced preview is shown first when the format has one.
pub const PREVIEW_PIXELS: u64 = 24_000_000;

pub type ClientId = u64;
pub type CancelToken = Arc<AtomicBool>;

/// Buffer ready for upload (view space, premultiplied).
#[derive(Debug)]
pub struct ViewImage {
    pub w: u32,
    pub h: u32,
    pub rgba: Vec<u8>,
    /// Displayed size of the full image. Equal to (w, h) except for previews, which the window
    /// draws stretched to this size so the full image replaces them without a zoom jump.
    pub full: (u32, u32),
    pub preview: bool,
    pub info: ImageInfo,
    /// Whole file, kept for animated images: the player decodes the frames from it.
    pub animation: Option<Arc<[u8]>>,
}

/// Facts for the Info dialog that come out of decoding.
#[derive(Clone, Debug)]
pub struct ImageInfo {
    pub format: Format,
    /// ICC description, "Embedded ICC profile" if it has none, or "sRGB (assumed)".
    pub profile: String,
    pub profile_assumed: bool,
    /// HDR / float content shown as SDR.
    pub hdr: bool,
    /// File size in bytes (title "Verbose", Info).
    pub file_size: u64,
    /// Bits per channel of the decoded pixels (8 or 16).
    pub bit_depth: u8,
    /// EXIF present, and with GPS position tags; None = not read for this format.
    pub exif: Option<(bool, bool)>,
}

impl ImageInfo {
    pub fn of(img: &decode::DecodedImage) -> ImageInfo {
        let profile = match (&img.color.icc, img.color.assumed) {
            (Some(icc), _) => crate::color::icc_description(icc).unwrap_or_else(|| "Embedded ICC profile".into()),
            (None, true) => "sRGB (assumed)".into(),
            (None, false) => "sRGB".into(),
        };
        ImageInfo {
            format: img.format,
            profile,
            profile_assumed: img.color.icc.is_none(),
            hdr: img.color.hdr,
            file_size: 0,
            bit_depth: match img.pixels {
                decode::Pixels::Rgba8(_) => 8,
                decode::Pixels::Rgba16(_) => 16,
            },
            exif: img.meta.exif_read.then(|| {
                let gps = img.meta.exif.as_deref().is_some_and(has_gps);
                (img.meta.exif.is_some(), gps)
            }),
        }
    }
}

/// EXIF block (TIFF structure, as `image` returns it) with a GPS position (latitude tag).
fn has_gps(exif: &[u8]) -> bool {
    exif::Reader::new()
        .read_raw(exif.to_vec())
        .is_ok_and(|e| e.fields().any(|f| f.tag == exif::Tag::GPSLatitude))
}

#[derive(Clone, Debug)]
pub struct LoadError {
    pub message: String,
    /// "(Error <n>)" shown to the user (decode codes from `DecodeError::code`, 5 = I/O).
    pub code: u32,
    /// The file is gone: navigation skips it in the direction of travel.
    pub not_found: bool,
}

pub struct Outcome {
    pub client: ClientId,
    pub generation: u64,
    pub path: PathBuf,
    pub result: Result<Arc<ViewImage>, LoadError>,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Options {
    pub decode: DecodeOptions,
    /// View colour space; part of the cache key.
    pub view_space: crate::color::ViewSpace,
}

/// Preload request: distance from the current file and whether it lies behind the direction of travel.
#[derive(Clone, Debug)]
pub struct Preload {
    pub path: PathBuf,
    pub distance: u8,
    pub behind: bool,
}

type Key = (PathBuf, u64, SystemTime, u32);

enum Work {
    Current {
        bypass_cache: bool,
        preview_side: u32,
    },
    /// Full decode after a preview was sent; bytes already read.
    CurrentFull {
        bytes: Arc<[u8]>,
        key: Key,
        format: Format,
    },
    Preload,
}

struct Job {
    id: u64,
    /// Lower is more urgent: (class, distance, behind). Current.Preview 0, Current.Full 1, Preload 2.
    prio: (u8, u8, bool),
    client: ClientId,
    generation: u64,
    path: PathBuf,
    work: Work,
    cancel: CancelToken,
}

impl Job {
    fn is_preload(&self) -> bool {
        matches!(self.work, Work::Preload)
    }
}

impl PartialEq for Job {
    fn eq(&self, o: &Self) -> bool {
        self.id == o.id
    }
}
impl Eq for Job {}
impl PartialOrd for Job {
    fn partial_cmp(&self, o: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(o))
    }
}
impl Ord for Job {
    /// Max-heap: the most urgent job (smallest prio, then oldest id) is the greatest.
    fn cmp(&self, o: &Self) -> std::cmp::Ordering {
        (o.prio, o.id).cmp(&(self.prio, self.id))
    }
}

struct ActiveJob {
    client: ClientId,
    path: PathBuf,
    preload: bool,
    cancel: CancelToken,
}

struct Queue {
    heap: BinaryHeap<Job>,
    /// Queued or running jobs, for cancellation and preload de-duplication.
    active: HashMap<u64, ActiveJob>,
    running_preloads: usize,
    next_id: u64,
    shutdown: bool,
}

type Deliver = Box<dyn Fn(Outcome) + Send + Sync>;

struct Shared {
    queue: Mutex<Queue>,
    cv: Condvar,
    cache: Mutex<Lru<Key, Arc<ViewImage>>>,
    options: Mutex<Options>,
    workers: usize,
    deliver: Deliver,
}

pub struct Loader {
    shared: Arc<Shared>,
    threads: Vec<std::thread::JoinHandle<()>>,
}

/// W = clamp(cpu − 1, 2, 6).
pub fn default_workers() -> usize {
    let cpus = std::thread::available_parallelism().map_or(2, |n| n.get());
    cpus.saturating_sub(1).clamp(2, 6)
}

impl Loader {
    pub fn new(
        workers: usize,
        cache_budget: u64,
        options: Options,
        deliver: impl Fn(Outcome) + Send + Sync + 'static,
    ) -> Loader {
        let workers = workers.max(2);
        let shared = Arc::new(Shared {
            queue: Mutex::new(Queue {
                heap: BinaryHeap::new(),
                active: HashMap::new(),
                running_preloads: 0,
                next_id: 0,
                shutdown: false,
            }),
            cv: Condvar::new(),
            cache: Mutex::new(Lru::new(cache_budget)),
            options: Mutex::new(options),
            workers,
            deliver: Box::new(deliver),
        });
        let threads = (0..workers)
            .map(|i| {
                let s = shared.clone();
                std::thread::Builder::new()
                    .name(format!("loader-{i}"))
                    .spawn(move || worker(&s))
                    .expect("spawn loader worker")
            })
            .collect();
        Loader { shared, threads }
    }

    pub fn set_options(&self, o: Options) {
        *self.shared.options.lock().unwrap() = o;
    }

    pub fn set_cache_budget(&self, budget: u64) {
        self.shared.cache.lock().unwrap().set_budget(budget);
    }

    /// New current image for `client` plus the files to preload around it. Cancels the client's
    /// older current jobs and its preloads that are not wanted any more. A reduced preview,
    /// when used, has its long side ≥ `preview_side` (the window's viewport, physical px).
    pub fn request(
        &self,
        client: ClientId,
        generation: u64,
        path: PathBuf,
        bypass_cache: bool,
        preview_side: u32,
        preloads: Vec<Preload>,
    ) {
        let mut q = self.shared.queue.lock().unwrap();
        update(&mut q, client, generation, preloads, true);
        push(
            &mut q,
            client,
            generation,
            path,
            Work::Current {
                bypass_cache,
                preview_side,
            },
            (0, 0, false),
        );
        drop(q);
        self.shared.cv.notify_all();
    }

    /// Replace the preload set of `client` without touching its current job (e.g. once the
    /// folder scan finishes after the current image was requested).
    pub fn set_preloads(&self, client: ClientId, generation: u64, preloads: Vec<Preload>) {
        let mut q = self.shared.queue.lock().unwrap();
        update(&mut q, client, generation, preloads, false);
        drop(q);
        self.shared.cv.notify_all();
    }

    /// Window closed: cancel everything of `client`.
    pub fn cancel_client(&self, client: ClientId) {
        let mut q = self.shared.queue.lock().unwrap();
        for job in q.active.values().filter(|j| j.client == client) {
            job.cancel.store(true, AtOrd::Relaxed);
        }
        drop_cancelled(&mut q);
    }

    /// Drop cached entries of `path` (after save/overwrite).
    pub fn invalidate(&self, path: &Path) {
        let canon = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        self.shared.cache.lock().unwrap().retain(|k| k.0 != canon);
    }

    pub fn is_cached(&self, path: &Path) -> bool {
        let vs = self.shared.options.lock().unwrap().view_space.id();
        cache_key(path, vs).is_ok_and(|k| self.shared.cache.lock().unwrap().contains(&k))
    }

    /// Jobs queued or running (tests / idle detection).
    pub fn pending(&self) -> usize {
        self.shared.queue.lock().unwrap().active.len()
    }
}

impl Drop for Loader {
    fn drop(&mut self) {
        {
            let mut q = self.shared.queue.lock().unwrap();
            q.shutdown = true;
            for j in q.active.values() {
                j.cancel.store(true, AtOrd::Relaxed);
            }
        }
        self.shared.cv.notify_all();
        for t in self.threads.drain(..) {
            let _ = t.join();
        }
    }
}

/// Cancel the client's preloads not in `preloads` (and its current jobs if asked), then queue the
/// preloads that are not already queued or running.
fn update(q: &mut Queue, client: ClientId, generation: u64, preloads: Vec<Preload>, cancel_current: bool) {
    let mut kept = Vec::new();
    for job in q.active.values().filter(|j| j.client == client) {
        if job.preload && preloads.iter().any(|p| p.path == job.path) {
            kept.push(job.path.clone());
        } else if job.preload || cancel_current {
            job.cancel.store(true, AtOrd::Relaxed);
        }
    }
    drop_cancelled(q);
    for p in preloads.into_iter().filter(|p| !kept.contains(&p.path)) {
        push(q, client, generation, p.path, Work::Preload, (2, p.distance, p.behind));
    }
}

/// Remove cancelled jobs that haven't started; running ones leave `active` when their worker ends.
fn drop_cancelled(q: &mut Queue) {
    let (keep, gone): (Vec<Job>, Vec<Job>) = std::mem::take(&mut q.heap)
        .into_vec()
        .into_iter()
        .partition(|j| !j.cancel.load(AtOrd::Relaxed));
    q.heap = keep.into();
    for j in gone {
        q.active.remove(&j.id);
    }
}

fn push(q: &mut Queue, client: ClientId, generation: u64, path: PathBuf, work: Work, prio: (u8, u8, bool)) {
    q.next_id += 1;
    let id = q.next_id;
    let cancel: CancelToken = Arc::default();
    let preload = matches!(work, Work::Preload);
    q.active.insert(
        id,
        ActiveJob {
            client,
            path: path.clone(),
            preload,
            cancel: cancel.clone(),
        },
    );
    q.heap.push(Job {
        id,
        prio,
        client,
        generation,
        path,
        work,
        cancel,
    });
}

fn worker(s: &Shared) {
    loop {
        let job = {
            let mut q = s.queue.lock().unwrap();
            loop {
                if q.shutdown {
                    return;
                }
                // Preloads may use at most W−1 workers: one is always free for the current image.
                let runnable = q
                    .heap
                    .peek()
                    .is_some_and(|j| !j.is_preload() || q.running_preloads < s.workers - 1);
                if runnable {
                    break;
                }
                q = s.cv.wait(q).unwrap();
            }
            let job = q.heap.pop().unwrap();
            if job.is_preload() {
                q.running_preloads += 1;
            }
            job
        };
        let preload = job.is_preload();
        let id = job.id;
        run(s, job);
        let mut q = s.queue.lock().unwrap();
        q.active.remove(&id);
        if preload {
            q.running_preloads -= 1;
        }
        drop(q);
        s.cv.notify_all();
    }
}

fn cache_key(path: &Path, view_space: u32) -> std::io::Result<Key> {
    let md = std::fs::metadata(path)?;
    let canon = std::fs::canonicalize(path)?;
    Ok((
        canon,
        md.len(),
        md.modified().unwrap_or(SystemTime::UNIX_EPOCH),
        view_space,
    ))
}

fn io_error(e: std::io::Error) -> LoadError {
    LoadError {
        message: e.to_string(),
        code: 5,
        not_found: matches!(e.kind(), std::io::ErrorKind::NotFound),
    }
}

fn decode_error(e: decode::DecodeError) -> LoadError {
    LoadError {
        message: e.to_string(),
        code: e.code(),
        not_found: false,
    }
}

fn to_view(
    img: decode::DecodedImage,
    full: Option<(u32, u32)>,
    preview: bool,
    space: crate::color::ViewSpace,
    animation: Option<Arc<[u8]>>,
    file_size: usize,
) -> ViewImage {
    let (w, h) = (img.size.w, img.size.h);
    let info = ImageInfo {
        file_size: file_size as u64,
        ..ImageInfo::of(&img)
    };
    ViewImage {
        w,
        h,
        rgba: crate::color::into_view(img, space),
        full: full.unwrap_or((w, h)),
        preview,
        info,
        animation,
    }
}

fn run(s: &Shared, job: Job) {
    let opts = *s.options.lock().unwrap();
    let cancelled = || job.cancel.load(AtOrd::Relaxed);
    let deliver = |result: Result<Arc<ViewImage>, LoadError>| {
        if !cancelled() && !job.is_preload() {
            (s.deliver)(Outcome {
                client: job.client,
                generation: job.generation,
                path: job.path.clone(),
                result,
            });
        }
    };
    let ext = Format::from_path(&job.path);

    // Full decode after a preview: bytes are already in memory.
    if let Work::CurrentFull { bytes, key, format } = &job.work {
        let r = full_decode(s, bytes, Some(*format), &opts, key.clone(), &cancelled);
        if let Some(r) = r {
            deliver(r);
        }
        return;
    }

    // 1. cache
    let key = match cache_key(&job.path, opts.view_space.id()) {
        Ok(k) => k,
        Err(e) => return deliver(Err(io_error(e))),
    };
    match job.work {
        Work::Current { bypass_cache: true, .. } => s.cache.lock().unwrap().remove(&key),
        _ => {
            if let Some(hit) = s.cache.lock().unwrap().get(&key) {
                return deliver(Ok(hit));
            }
        }
    }
    if cancelled() {
        return;
    }
    let t0 = std::time::Instant::now();
    // 2. read the whole file: no handle stays open (rename/delete/overwrite always possible)
    let bytes: Arc<[u8]> = match std::fs::read(&job.path) {
        Ok(b) => b.into(),
        Err(e) => return deliver(Err(io_error(e))),
    };
    if cancelled() {
        return;
    }
    // 3. header
    let header: Option<Header> = decode::probe(&bytes, ext);
    if cancelled() {
        return;
    }
    if job.is_preload() {
        // Don't preload what would take more than half of the budget.
        let budget = s.cache.lock().unwrap().budget();
        let est = header.map_or(0, |h| h.size.w as u64 * h.size.h as u64 * 4);
        if est > budget / 2 {
            return;
        }
    }
    // 4. preview for big current files
    if let (&Work::Current { preview_side, .. }, Some(h)) = (&job.work, header)
        && h.size.w as u64 * h.size.h as u64 > PREVIEW_PIXELS
        && matches!(h.format, Format::Jpeg | Format::Jxl)
    {
        let mut q = s.queue.lock().unwrap();
        if !cancelled() {
            let full = Work::CurrentFull {
                bytes: bytes.clone(),
                key: key.clone(),
                format: h.format,
            };
            // Own token, registered under the same client: `request` cancels it like the others.
            push(
                &mut q,
                job.client,
                job.generation,
                job.path.clone(),
                full,
                (1, 0, false),
            );
        }
        drop(q);
        s.cv.notify_all();
        log::debug!("{}: read+header {:?}", job.path.display(), t0.elapsed());
        if let Some(p) = decode::decode_preview(&bytes, h.format, preview_side)
            && !cancelled()
        {
            log::debug!("preview {}x{} decoded at {:?}", p.size.w, p.size.h, t0.elapsed());
            let v = to_view(p, Some((h.size.w, h.size.h)), true, opts.view_space, None, bytes.len());
            log::debug!("preview ready at {:?}", t0.elapsed());
            deliver(Ok(Arc::new(v)));
        }
        return;
    }
    // 5. full decode (6. preloads end here, cached only)
    if let Some(r) = full_decode(s, &bytes, ext, &opts, key, &cancelled) {
        deliver(r);
    }
}

fn full_decode(
    s: &Shared,
    bytes: &Arc<[u8]>,
    ext: Option<Format>,
    opts: &Options,
    key: Key,
    cancelled: &dyn Fn() -> bool,
) -> Option<Result<Arc<ViewImage>, LoadError>> {
    let t = std::time::Instant::now();
    let img = match decode::decode(bytes, ext, &opts.decode) {
        Ok(i) => i,
        Err(e) => return Some(Err(decode_error(e))),
    };
    let t_decode = t.elapsed();
    if cancelled() {
        log::debug!("cancelled after decode ({t_decode:?})");
        return None;
    }
    // First frame now; the player streams the others from the same bytes.
    let animation = crate::anim::is_animated(bytes, img.format).then(|| bytes.clone());
    let view = Arc::new(to_view(img, None, false, opts.view_space, animation, bytes.len()));
    log::debug!("decode {t_decode:?}, view ready at {:?}", t.elapsed());
    if cancelled() {
        return None;
    }
    let cost = view.rgba.len() as u64 + view.animation.as_ref().map_or(0, |b| b.len() as u64);
    s.cache.lock().unwrap().insert(key, view.clone(), cost);
    Some(Ok(view))
}
