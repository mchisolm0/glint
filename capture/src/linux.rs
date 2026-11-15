//! The Linux back end: xdg-desktop-portal through ashpd, so it works on any Wayland desktop with a portal
//! (GNOME, KDE). Screenshots come from the Screenshot portal. A recording is a ScreenCast portal session
//! whose PipeWire stream a `gst-launch-1.0` child encodes: to MP4 itself, or to raw RGBA frames on a pipe
//! for the shared GIF encoder or scrolling stitcher.
//!
//! Wayland lets no app read the pointer or other windows, so the portal's own picker stands in for Glint's
//! overlay: `area` with an empty rect and `window` show it, and a recording's first start shows the desktop's
//! chooser (a restore token beside the recording skips it after that). Rects are the portal picture's
//! pixels, origin top-left: at scale 1 that is the desktop's points.

use crate::{frames, gif, scroll, RecordOptions, Rect, Result, Shot, Video, VideoKind};
use ashpd::desktop::screencast::{CursorMode, Screencast, SelectSourcesOptions, SourceType};
use ashpd::desktop::screenshot::Screenshot;
use ashpd::desktop::{PersistMode, Session};
use std::io::Read;
use std::os::fd::{AsRawFd, OwnedFd};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{mpsc, Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

const MP4_FPS: u32 = 30;
/// GIF frames closer together than this are dropped: 15 fps, as on the Mac.
const GIF_FRAME: Duration = Duration::from_micros(1_000_000 / 15);
/// A scrolling capture's frames: twice a GIF's, so a quick scroll still overlaps the last frame.
const SCROLL_FRAME: Duration = Duration::from_micros(1_000_000 / 30);
const GIF_MAX_WIDTH: f64 = 1280.0;
/// How long a recording may take to start, and an MP4 to finish writing.
const PATIENCE: Duration = Duration::from_secs(10);
/// Beside each recording: the portal's restore token, so the next one skips the chooser.
const TOKEN: &str = "screencast.token";

/// A portal failure in words a person can act on. `what` names the request ("the screenshot"); `asked`
/// says whether the person was shown a picker, whose cancel is theirs, or only a permission prompt.
fn refusal(error: ashpd::Error, what: &str, asked: bool) -> String {
    match error {
        // GNOME 50 answers a closed picker and a declined permission prompt alike, with "other" rather
        // than "cancelled", and a declined prompt answers every later request the same way.
        ashpd::Error::Response(_) if asked => format!("{what} was cancelled"),
        ashpd::Error::Response(_) => {
            format!("the desktop refused {what}: allow it in Settings, Privacy and Security, Screenshots")
        }
        ashpd::Error::PortalNotFound(portal) => {
            format!("this desktop has no {portal} portal (install xdg-desktop-portal and its GNOME or KDE back end)")
        }
        ashpd::Error::Zbus(e) => format!("Glint could not reach the desktop portal: {e}"),
        e => format!("{what} failed: {e}"),
    }
}

/// A `file://` URI's path, percent-decoded.
fn file_path(uri: &str) -> Result<PathBuf> {
    let rest = uri.strip_prefix("file://").ok_or_else(|| format!("the portal gave {uri}, not a file"))?;
    // `file://host/path` names a host; only the local one (empty, or localhost) is a path here.
    let path = rest.strip_prefix("localhost").unwrap_or(rest);
    let bytes = path.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = |b: u8| (b as char).to_digit(16);
        match (bytes[i], bytes.get(i + 1).copied().and_then(hex), bytes.get(i + 2).copied().and_then(hex)) {
            (b'%', Some(hi), Some(lo)) => {
                out.push((hi * 16 + lo) as u8);
                i += 3;
            }
            (b, ..) => {
                out.push(b);
                i += 1;
            }
        }
    }
    use std::os::unix::ffi::OsStringExt;
    Ok(PathBuf::from(std::ffi::OsString::from_vec(out)))
}

/// The portal's screenshot of the whole desktop, or with `interactive` what the person picked in the
/// desktop's own area, window and screen picker.
fn portal_png(interactive: bool) -> Result<PathBuf> {
    let shot = pollster::block_on(async {
        Screenshot::request().interactive(interactive).modal(false).send().await?.response()
    })
    .map_err(|e| refusal(e, "the screenshot", interactive))?;
    file_path(shot.uri().as_str())
}

