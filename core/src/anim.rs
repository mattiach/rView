//! Animations: GIF, APNG, animated WebP, JPEG XL. Frames are decoded on a thread
//! from the file bytes already in memory and sent through a bounded channel. While the total stays
//! ≤ 256 MB the player keeps every frame and the thread stops after one pass; above that the player
//! streams, and the thread loops, decoding again from the compressed bytes.
//! The player is driven by explicit `now` values (testable, UI uses `WaitUntil(next_due)`).

use crate::color::{ViewSpace, ViewTransform, finish_view};
use crate::formats::Format;
use image::{AnimationDecoder, ImageDecoder};
use std::collections::VecDeque;
use std::io::Cursor;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, SyncSender, TryRecvError, sync_channel};
use std::time::{Duration, Instant};

/// Frames kept in memory up to this many bytes.
pub const STORE_LIMIT: u64 = 256 << 20;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Loops {
    Infinite,
    /// Total number of plays.
    Finite(u32),
}

fn loops_from(l: image::metadata::LoopCount) -> Loops {
    match l {
        image::metadata::LoopCount::Infinite => Loops::Infinite,
        image::metadata::LoopCount::Finite(n) => Loops::Finite(n.get()),
    }
}

/// Composited frame in the source colour space, straight alpha, full canvas size.
pub struct SourceFrame {
    pub rgba: Vec<u8>,
    pub w: u32,
    pub h: u32,
    pub delay: Duration,
}

/// Delays ≤ 10 ms are played as 100 ms, like browsers (many GIFs rely on it).
fn clamp_delay(ms: f64) -> Duration {
    Duration::from_secs_f64(if ms <= 10.0 { 0.1 } else { ms / 1000.0 })
}

type FrameIter<'a> = Box<dyn Iterator<Item = Result<SourceFrame, String>> + 'a>;

