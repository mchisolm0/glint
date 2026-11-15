//! The macOS back end: ScreenCaptureKit through the `screencapturekit` crate. Screenshots use
//! SCScreenshotManager; recordings use SCStream, with SCRecordingOutput writing MP4 and frame callbacks
//! feeding the shared GIF encoder. Every display filter excludes Glint's own app by pid.

use crate::{gif, RecordOptions, Rect, Result, Shot, Video, VideoKind};
use screencapturekit::cg::CGRect;
use screencapturekit::cm::SCFrameStatus;
use screencapturekit::cv::CVPixelBufferLockFlags;
use screencapturekit::prelude::*;
use screencapturekit::recording_output::{
    RecordingCallbacks, SCRecordingOutput, SCRecordingOutputCodec, SCRecordingOutputConfiguration,
    SCRecordingOutputFileType,
};
use screencapturekit::screenshot_manager::{
    CGImage, CGImageExt, ImageFormat, SCScreenshotConfiguration, SCScreenshotManager,
};
use screencapturekit::shareable_content::SCShareableContentInfo;
use std::ffi::c_void;
use std::path::Path;
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

const MP4_FPS: u32 = 60;
const GIF_FPS: u32 = 15;
const GIF_MAX_WIDTH: f64 = 1280.0;
/// How long a recording may take to start, and an MP4 to finish writing.
const PATIENCE: Duration = Duration::from_secs(10);

/// The pointer, in global points with the origin top-left. ScreenCaptureKit has no pointer API, so this
/// asks CoreGraphics: an event created with no source carries the current location.
fn pointer() -> (f64, f64) {
    #[repr(C)]
    struct Point {
        x: f64,
        y: f64,
    }
    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGEventCreate(source: *const c_void) -> *mut c_void;
        fn CGEventGetLocation(event: *mut c_void) -> Point;
    }
    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFRelease(object: *const c_void);
    }
    // SAFETY: CGEventCreate(NULL) returns a +1 event or null; we read it and release it once.
    unsafe {
        let event = CGEventCreate(std::ptr::null());
        if event.is_null() {
            return (0.0, 0.0);
        }
        let p = CGEventGetLocation(event);
        CFRelease(event);
        (p.x, p.y)
    }
}

/// Everything ScreenCaptureKit may capture. The first call is what asks for Screen Recording permission.
fn content() -> Result<SCShareableContent> {
    SCShareableContent::get().map_err(|e| format!("ScreenCaptureKit could not list the screen's content: {e}"))
}

fn rect(r: CGRect) -> Rect {
    Rect { x: r.origin.x, y: r.origin.y, width: r.size.width, height: r.size.height }
}

/// The display holding `point`, or the first one when none does.
fn display_at(content: &SCShareableContent, point: (f64, f64)) -> Result<SCDisplay> {
    let displays = content.displays();
    let found = displays.iter().position(|d| rect(d.frame()).contains(point)).unwrap_or(0);
    displays.into_iter().nth(found).ok_or_else(|| "there is no display".to_string())
}

/// The display, minus every window of Glint's own process: the overlays, panels and main window.
fn display_filter(content: &SCShareableContent, display: &SCDisplay) -> Result<SCContentFilter> {
    let pid = std::process::id() as i32;
    let apps = content.applications();
    let ours: Vec<&SCRunningApplication> = apps.iter().filter(|a| a.process_id() == pid).collect();
    SCContentFilter::create()
        .with_display(display)
        .with_excluding_applications(&ours, &[])
        .build()
        .map_err(|e| e.to_string())
}

/// Pixels per point for what `filter` captures.
fn scale(filter: &SCContentFilter) -> Result<f64> {
    SCShareableContentInfo::for_filter(filter)
        .map(|info| f64::from(info.point_pixel_scale()))
        .ok_or_else(|| "ScreenCaptureKit gave no scale for the display".to_string())
}

/// Points to whole pixels, at least one.
fn pixels(points: f64, scale: f64) -> u32 {
    ((points * scale).round() as u32).max(1)
}

/// `rect` relative to `display`'s origin, which is what `sourceRect` takes.
fn source_rect(rect: &Rect, display: &Rect) -> CGRect {
    CGRect::new(rect.x - display.x, rect.y - display.y, rect.width, rect.height)
}