/// A PNG as straight-alpha RGBA.
fn decode(path: &Path) -> Result<(Vec<u8>, u32, u32)> {
    let file = std::fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut decoder = png::Decoder::new(std::io::BufReader::new(file));
    decoder.set_transformations(png::Transformations::normalize_to_color8() | png::Transformations::ALPHA);
    let mut reader = decoder.read_info().map_err(|e| format!("{}: {e}", path.display()))?;
    let mut buf = vec![0; reader.output_buffer_size().ok_or("the screenshot is too large")?];
    let info = reader.next_frame(&mut buf).map_err(|e| format!("{}: {e}", path.display()))?;
    buf.truncate(info.buffer_size());
    let rgba = match info.color_type {
        png::ColorType::Rgba => buf,
        png::ColorType::GrayscaleAlpha => buf.chunks_exact(2).flat_map(|p| [p[0], p[0], p[0], p[1]]).collect(),
        other => return Err(format!("the screenshot decoded as {other:?}, not RGBA")),
    };
    Ok((rgba, info.width, info.height))
}

/// `rect` of an RGBA picture, clipped to it, in whole pixels.
fn crop(rgba: &[u8], width: u32, height: u32, rect: Rect) -> Option<(Vec<u8>, u32, u32)> {
    let frame = Rect { x: 0.0, y: 0.0, width: f64::from(width), height: f64::from(height) };
    let r = frame.intersect(&rect)?;
    let (x, y) = (r.x.round() as u32, r.y.round() as u32);
    let (w, h) = ((r.width.round() as u32).min(width - x), (r.height.round() as u32).min(height - y));
    let mut out = Vec::with_capacity((w * h * 4) as usize);
    for row in y..y + h {
        let start = ((row * width + x) * 4) as usize;
        out.extend_from_slice(&rgba[start..start + (w * 4) as usize]);
    }
    Some((out, w, h))
}

/// Moves the portal's PNG to `to` (cropped to `rect` when one is given) and writes its mosaic. The
/// portal wrote the file for this request alone, so it is removed once copied.
fn keep(from: &Path, rect: Option<Rect>, to: &Path) -> Result<Shot> {
    let kept = (|| {
        let (rgba, width, height) = decode(from)?;
        let (rgba, width, height) = match rect {
            Some(rect) => {
                let (part, w, h) = crop(&rgba, width, height, rect).ok_or("the area is off the screen")?;
                crate::write_png(to, &part, w, h)?;
                (part, w, h)
            }
            // The portal's own bytes, colour profile and all.
            None => {
                std::fs::copy(from, to).map_err(|e| format!("{}: {e}", to.display()))?;
                (rgba, width, height)
            }
        };
        let mosaic = crate::write_mosaic(to, &rgba, width, height)?;
        Ok(Shot { width, height, mosaic })
    })();
    let _ = std::fs::remove_file(from);
    kept
}

/// The whole desktop: Wayland does not say which display the pointer is on.
pub(crate) fn screen(to: &Path) -> Result<Shot> {
    keep(&portal_png(false)?, None, to)
}

/// `rect` of the desktop, or with an empty rect the area the person picks in the portal's picker.
pub(crate) fn area(rect: Rect, to: &Path) -> Result<Shot> {
    if rect.width >= 1.0 && rect.height >= 1.0 {
        return keep(&portal_png(false)?, Some(rect), to);
    }
    keep(&portal_png(true)?, None, to)
}

/// Wayland has no window ids for apps, so `id` is unused: the person picks the window in the portal.
pub(crate) fn window(_id: u32, to: &Path) -> Result<Shot> {
    keep(&portal_png(true)?, None, to)
}

/// A started ScreenCast session: its PipeWire remote and the one stream picked.
struct Cast {
    session: Session<Screencast>,
    remote: OwnedFd,
    node: u32,
    /// The stream's size and position in the desktop's logical pixels (a window has no position).
    size: (u32, u32),
    position: (i32, i32),
}