fn frames(f: image::Frames<'_>) -> FrameIter<'_> {
    Box::new(f.map(|r| {
        r.map_err(|e| e.to_string()).map(|f| {
            let (n, d) = f.delay().numer_denom_ms();
            let delay = clamp_delay(n as f64 / d.max(1) as f64);
            let buf = f.into_buffer();
            SourceFrame {
                w: buf.width(),
                h: buf.height(),
                rgba: buf.into_raw(),
                delay,
            }
        })
    }))
}

/// One decoding pass over the frames, with the ICC profile and loop count.
fn open(bytes: &[u8], format: Format) -> Result<(Option<Vec<u8>>, Loops, FrameIter<'_>), String> {
    let e = |e: image::ImageError| e.to_string();
    match format {
        Format::Gif => {
            let d = image::codecs::gif::GifDecoder::new(Cursor::new(bytes)).map_err(e)?;
            let loops = loops_from(d.loop_count());
            Ok((None, loops, frames(d.into_frames())))
        }
        Format::Png => {
            let mut d = image::codecs::png::PngDecoder::new(Cursor::new(bytes)).map_err(e)?;
            let icc = d.icc_profile().ok().flatten();
            let a = d.apng().map_err(e)?;
            let loops = loops_from(a.loop_count());
            Ok((icc, loops, frames(a.into_frames())))
        }
        Format::Webp => {
            let mut d = image::codecs::webp::WebPDecoder::new(Cursor::new(bytes)).map_err(e)?;
            let icc = d.icc_profile().ok().flatten();
            let loops = loops_from(d.loop_count());
            Ok((icc, loops, frames(d.into_frames())))
        }
        Format::Jxl => open_jxl(bytes),
        _ => Err("not an animated format".into()),
    }
}

// Validated on the libjxl conformance animation: 36 frames, durations as in its test.json.
fn open_jxl(bytes: &[u8]) -> Result<(Option<Vec<u8>>, Loops, FrameIter<'_>), String> {
    let image = jxl_oxide::JxlImage::builder()
        .read(Cursor::new(bytes))
        .map_err(|e| e.to_string())?;
    let (tps, loops) = match &image.image_header().metadata.animation {
        Some(a) if a.tps_numerator > 0 => (
            a.tps_numerator as f64 / a.tps_denominator.max(1) as f64,
            if a.num_loops == 0 {
                Loops::Infinite
            } else {
                Loops::Finite(a.num_loops)
            },
        ),
        _ => (1000.0, Loops::Infinite),
    };
    let icc = Some(image.rendered_icc());
    let n = image.num_loaded_keyframes();
    let iter = (0..n).map(move |k| {
        let r = image.render_frame(k).map_err(|e| e.to_string())?;
        let mut s = r.stream();
        let (w, h, ch) = (s.width(), s.height(), s.channels() as usize);
        let mut buf = vec![0u8; w as usize * h as usize * ch];
        s.write_to_buffer(&mut buf);
        let rgba = crate::decode::expand_to_rgba(&buf, ch, u8::MAX);
        Ok(SourceFrame {
            rgba,
            w,
            h,
            delay: clamp_delay(r.duration() as f64 * 1000.0 / tps),
        })
    });
    Ok((icc, loops, Box::new(iter)))
}

/// More than one frame (play automatically only then).
pub fn is_animated(bytes: &[u8], format: Format) -> bool {
    match format {
        Format::Gif => open(bytes, format).is_ok_and(|(_, _, mut it)| it.next().is_some() && it.next().is_some()),
        Format::Png => {
            image::codecs::png::PngDecoder::new(Cursor::new(bytes))
                .and_then(|d| d.is_apng())
                .unwrap_or(false)
                && open(bytes, format).is_ok_and(|(_, _, mut it)| it.next().is_some() && it.next().is_some())
        }
        Format::Webp => image::codecs::webp::WebPDecoder::new(Cursor::new(bytes)).is_ok_and(|d| d.has_animation()),
        Format::Jxl => jxl_oxide::JxlImage::builder()
            .read(Cursor::new(bytes))
            .is_ok_and(|i| i.image_header().metadata.animation.is_some() && i.num_loaded_keyframes() > 1),
        _ => false,
    }
}

/// Frame `n` (0-based) in the source colour space, for "Save Frame As", plus the ICC.
pub fn source_frame(bytes: &[u8], format: Format, n: usize) -> Result<(SourceFrame, Option<Vec<u8>>), String> {
    let (icc, _, mut it) = open(bytes, format)?;
    let f = it.nth(n).ok_or("frame out of range")??;
    Ok((f, icc))
}

/// Frame `n` encoded as PNG with the source ICC profile (Save Frame As).
pub fn frame_png(bytes: &[u8], format: Format, n: usize) -> Result<Vec<u8>, String> {
    use image::ImageEncoder;
    let (f, icc) = source_frame(bytes, format, n)?;
    let mut out = Vec::new();
    let mut e = image::codecs::png::PngEncoder::new(&mut out);
    if let Some(icc) = icc {
        let _ = e.set_icc_profile(icc);
    }
    e.write_image(&f.rgba, f.w, f.h, image::ExtendedColorType::Rgba8)
        .map_err(|e| e.to_string())?;
    Ok(out)
}

/// Frame ready for upload (view space, premultiplied).
pub struct ViewFrame {
    pub rgba: Vec<u8>,
    pub delay: Duration,
}

enum Msg {
    Frame(ViewFrame),
    /// End of a pass, with the loop count of the file.
    End(Loops),
    Error(String),
}

fn decoder_thread(
    bytes: Arc<[u8]>,
    format: Format,
    space: ViewSpace,
    limit: u64,
    tx: SyncSender<Msg>,
    stop: Arc<AtomicBool>,
    wake: Arc<dyn Fn() + Send + Sync>,
) {
    let send = |m: Msg| {
        let ok = tx.send(m).is_ok();
        wake();
        ok
    };
    let mut first_pass_bytes = 0u64;
    let mut pass = 0;
    loop {
        let (icc, loops, frames) = match open(&bytes, format) {
            Ok(x) => x,
            Err(e) => {
                send(Msg::Error(e));
                return;
            }
        };
        let t = ViewTransform::new(icc.as_deref(), space);
        for f in frames {
            if stop.load(Ordering::Relaxed) {
                return;
            }
            let f = match f {
                Ok(f) => f,
                Err(e) => {
                    send(Msg::Error(e));
                    return;
                }
            };
            if pass == 0 {
                first_pass_bytes += f.rgba.len() as u64;
            }
            let vf = ViewFrame {
                rgba: finish_view(f.rgba, &t),
                delay: f.delay,
            };
            if !send(Msg::Frame(vf)) {
                return; // player dropped
            }
        }
        if !send(Msg::End(loops)) || first_pass_bytes <= limit {
            return; // the player keeps every frame: no further passes
        }
        pass += 1;
    }
}

/// Playback state of one animated image (owned by the window).
pub struct Player {
    rx: Receiver<Msg>,
    stop: Arc<AtomicBool>,
    /// Stored frames (all of them once `complete`), empty while streaming.
    frames: Vec<Arc<ViewFrame>>,
    stored_bytes: u64,
    limit: u64,
    streaming: bool,
    /// Streaming mode: decoded frames (and end-of-pass marks) not yet shown, in order.
    queue: VecDeque<PendingFrame>,
    complete: bool,
    loops: Option<Loops>,
    plays: u32,
    finished: bool,
    /// Index of the frame on screen (counted within the current pass).
    index: usize,
    current: Option<Arc<ViewFrame>>,
    due: Option<Instant>,
    start: Instant,
    pub paused: bool,
    /// 0–1000 %, step 25.
    pub speed: u32,
    pub error: Option<String>,
    /// Frames presented since the start (for tests / diagnostics).
    pub shown: u64,
}

impl Drop for Player {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

impl Player {
    /// Frame 0 is assumed to be on screen already (the loader showed it); playback starts at `start`.
    pub fn spawn(
        bytes: Arc<[u8]>,
        format: Format,
        space: ViewSpace,
        start: Instant,
        wake: Arc<dyn Fn() + Send + Sync>,
    ) -> Player {
        Player::spawn_with_limit(bytes, format, space, start, wake, STORE_LIMIT)
    }

    /// `spawn` with another storage limit (tests exercise the streaming path with a tiny one).
    pub fn spawn_with_limit(
        bytes: Arc<[u8]>,
        format: Format,
        space: ViewSpace,
        start: Instant,
        wake: Arc<dyn Fn() + Send + Sync>,
        limit: u64,
    ) -> Player {
        let (tx, rx) = sync_channel(4); // bounded: a paused or slow player never piles up frames
        let stop = Arc::new(AtomicBool::new(false));
        let s = stop.clone();
        std::thread::Builder::new()
            .name("anim".into())
            .spawn(move || decoder_thread(bytes, format, space, limit, tx, s, wake))
            .expect("spawn anim thread");
        Player {
            rx,
            stop,
            frames: Vec::new(),
            stored_bytes: 0,
            limit,
            streaming: false,
            queue: VecDeque::new(),
            complete: false,
            loops: None,
            plays: 0,
            finished: false,
            index: 0,
            current: None,
            due: None,
            start,
            paused: false,
            speed: 100,
            error: None,
            shown: 0,
        }
    }

    /// Known once the first pass has been stored completely.
    pub fn frame_count(&mut self) -> Option<usize> {
        self.pump();
        self.complete.then_some(self.frames.len())
    }

    pub fn is_streaming(&mut self) -> bool {
        self.pump();
        self.streaming
    }

    pub fn index(&self) -> usize {
        self.index
    }

    /// Receive what the decoder produced. Storing mode drains everything; streaming mode keeps at
    /// most one frame ahead (the channel bound applies the back-pressure).
    fn pump(&mut self) {
        loop {
            if self.streaming && !self.queue.is_empty() {
                return;
            }
            match self.rx.try_recv() {
                Ok(Msg::Frame(f)) => {
                    let f = Arc::new(f);
                    if self.current.is_none() {
                        // frame 0: already on screen
                        self.due = Some(self.start + self.scaled(f.delay));
                        self.current = Some(f.clone());
                    }
                    if self.streaming {
                        if !Arc::ptr_eq(self.current.as_ref().unwrap(), &f) {
                            self.queue.push_back(PendingFrame::Frame(f));
                        }
                        continue;
                    }
                    self.stored_bytes += f.rgba.len() as u64;
                    if self.stored_bytes > self.limit {
                        // Too big to keep: stream from now on (the decoder keeps looping). Frames
                        // decoded ahead of playback stay queued, in order.
                        self.streaming = true;
                        let ahead = self.frames.drain(..).skip(self.index + 1);
                        self.queue.extend(ahead.map(PendingFrame::Frame));
                        if !Arc::ptr_eq(self.current.as_ref().unwrap(), &f) {
                            self.queue.push_back(PendingFrame::Frame(f)); // frame 0 is on screen already
                        }
                    } else {
                        self.frames.push(f);
                    }
                }
                Ok(Msg::End(l)) => {
                    self.loops = Some(l);
                    if self.streaming {
                        self.queue.push_back(PendingFrame::End);
                    } else {
                        self.complete = true;
                    }
                }
                Ok(Msg::Error(e)) => {
                    self.error = Some(e);
                    return;
                }
                Err(TryRecvError::Empty | TryRecvError::Disconnected) => return,
            }
        }
    }

    fn scaled(&self, d: Duration) -> Duration {
        d.mul_f64(100.0 / self.speed.max(1) as f64)
    }

    /// When the window should wake up next (None: nothing scheduled).
    pub fn next_due(&mut self) -> Option<Instant> {
        self.pump();
        if self.paused || self.speed == 0 || self.finished || self.error.is_some() {
            return None;
        }
        self.due
    }

    /// Advance if the next frame is due; returns the frame to upload.
    pub fn tick(&mut self, now: Instant) -> Option<Arc<ViewFrame>> {
        self.pump();
        if self.paused || self.speed == 0 || self.finished || self.due.is_none_or(|d| now < d) {
            return None;
        }
        self.step(now)
    }

    /// "Next Frame" (N): one frame forward, whatever the clock says.
    pub fn next_frame(&mut self, now: Instant) -> Option<Arc<ViewFrame>> {
        self.pump();
        self.finished = false;
        self.step(now)
    }

    fn step(&mut self, now: Instant) -> Option<Arc<ViewFrame>> {
        let next = if self.streaming {
            match self.queue.pop_front() {
                Some(PendingFrame::Frame(f)) => {
                    self.index += 1;
                    Some(f)
                }
                Some(PendingFrame::End) => {
                    self.end_of_pass();
                    self.pump();
                    return if self.finished { None } else { self.step(now) };
                }
                None => None, // decoder behind: try again on the next wake
            }
        } else if self.index + 1 < self.frames.len() {
            self.index += 1;
            Some(self.frames[self.index].clone())
        } else if self.complete && self.frames.len() > 1 {
            self.end_of_pass();
            if self.finished {
                return None;
            }
            self.index = 0;
            Some(self.frames[0].clone())
        } else {
            None
        }?;
        // Keep the cadence unless we fell behind by more than a frame.
        let base = self
            .due
            .filter(|d| now.saturating_duration_since(*d) < next.delay)
            .unwrap_or(now);
        self.due = Some(base + self.scaled(next.delay));
        self.current = Some(next.clone());
        self.shown += 1;
        Some(next)
    }

    fn end_of_pass(&mut self) {
        self.plays += 1;
        if let Some(Loops::Finite(n)) = self.loops
            && self.plays >= n
        {
            self.finished = true;
            if self.streaming {
                self.stop.store(true, Ordering::Relaxed);
            }
        }
        if self.streaming {
            self.index = 0;
        }
    }

    pub fn toggle_pause(&mut self, now: Instant) {
        self.paused = !self.paused;
        if !self.paused {
            self.finished = false;
            self.due = Some(now);
        }
    }

    /// [ ] \ : −25 %, +25 %, reset to 100 % (limits 0–1000 %).
    pub fn change_speed(&mut self, delta: i32) {
        self.speed = (self.speed as i32 + delta).clamp(0, 1000) as u32;
    }

    pub fn reset_speed(&mut self) {
        self.speed = 100;
    }
}

enum PendingFrame {
    Frame(Arc<ViewFrame>),
    End,
}