fn snap(filter: &SCContentFilter, config: &SCStreamConfiguration, to: &Path) -> Result<Shot> {
    let image = SCScreenshotManager::capture_image(filter, config).map_err(|e| e.to_string())?;
    save(&image, to)
}

/// Writes the PNG with ImageIO, which keeps the display's colour profile, then the mosaic beside it.
fn save(image: &CGImage, to: &Path) -> Result<Shot> {
    let path = to.to_str().ok_or_else(|| format!("{} is not UTF-8", to.display()))?;
    image.save(path, ImageFormat::Png).map_err(|e| e.to_string())?;
    let (width, height) = (image.width() as u32, image.height() as u32);
    let mut rgba = image.rgba_data().map_err(|e| e.to_string())?;
    unpremultiply(&mut rgba);
    let mosaic = crate::write_mosaic(to, &rgba, width, height)?;
    Ok(Shot { width, height, mosaic })
}

/// CoreGraphics renders premultiplied RGBA; the shared encoders take straight alpha.
fn unpremultiply(rgba: &mut [u8]) {
    for p in rgba.as_chunks_mut::<4>().0 {
        let a = u16::from(p[3]);
        if a != 0 && a != 255 {
            for c in &mut p[..3] {
                *c = ((u16::from(*c) * 255 + a / 2) / a).min(255) as u8;
            }
        }
    }
}

pub(crate) fn screen(to: &Path) -> Result<Shot> {
    let content = content()?;
    let display = display_at(&content, pointer())?;
    let filter = display_filter(&content, &display)?;
    let (frame, scale) = (rect(display.frame()), scale(&filter)?);
    let config = SCStreamConfiguration::new()
        .with_width(pixels(frame.width, scale))
        .with_height(pixels(frame.height, scale))
        .with_shows_cursor(false);
    snap(&filter, &config, to)
}

pub(crate) fn area(area: Rect, to: &Path) -> Result<Shot> {
    let content = content()?;
    let display = display_at(&content, area.centre())?;
    let frame = rect(display.frame());
    let clipped = frame.intersect(&area).ok_or_else(|| "the area is off every display".to_string())?;
    let filter = display_filter(&content, &display)?;
    let scale = scale(&filter)?;
    let config = SCStreamConfiguration::new()
        .with_source_rect(source_rect(&clipped, &frame))
        .with_width(pixels(clipped.width, scale))
        .with_height(pixels(clipped.height, scale))
        .with_shows_cursor(false);
    snap(&filter, &config, to)
}

pub(crate) fn window(id: u32, to: &Path) -> Result<Shot> {
    let window = content()?
        .windows()
        .into_iter()
        .find(|w| w.window_id() == id)
        .ok_or_else(|| format!("window {id} is not on screen"))?;
    // Desktop-independent: the window alone, wherever it is, with nothing behind it.
    let filter = SCContentFilter::create().with_window(&window).build().map_err(|e| e.to_string())?;
    // Only macOS 26's screenshot configuration widens the picture to hold the shadow. An SCStreamConfiguration
    // sized to the window keeps the shadow off, so before 26 the picture is the window without one.
    if let Ok(config) = SCScreenshotConfiguration::new() {
        let config = config.with_ignore_shadows(false).with_shows_cursor(false);
        let output = SCScreenshotManager::capture_screenshot(&filter, &config).map_err(|e| e.to_string())?;
        let image = output.sdr_image().ok_or_else(|| "the window screenshot has no image".to_string())?;
        return save(&image, to);
    }
    let info = SCShareableContentInfo::for_filter(&filter)
        .ok_or_else(|| "ScreenCaptureKit gave no size for the window".to_string())?;
    let (width, height) = info.pixel_size();
    let config = SCStreamConfiguration::new()
        .with_width(width)
        .with_height(height)
        .with_shows_cursor(false)
        .with_should_be_opaque(false)
        .map_err(|e| e.to_string())?;
    snap(&filter, &config, to)
}

enum Event {
    Started,
    Finished,
    Failed(String),
}