/// Asks the ScreenCast portal for one monitor (or, without an area, a monitor or a window), with the
/// cursor drawn in when `cursor`. A token from the last recording restores its pick without the chooser;
/// the new one is kept for the next.
fn cast(cursor: bool, monitor_only: bool, token: Option<&Path>) -> Result<Cast> {
    let restore = token
        .and_then(|t| std::fs::read_to_string(t).ok())
        .map(|t| t.trim().to_owned())
        .filter(|t| !t.is_empty());
    let started = pollster::block_on(async {
        let proxy = Screencast::new().await?;
        let modes = proxy.available_cursor_modes().await?;
        let cursor_mode = match cursor && modes.contains(CursorMode::EMBEDDED) {
            true => CursorMode::EMBEDDED,
            false => CursorMode::HIDDEN,
        };
        let sources = match monitor_only {
            true => SourceType::MONITOR,
            false => SourceType::MONITOR | SourceType::WINDOW,
        };
        let session = proxy.create_session(Default::default()).await?;
        let options = SelectSourcesOptions::default()
            .set_cursor_mode(cursor_mode)
            .set_sources(sources)
            .set_multiple(false)
            .set_persist_mode(if token.is_some() { PersistMode::ExplicitlyRevoked } else { PersistMode::DoNot })
            .set_restore_token(restore.as_deref());
        proxy.select_sources(&session, options).await?.response()?;
        let streams = proxy.start(&session, None, Default::default()).await?.response()?;
        let remote = proxy.open_pipe_wire_remote(&session, Default::default()).await?;
        Ok::<_, ashpd::Error>((session, streams, remote))
    });
    let (session, streams, remote) = started.map_err(|e| refusal(e, "the recording", true))?;
    if let (Some(next), Some(token)) = (streams.restore_token(), token) {
        let _ = std::fs::write(token, next);
    }
    let Some((stream, (w, h))) = streams.streams().first().and_then(|s| Some((s, s.size()?))) else {
        let _ = pollster::block_on(session.close());
        return Err("the portal started a recording with no stream, or one with no size".into());
    };
    Ok(Cast {
        node: stream.pipe_wire_node_id(),
        size: (w.max(1) as u32, h.max(1) as u32),
        position: stream.position().unwrap_or((0, 0)),
        session,
        remote,
    })
}

/// What a recording keeps of its stream, in the stream's pixels: videocrop's four margins and the size
/// left. H.264 wants even sides, so `even` trims a pixel off an odd one.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Crop {
    left: u32,
    top: u32,
    right: u32,
    bottom: u32,
    width: u32,
    height: u32,
}

/// `area` (desktop pixels, `None` for all of it) of a `size` stream at `position`, clipped to the stream.
fn region(size: (u32, u32), position: (i32, i32), area: Option<Rect>, even: bool) -> Option<Crop> {
    let frame = Rect { x: 0.0, y: 0.0, width: f64::from(size.0), height: f64::from(size.1) };
    let r = match area {
        Some(a) => frame.intersect(&Rect { x: a.x - f64::from(position.0), y: a.y - f64::from(position.1), ..a })?,
        None => frame,
    };
    let (left, top) = (r.x.round() as u32, r.y.round() as u32);
    let mut width = (r.width.round() as u32).min(size.0 - left);
    let mut height = (r.height.round() as u32).min(size.1 - top);
    if even {
        (width, height) = (width & !1, height & !1);
    }
    (width > 0 && height > 0).then_some(Crop {
        left,
        top,
        right: size.0 - left - width,
        bottom: size.1 - top - height,
        width,
        height,
    })
}

/// The GIF's size: the region at one pixel per point, shrunk to fit GIF_MAX_WIDTH.
fn gif_size(crop: &Crop) -> (u32, u32) {
    let fit = (GIF_MAX_WIDTH / f64::from(crop.width)).min(1.0);
    let side = |n: u32| ((f64::from(n) * fit).round() as u32).max(1);
    (side(crop.width), side(crop.height))
}

/// Where the encoded video goes.
enum Output<'a> {
    /// H.264 in MP4 at `path`, with the named audio sources (pulsesrc devices) mixed into AAC.
    Mp4 { path: &'a Path, encoder: &'a str, aac: &'a str, audio: &'a [&'a str] },
    /// Raw RGBA frames of this size on stdout.
    Raw { width: u32, height: u32 },
}

/// `gst-launch-1.0`'s arguments: `source` (the PipeWire stream, or a test source), scaled to the
/// stream's logical `size` (so a resized window or a HiDPI buffer keeps one frame size), cropped, then
/// encoded to `output`.
fn pipeline(source: &[String], size: (u32, u32), crop: &Crop, output: &Output) -> Result<Vec<String>> {
    let mut args: Vec<String> = words(&["-e", "-q"]).chain(source.iter().cloned()).collect();
    args.extend(words(&["!", "videoconvert", "!", "videoscale", "!"]));
    args.push(format!("video/x-raw,width={},height={},pixel-aspect-ratio=1/1", size.0, size.1));
    args.extend(words(&["!", "videocrop"]));
    let margins = [("left", crop.left), ("top", crop.top), ("right", crop.right), ("bottom", crop.bottom)];
    args.extend(margins.iter().map(|(k, v)| format!("{k}={v}")));
    match output {
        Output::Mp4 { path, encoder, aac, audio } => {
            let path = path.to_str().filter(|p| !p.contains('"')).ok_or("the recording's path cannot be quoted")?;
            // skip-to-first: no copies of the first frame back to when the pipeline started.
            args.extend(words(&[
                "!",
                "videorate",
                "skip-to-first=true",
                "!",
                &format!("video/x-raw,framerate={MP4_FPS}/1"),
            ]));
            args.extend(words(&["!", "videoconvert", "!", "video/x-raw,format=I420", "!", "queue", "!"]));
            // About 3 bits per pixel per second: enough for crisp text, which screen content is.
            let bits = u64::from(crop.width) * u64::from(crop.height) * 3;
            args.extend(match *encoder {
                "x264enc" => words(&["x264enc", &format!("bitrate={}", bits / 1000), "speed-preset=veryfast"])
                    .collect::<Vec<_>>(),
                _ => words(&[encoder, &format!("bitrate={bits}"), "usage-type=screen", "complexity=low"]).collect(),
            });
            args.extend(words(&["!", "h264parse", "!", "queue", "!", "mp4mux", "name=mux", "!", "filesink"]));
            // Unbuffered, so the file grows with the first frame, which is how `start` knows frames flow.
            args.push("buffer-mode=unbuffered".into());
            args.push(format!("location=\"{path}\""));
            if !audio.is_empty() {
                args.extend(words(&[
                    "audiomixer",
                    "name=mix",
                    "!",
                    "audioconvert",
                    "!",
                    aac,
                    "!",
                    "queue",
                    "!",
                    "mux.",
                ]));
                for device in *audio {
                    args.extend(words(&["pulsesrc", &format!("device={device}"), "do-timestamp=true", "!"]));
                    args.extend(words(&["audioconvert", "!", "audioresample", "!", "queue", "!", "mix."]));
                }
            }
        }
        Output::Raw { width, height } => {
            args.extend(words(&["!", "videoscale", "!", "videoconvert", "!"]));
            args.push(format!("video/x-raw,format=RGBA,width={width},height={height},pixel-aspect-ratio=1/1"));
            args.extend(words(&["!", "fdsink", "fd=1", "sync=false"]));
        }
    }
    Ok(args)
}

fn words<'a>(items: &'a [&'a str]) -> impl Iterator<Item = String> + 'a {
    items.iter().map(|s| s.to_string())
}

/// Whether GStreamer has `element`.
fn has(element: &str) -> bool {
    Command::new("gst-inspect-1.0").args(["--exists", element]).status().is_ok_and(|s| s.success())
}

/// The first of `elements` GStreamer has.
fn first<'a>(elements: &[&'a str], what: &str) -> Result<&'a str> {
    elements
        .iter()
        .copied()
        .find(|e| has(e))
        .ok_or_else(|| format!("GStreamer has no {what} ({})", elements.join(" or ")))
}