enum Sink {
    Mp4 { output: SCRecordingOutput, events: Receiver<Event>, started: Instant },
    Gif(gif::Encoder),
}

pub(crate) struct Recording {
    stream: SCStream,
    sink: Sink,
    width: u32,
    height: u32,
}

impl Recording {
    pub(crate) fn stop(self) -> Result<Video> {
        let end = Instant::now();
        let stopped = self.stream.stop_capture().map_err(|e| e.to_string());
        let (width, height) = (self.width, self.height);
        match self.sink {
            Sink::Gif(encoder) => {
                stopped?;
                encoder.finish(end)
            }
            Sink::Mp4 { output, events, started } => {
                stopped?;
                loop {
                    match events.recv_timeout(PATIENCE) {
                        Ok(Event::Finished) => break,
                        Ok(Event::Started) => continue,
                        Ok(Event::Failed(e)) => return Err(format!("the recording failed: {e}")),
                        Err(RecvTimeoutError::Timeout) => return Err("the MP4 never finished writing".into()),
                        Err(RecvTimeoutError::Disconnected) => return Err("the recording vanished".into()),
                    }
                }
                let recorded = output.recorded_duration();
                let duration_ms = match recorded.timescale {
                    t if t > 0 && recorded.value > 0 => (recorded.value as f64 * 1000.0 / f64::from(t)) as u64,
                    _ => end.duration_since(started).as_millis() as u64,
                };
                Ok(Video { width, height, duration_ms })
            }
        }
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
    let content = content()?;
    let display = display_at(&content, options.area.map_or_else(pointer, |a| a.centre()))?;
    let frame = rect(display.frame());
    let region = match options.area {
        Some(area) => frame.intersect(&area).ok_or_else(|| "the area is off every display".to_string())?,
        None => frame,
    };
    let filter = display_filter(&content, &display)?;
    let mut config = SCStreamConfiguration::new().with_shows_cursor(options.cursor);
    // Click rings arrived in macOS 15; on 14 a GIF records without them rather than failing.
    let _ = config.set_shows_mouse_clicks(options.clicks);
    if options.area.is_some() {
        config.set_source_rect(source_rect(&region, &frame));
    }
    match options.kind {
        VideoKind::Mp4 => start_mp4(&filter, config, &region, options, to),
        VideoKind::Gif => start_gif(&filter, config, &region, to),
    }
}

fn start_mp4(
    filter: &SCContentFilter,
    config: SCStreamConfiguration,
    region: &Rect,
    options: &RecordOptions,
    to: &Path,
) -> Result<Recording> {
    let scale = scale(filter)?;
    // H.264 wants even sides.
    let even = |points: f64| pixels(points, scale) & !1;
    let (width, height) = (even(region.width).max(2), even(region.height).max(2));
    let config = config
        .with_width(width)
        .with_height(height)
        .with_fps(MP4_FPS)
        .with_captures_audio(options.audio)
        .with_excludes_current_process_audio(true)
        .with_captures_microphone(options.microphone)
        .map_err(|e| e.to_string())?;
    let file = SCRecordingOutputConfiguration::new()
        .map_err(|e| e.to_string())?
        .with_output_url(to)
        .map_err(|e| e.to_string())?
        .with_video_codec(SCRecordingOutputCodec::H264)
        .with_output_file_type(SCRecordingOutputFileType::MP4);
    let (tx, events) = channel();
    let (on_start, on_fail, on_finish) = (tx.clone(), tx.clone(), tx);
    let callbacks = RecordingCallbacks::new()
        .on_start(move || drop(on_start.send(Event::Started)))
        .on_fail(move |e| drop(on_fail.send(Event::Failed(e))))
        .on_finish(move || drop(on_finish.send(Event::Finished)));
    let output = SCRecordingOutput::new_with_delegate(&file, callbacks)
        .ok_or_else(|| "recording to MP4 needs macOS 15 or later".to_string())?;
    let stream = SCStream::new(filter, &config).map_err(|e| e.to_string())?;
    stream.add_recording_output(&output).map_err(|e| e.to_string())?;
    stream.start_capture().map_err(|e| e.to_string())?;
    match events.recv_timeout(PATIENCE) {
        Ok(Event::Started) => {}
        Ok(Event::Failed(e)) => return Err(format!("the recording failed to start: {e}")),
        _ => return Err("the recording never started".into()),
    }
    let sink = Sink::Mp4 { output, events, started: Instant::now() };
    Ok(Recording { stream, sink, width, height })
}

fn start_gif(filter: &SCContentFilter, config: SCStreamConfiguration, region: &Rect, to: &Path) -> Result<Recording> {
    // One pixel per point keeps a GIF small; past the cap it shrinks to fit.
    let fit = (GIF_MAX_WIDTH / region.width).min(1.0);
    let (width, height) = (pixels(region.width, fit), pixels(region.height, fit));
    let config = config.with_width(width).with_height(height).with_fps(GIF_FPS).with_pixel_format(PixelFormat::BGRA);
    let (frames, encoder) = gif::start(to, width, height)?;
    let mut stream = SCStream::new(filter, &config).map_err(|e| e.to_string())?;
    stream
        .add_output_handler(
            move |sample: CMSampleBuffer, kind: SCStreamOutputType| {
                if matches!(kind, SCStreamOutputType::Screen) {
                    if let Some(rgba) = rgba(&sample) {
                        frames.push(rgba, Instant::now());
                    }
                }
            },
            SCStreamOutputType::Screen,
        )
        .map_err(|e| e.to_string())?;
    stream.start_capture().map_err(|e| e.to_string())?;
    Ok(Recording { stream, sink: Sink::Gif(encoder), width, height })
}

/// A BGRA frame as opaque RGBA, or `None` for a frame with nothing new (ScreenCaptureKit sends those
/// while the screen is still).
fn rgba(sample: &CMSampleBuffer) -> Option<Vec<u8>> {
    if !matches!(sample.frame_status(), Some(SCFrameStatus::Complete | SCFrameStatus::Started)) {
        return None;
    }
    let buffer = sample.pixel_buffer()?;
    let pixels = buffer.lock(CVPixelBufferLockFlags::READ_ONLY).ok()?;
    let (width, height, stride) = (pixels.width(), pixels.height(), pixels.bytes_per_row());
    // SAFETY: the lock guard keeps the buffer's memory mapped while we read it.
    let data = unsafe { pixels.as_slice() }?;
    let mut out = Vec::with_capacity(width * height * 4);
    for row in data.chunks(stride).take(height) {
        for p in row.get(..width * 4)?.as_chunks::<4>().0 {
            out.extend([p[2], p[1], p[0], 255]);
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    // Needs a display and the Screen Recording grant, so it runs only when asked:
    // `cargo test -- --ignored --nocapture`. It deletes what it captured, since that shows the screen.
    use super::*;
    use crate::{area, mosaic_path, record, screen, window};
    use std::path::PathBuf;

    fn size(path: &Path) -> (u32, u32) {
        let info = png::Decoder::new(std::io::BufReader::new(std::fs::File::open(path).unwrap()))
            .read_info()
            .unwrap()
            .info()
            .clone();
        (info.width, info.height)
    }

    fn proof() {
        // A test binary has no WindowServer connection until a CoreGraphics call opens one, and reading a
        // macOS 26 screenshot's pixels without one aborts. The app always has one; `pointer` opens it here.
        pointer();
        let dir = PathBuf::from("/tmp/glint-proof");
        std::fs::create_dir_all(&dir).unwrap();
        let content = content().unwrap();
        let display = display_at(&content, pointer()).unwrap();
        let frame = rect(display.frame());
        let scale = scale(&display_filter(&content, &display).unwrap()).unwrap();
        println!("display {frame:?} at {scale}x, pointer {:?}", pointer());

        let shot = screen(&dir.join("screen.png")).unwrap();
        println!("screen: {}x{}, mosaic {:?}", shot.width, shot.height, size(&shot.mosaic));
        assert_eq!((shot.width, shot.height), (pixels(frame.width, scale), pixels(frame.height, scale)));
        assert_eq!(size(&dir.join("screen.png")), (shot.width, shot.height));
        assert_eq!(size(&shot.mosaic), (shot.width.div_ceil(16), shot.height.div_ceil(16)));

        let picked = Rect { x: frame.x + 40.0, y: frame.y + 60.0, width: 300.0, height: 200.0 };
        let part = area(picked, &dir.join("area.png")).unwrap();
        println!("area 300x200 pt: {}x{}", part.width, part.height);
        assert_eq!((part.width, part.height), (pixels(300.0, scale), pixels(200.0, scale)));
        // Past the display's edge, it clips.
        let edge = Rect { x: frame.x + frame.width - 50.0, ..picked };
        let clipped = area(edge, &dir.join("edge.png")).unwrap();
        println!("area 300x200 pt, 50 pt on screen: {}x{}", clipped.width, clipped.height);
        assert_eq!(clipped.width, pixels(50.0, scale));

        let pid = std::process::id() as i32;
        let target = content
            .windows()
            .into_iter()
            .filter(|w| w.is_on_screen() && w.window_layer() == 0 && w.frame().size.width > 200.0)
            .find(|w| w.owning_application().is_some_and(|a| a.process_id() != pid))
            .expect("an ordinary window on screen");
        let wf = rect(target.frame());
        let app = target.owning_application().map(|a| a.application_name()).unwrap_or_default();
        let one = window(target.window_id(), &dir.join("window.png")).unwrap();
        println!("window {} ({app}) {wf:?}: {}x{}", target.window_id(), one.width, one.height);
        // The shadow widens the picture past the window, and its corner is transparent.
        assert!(one.width > pixels(wf.width, scale) && one.height > pixels(wf.height, scale));
        let mut png = png::Decoder::new(std::io::BufReader::new(std::fs::File::open(dir.join("window.png")).unwrap()))
            .read_info()
            .unwrap();
        let mut pixels_ = vec![0; png.output_buffer_size().unwrap()];
        png.next_frame(&mut pixels_).unwrap();
        println!("window corner RGBA {:?}, mosaic {:?}", &pixels_[..4], size(&one.mosaic));
        assert_eq!(pixels_[3], 0);

        let options = RecordOptions {
            area: None,
            kind: VideoKind::Mp4,
            cursor: true,
            clicks: true,
            audio: true,
            microphone: false,
        };
        let mp4 = dir.join("rec.mp4");
        let live = record(&options, &mp4).unwrap();
        std::thread::sleep(Duration::from_secs(3));
        let video = live.stop().unwrap();
        let bytes = std::fs::metadata(&mp4).unwrap().len();
        println!("mp4: {}x{}, {} ms, {bytes} bytes", video.width, video.height, video.duration_ms);
        assert!((2500..3600).contains(&video.duration_ms) && bytes > 1000);

        let gif_path = dir.join("rec.gif");
        let options = RecordOptions {
            area: Some(Rect { width: 640.0, height: 400.0, ..picked }),
            kind: VideoKind::Gif,
            ..options
        };
        let live = record(&options, &gif_path).unwrap();
        std::thread::sleep(Duration::from_secs(2));
        let video = live.stop().unwrap();
        let mut decoder = ::gif::DecodeOptions::new().read_info(std::fs::File::open(&gif_path).unwrap()).unwrap();
        let (mut frames, mut cs) = (0, 0u64);
        while let Some(f) = decoder.read_next_frame().unwrap() {
            frames += 1;
            cs += u64::from(f.delay);
        }
        let bytes = std::fs::metadata(&gif_path).unwrap().len();
        println!(
            "gif: {}x{}, {} ms, {frames} frames summing {cs} cs, {bytes} bytes",
            video.width, video.height, video.duration_ms
        );
        assert_eq!((video.width, video.height), (640, 400));
        assert!((1800..2300).contains(&video.duration_ms) && cs * 10 == video.duration_ms);
        assert!(!mosaic_path(&gif_path).exists());
    }

    #[test]
    #[ignore]
    fn captures_and_records_the_screen() {
        let result = std::panic::catch_unwind(proof);
        // Whatever happened, the pictures go: they show the user's screen.
        let _ = std::fs::remove_dir_all("/tmp/glint-proof");
        if let Err(panic) = result {
            std::panic::resume_unwind(panic);
        }
    }
}