/// The `gst-launch-1.0` child. Dropping it kills it.
struct Gst {
    child: Child,
    /// The end of what it printed to stderr, for an error.
    errors: Arc<Mutex<String>>,
}

impl Gst {
    fn spawn(args: &[String], remote: &OwnedFd, stdout: Stdio) -> Result<Gst> {
        // The portal's fd is close-on-exec; the child needs it as itself.
        // SAFETY: fcntl on an fd we own, changing only its descriptor flags.
        if unsafe { libc::fcntl(remote.as_raw_fd(), libc::F_SETFD, 0) } != 0 {
            return Err(format!("the PipeWire remote: {}", std::io::Error::last_os_error()));
        }
        let mut child = Command::new("gst-launch-1.0")
            .args(args)
            .stdin(Stdio::null())
            .stdout(stdout)
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("recording needs GStreamer's gst-launch-1.0 with its PipeWire plugin: {e}"))?;
        let errors = Arc::new(Mutex::new(String::new()));
        let (mut stderr, tail) = (child.stderr.take().expect("piped"), errors.clone());
        std::thread::spawn(move || {
            let mut buf = [0; 1024];
            while let Ok(n @ 1..) = stderr.read(&mut buf) {
                let mut tail = tail.lock().unwrap_or_else(|e| e.into_inner());
                tail.push_str(&String::from_utf8_lossy(&buf[..n]));
                if tail.len() > 4096 {
                    let cut = tail.len() - 4096;
                    let cut = (cut..tail.len()).find(|&i| tail.is_char_boundary(i)).unwrap_or(0);
                    tail.drain(..cut);
                }
            }
        });
        Ok(Gst { child, errors })
    }

    fn failed(&self, what: &str) -> String {
        let errors = self.errors.lock().unwrap_or_else(|e| e.into_inner());
        match errors.trim() {
            "" => what.to_owned(),
            e => format!("{what}: {e}"),
        }
    }

    /// Waits up to PATIENCE for the child to exit; whether it exited cleanly.
    fn wait(&mut self) -> Result<bool> {
        let until = Instant::now() + PATIENCE;
        while Instant::now() < until {
            if let Some(status) = self.child.try_wait().map_err(|e| e.to_string())? {
                return Ok(status.success());
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        Err(self.failed("GStreamer never finished"))
    }

    /// Asks for EOS, as Control-C does with `-e`, so mp4mux writes its index.
    fn interrupt(&self) {
        // SAFETY: a signal to our own child, whose pid we hold until it is reaped.
        unsafe { libc::kill(self.child.id() as libc::pid_t, libc::SIGINT) };
    }
}

impl Drop for Gst {
    fn drop(&mut self) {
        if let Ok(None) = self.child.try_wait() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

enum Sink {
    Mp4 { path: PathBuf },
    Frames { encoder: Option<frames::Encoder>, reader: Option<JoinHandle<()>> },
}

pub(crate) struct Recording {
    gst: Gst,
    sink: Sink,
    session: Session<Screencast>,
    width: u32,
    height: u32,
    started: Instant,
}

impl Recording {
    pub(crate) fn stop(mut self) -> Result<Video> {
        let end = Instant::now();
        let (width, height) = (self.width, self.height);
        match &mut self.sink {
            Sink::Mp4 { path } => {
                self.gst.interrupt();
                let clean = self.gst.wait()?;
                let bytes = std::fs::metadata(&*path).map_or(0, |m| m.len());
                if !clean || bytes == 0 {
                    return Err(self.gst.failed("the MP4 did not finish writing"));
                }
                Ok(Video { width, height, duration_ms: end.duration_since(self.started).as_millis() as u64 })
            }
            Sink::Frames { encoder, reader } => {
                // Nothing to finish on GStreamer's side: the frames are ours.
                let _ = self.gst.child.kill();
                let _ = self.gst.child.wait();
                if let Some(reader) = reader.take() {
                    let _ = reader.join();
                }
                encoder.take().ok_or("the recording already finished")?.finish(end)
            }
        }
    }
}

impl Drop for Recording {
    fn drop(&mut self) {
        let _ = pollster::block_on(self.session.close());
    }
}

pub(crate) fn record(options: &RecordOptions, to: &Path) -> Result<Recording> {
    let started = start(options, to);
    if started.is_err() {
        let _ = std::fs::remove_file(to);
    }
    started
}

fn start(options: &RecordOptions, to: &Path) -> Result<Recording> {
    let audio: Vec<&str> = [(options.audio, "@DEFAULT_MONITOR@"), (options.microphone, "@DEFAULT_SOURCE@")]
        .into_iter()
        .filter_map(|(on, device)| (on && options.kind == VideoKind::Mp4).then_some(device))
        .collect();
    // Checked before the portal asks anything of the person.
    let (encoder, aac) = match options.kind {
        VideoKind::Mp4 => (
            first(&["openh264enc", "x264enc"], "H.264 encoder")?,
            if audio.is_empty() { "" } else { first(&["fdkaacenc", "avenc_aac", "voaacenc"], "AAC encoder")? },
        ),
        VideoKind::Gif | VideoKind::Scroll => ("", ""),
    };
    if !has("pipewiresrc") {
        return Err("GStreamer has no PipeWire source (pipewiresrc): install its PipeWire plugin".into());
    }
    let token = to.with_file_name(TOKEN);
    // A scrolling capture asks every time, since each one is likely a different window.
    let scroll = options.kind == VideoKind::Scroll;
    let cast = cast(options.cursor && !scroll, options.area.is_some(), (!scroll).then_some(token.as_path()))?;
    let even = options.kind == VideoKind::Mp4;
    let crop = region(cast.size, cast.position, options.area, even).ok_or("the area is off the recorded screen")?;
    let source = vec![
        "pipewiresrc".into(),
        format!("fd={}", cast.remote.as_raw_fd()),
        format!("path={}", cast.node),
        "do-timestamp=true".into(),
        // pipewiresrc sees EOS only with its next buffer, and a still screen sends none, so an MP4 has the
        // last one resent every 100 ms. A GIF's frames stop with a kill, and a still screen stays one frame.
        format!("keepalive-time={}", if even { 100 } else { 0 }),
        // Sharing stopped from the desktop's own indicator still finishes the file.
        "on-disconnect=eos".into(),
    ];
    match options.kind {
        VideoKind::Mp4 => {
            let output = Output::Mp4 { path: to, encoder, aac, audio: &audio };
            let gst = Gst::spawn(&pipeline(&source, cast.size, &crop, &output)?, &cast.remote, Stdio::null())?;
            drop(cast.remote);
            let mut recording = Recording {
                gst,
                sink: Sink::Mp4 { path: to.to_owned() },
                session: cast.session,
                width: crop.width,
                height: crop.height,
                started: Instant::now(),
            };
            // Frames are flowing once mp4mux has written something.
            let until = Instant::now() + PATIENCE;
            while std::fs::metadata(to).map_or(0, |m| m.len()) == 0 {
                if recording.gst.child.try_wait().map_err(|e| e.to_string())?.is_some() {
                    return Err(recording.gst.failed("the recording failed to start"));
                }
                if Instant::now() > until {
                    return Err(recording.gst.failed("the recording never started"));
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            recording.started = Instant::now();
            Ok(recording)
        }
        VideoKind::Gif | VideoKind::Scroll => {
            // A scrolling capture keeps every pixel.
            let (width, height) = if options.kind == VideoKind::Gif { gif_size(&crop) } else { (crop.width, crop.height) };
            let output = Output::Raw { width, height };
            let mut gst = Gst::spawn(&pipeline(&source, cast.size, &crop, &output)?, &cast.remote, Stdio::piped())?;
            drop(cast.remote);
            let (frames, encoder) = match options.kind {
                VideoKind::Scroll => scroll::start(to, width, height)?,
                _ => gif::start(to, width, height)?,
            };
            let stdout = gst.child.stdout.take().expect("piped");
            let every = if options.kind == VideoKind::Scroll { SCROLL_FRAME } else { GIF_FRAME };
            let (first_frame, flowing) = mpsc::channel();
            let reader = std::thread::Builder::new()
                .name("glint-gif-frames".into())
                .spawn(move || read_frames(stdout, frames, (width * height * 4) as usize, every, first_frame))
                .map_err(|e| e.to_string())?;
            let mut recording = Recording {
                gst,
                sink: Sink::Frames { encoder: Some(encoder), reader: Some(reader) },
                session: cast.session,
                width,
                height,
                started: Instant::now(),
            };
            if flowing.recv_timeout(PATIENCE).is_err() {
                return Err(recording.gst.failed("the recording never started"));
            }
            recording.started = Instant::now();
            Ok(recording)
        }
    }
}

/// Reads fixed-size RGBA frames until the pipe closes, passing on at most one per `every`. The first
/// frame is announced on `first`.
fn read_frames(mut pipe: impl Read, frames: frames::Frames, size: usize, every: Duration, first: mpsc::Sender<()>) {
    let mut last: Option<Instant> = None;
    loop {
        let mut frame = vec![0; size];
        if pipe.read_exact(&mut frame).is_err() {
            return;
        }
        let now = Instant::now();
        if last.is_some_and(|t| now.duration_since(t) < every) {
            continue;
        }
        if last.is_none() {
            let _ = first.send(());
        }
        last = Some(now);
        frames.push(frame, now);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_uri_is_a_decoded_path() {
        assert_eq!(
            file_path("file:///tmp/a%20b/Screenshot%E2%80%931.png").unwrap(),
            Path::new("/tmp/a b/Screenshot–1.png")
        );
        assert_eq!(file_path("file://localhost/tmp/x.png").unwrap(), Path::new("/tmp/x.png"));
        // A stray percent stays as it is.
        assert_eq!(file_path("file:///tmp/100%.png").unwrap(), Path::new("/tmp/100%.png"));
        assert!(file_path("https://example.com/x.png").is_err());
    }

    #[test]
    fn a_crop_clips_to_the_picture() {
        // 4 × 2, each pixel's red its index.
        let rgba: Vec<u8> = (0..8).flat_map(|i| [i, 0, 0, 255]).collect();
        let (part, w, h) = crop(&rgba, 4, 2, Rect { x: 2.0, y: 1.0, width: 10.0, height: 10.0 }).unwrap();
        assert_eq!((w, h), (2, 1));
        assert_eq!(part.chunks(4).map(|p| p[0]).collect::<Vec<_>>(), [6, 7]);
        assert!(crop(&rgba, 4, 2, Rect { x: 5.0, y: 0.0, width: 2.0, height: 2.0 }).is_none());
    }

    #[test]
    fn a_region_moves_to_the_stream_and_evens_its_sides() {
        let whole = region((1280, 800), (0, 0), None, true).unwrap();
        assert_eq!((whole.width, whole.height, whole.right, whole.bottom), (1280, 800, 0, 0));
        // A second monitor at x 1280: desktop x 1300 is 20 into its stream. 301 × 201 evens to 300 × 200.
        let area = Rect { x: 1300.0, y: 10.0, width: 301.0, height: 201.0 };
        let r = region((1920, 1080), (1280, 0), Some(area), true).unwrap();
        assert_eq!(r, Crop { left: 20, top: 10, right: 1600, bottom: 870, width: 300, height: 200 });
        // A GIF keeps odd sides; past the edge it clips.
        let edge = Rect { x: 1250.0, y: 790.0, width: 100.0, height: 100.0 };
        assert_eq!(region((1280, 800), (0, 0), Some(edge), false).map(|c| (c.width, c.height)), Some((30, 10)));
        assert_eq!(region((1280, 800), (0, 0), Some(Rect { x: 2000.0, ..edge }), false), None);
    }

    #[test]
    fn a_gif_shrinks_to_its_cap() {
        let wide = region((2560, 1440), (0, 0), None, false).unwrap();
        assert_eq!(gif_size(&wide), (1280, 720));
        let small = region((640, 400), (0, 0), None, false).unwrap();
        assert_eq!(gif_size(&small), (640, 400));
    }

    fn test_source() -> Vec<String> {
        ["videotestsrc", "num-buffers=45", "is-live=true"].map(String::from).to_vec()
    }

    #[test]
    fn an_mp4_pipeline_crops_encodes_and_mixes_audio() {
        let crop =
            region((1280, 800), (0, 0), Some(Rect { x: 10.0, y: 20.0, width: 641.0, height: 401.0 }), true).unwrap();
        let output = Output::Mp4 {
            path: Path::new("/tmp/g/1.mp4"),
            encoder: "openh264enc",
            aac: "fdkaacenc",
            audio: &["@DEFAULT_MONITOR@", "@DEFAULT_SOURCE@"],
        };
        let line = pipeline(&test_source(), (1280, 800), &crop, &output).unwrap().join(" ");
        assert!(line.contains("videocrop left=10 top=20 right=630 bottom=380"), "{line}");
        assert!(line.contains("openh264enc bitrate=768000 usage-type=screen"), "{line}");
        assert!(line.contains("filesink buffer-mode=unbuffered location=\"/tmp/g/1.mp4\""), "{line}");
        assert_eq!(line.matches("pulsesrc").count(), 2, "{line}");
        assert!(line.contains("audiomixer name=mix ! audioconvert ! fdkaacenc ! queue ! mux."), "{line}");
        let silent = Output::Mp4 { path: Path::new("/tmp/g/1.mp4"), encoder: "openh264enc", aac: "", audio: &[] };
        assert!(!pipeline(&test_source(), (1280, 800), &crop, &silent).unwrap().join(" ").contains("audiomixer"));
        let quoted = Output::Mp4 { path: Path::new("/tmp/\"x.mp4"), encoder: "openh264enc", aac: "", audio: &[] };
        assert!(pipeline(&test_source(), (1280, 800), &crop, &quoted).is_err());
    }

    /// Runs the real pipelines over GStreamer's test sources, so it needs gst-launch-1.0, openh264enc and
    /// an AAC encoder, but no desktop: `cargo test -- --ignored gstreamer`.
    #[test]
    #[ignore]
    fn gstreamer_runs_the_pipelines() {
        let dir = std::env::temp_dir().join(format!("glint-gst-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let crop =
            region((320, 240), (0, 0), Some(Rect { x: 10.0, y: 10.0, width: 101.0, height: 81.0 }), true).unwrap();
        let mp4 = dir.join("t.mp4");
        let aac = first(&["fdkaacenc", "avenc_aac", "voaacenc"], "AAC encoder").unwrap();
        let output = Output::Mp4 { path: &mp4, encoder: "openh264enc", aac, audio: &[] };
        let mut args = pipeline(&test_source(), (320, 240), &crop, &output).unwrap();
        // The audio branch, with test tones in place of the desktop's devices.
        args.extend(
            ["audiomixer", "name=mix", "!", "audioconvert", "!", aac, "!", "queue", "!", "mux."].map(String::from),
        );
        for wave in ["sine", "square"] {
            args.extend(
                [
                    "audiotestsrc",
                    "num-buffers=60",
                    "is-live=true",
                    &format!("wave={wave}"),
                    "!",
                    "audioconvert",
                    "!",
                    "queue",
                    "!",
                    "mix.",
                ]
                .map(String::from),
            );
        }
        let status = Command::new("gst-launch-1.0").args(&args).status().unwrap();
        assert!(status.success() && std::fs::metadata(&mp4).unwrap().len() > 1000, "{}", args.join(" "));
        let probe = Command::new("ffprobe")
            .args(["-v", "error", "-show_entries", "stream=codec_name,width,height", "-of", "csv=p=0"])
            .arg(&mp4)
            .output()
            .unwrap();
        let streams = String::from_utf8_lossy(&probe.stdout);
        println!("{streams}");
        assert!(streams.contains("h264,100,80") && streams.contains("aac"), "{streams}");

        let raw = region((320, 240), (0, 0), None, false).unwrap();
        let output = Output::Raw { width: 160, height: 120 };
        let mut child = Command::new("gst-launch-1.0")
            .args(pipeline(&test_source(), (320, 240), &raw, &output).unwrap())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let mut bytes = Vec::new();
        child.stdout.take().unwrap().read_to_end(&mut bytes).unwrap();
        assert!(child.wait().unwrap().success());
        assert_eq!(bytes.len(), 45 * 160 * 120 * 4);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
