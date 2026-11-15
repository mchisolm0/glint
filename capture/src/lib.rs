//! Glint's capture layer: screenshots and screen recordings, written straight to files.
//!
//! One API over a back end per platform: ScreenCaptureKit on macOS (`mac.rs`), and later xdg-desktop-portal
//! plus PipeWire on Linux (`linux.rs`, a stub for now). PNG writing, the mosaic and GIF encoding (`gif.rs`)
//! work on RGBA frames, so every back end shares them.
//!
//! Rects are the desktop's global points, origin top-left (CoreGraphics). Pixels are points times the
//! display's scale. Glint's own windows never appear in a capture.

use std::path::{Path, PathBuf};

// Linux's stub has no frames to encode yet.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
mod gif;

#[cfg(target_os = "macos")]
#[path = "mac.rs"]
mod platform;
#[cfg(not(target_os = "macos"))]
#[path = "linux.rs"]
mod platform;

pub type Result<T> = std::result::Result<T, String>;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

// The Linux stub has no displays to clip to yet.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
impl Rect {
    fn centre(&self) -> (f64, f64) {
        (self.x + self.width / 2.0, self.y + self.height / 2.0)
    }

    fn contains(&self, (x, y): (f64, f64)) -> bool {
        x >= self.x && y >= self.y && x < self.x + self.width && y < self.y + self.height
    }

    fn intersect(&self, other: &Rect) -> Option<Rect> {
        let (x, y) = (self.x.max(other.x), self.y.max(other.y));
        let width = (self.x + self.width).min(other.x + other.width) - x;
        let height = (self.y + self.height).min(other.y + other.height) - y;
        (width >= 1.0 && height >= 1.0).then_some(Rect { x, y, width, height })
    }
}

/// A PNG written by `screen`, `area` or `window`, in pixels.
#[derive(Debug)]
pub struct Shot {
    pub width: u32,
    pub height: u32,
    /// `<stem>.mosaic.png` beside it: the picture at 1/16 of each side, for the editor's pixelate tool.
    pub mosaic: PathBuf,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VideoKind {
    Mp4,
    Gif,
}

impl VideoKind {
    pub fn parse(name: &str) -> Result<Self> {
        match name {
            "mp4" => Ok(Self::Mp4),
            "gif" => Ok(Self::Gif),
            other => Err(format!("a recording is \"mp4\" or \"gif\", not {other:?}")),
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Mp4 => "mp4",
            Self::Gif => "gif",
        }
    }
}

#[derive(Clone, Debug)]
pub struct RecordOptions {
    /// `None` records the whole display under the pointer.
    pub area: Option<Rect>,
    pub kind: VideoKind,
    pub cursor: bool,
    /// Draws a ring at each click (macOS 15 and later).
    pub clicks: bool,
    /// System audio. MP4 only; a GIF has no sound.
    pub audio: bool,
    pub microphone: bool,
}

/// A finished recording, in pixels and milliseconds.
#[derive(Debug)]
pub struct Video {
    pub width: u32,
    pub height: u32,
    pub duration_ms: u64,
}

/// A recording in progress. Dropping it without `stop` abandons the file.
pub struct Recording {
    inner: platform::Recording,
    kind: VideoKind,
}

impl Recording {
    pub fn kind(&self) -> VideoKind {
        self.kind
    }

    /// Stops capturing and waits until the file is complete.
    pub fn stop(self) -> Result<Video> {
        self.inner.stop()
    }
}

/// The display under the pointer, as a PNG at `to`.
pub fn screen(to: &Path) -> Result<Shot> {
    platform::screen(to)
}

/// `rect`, clipped to the display holding its centre, as a PNG at `to` at the display's full resolution.
pub fn area(rect: Rect, to: &Path) -> Result<Shot> {
    platform::area(rect, to)
}

/// One window (a CGWindowID on macOS) alone with its shadow, on transparency, as a PNG at `to`.
pub fn window(id: u32, to: &Path) -> Result<Shot> {
    platform::window(id, to)
}

/// Starts recording into `to` (an `.mp4` or `.gif` path) and returns once frames are flowing.
pub fn record(options: &RecordOptions, to: &Path) -> Result<Recording> {
    Ok(Recording { inner: platform::record(options, to)?, kind: options.kind })
}

/// `<stem>.mosaic.png` beside `png`.
pub fn mosaic_path(png: &Path) -> PathBuf {
    let stem = png.file_stem().unwrap_or_default().to_string_lossy();
    png.with_file_name(format!("{stem}.mosaic.png"))
}

/// Each side of the mosaic is the picture's divided by this, rounded up.
const MOSAIC_BLOCK: u32 = 16;

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
/// Writes the mosaic of a straight-alpha RGBA picture beside `png` and returns its path.
pub(crate) fn write_mosaic(png: &Path, rgba: &[u8], width: u32, height: u32) -> Result<PathBuf> {
    let (small, w, h) = mosaic(rgba, width, height);
    let path = mosaic_path(png);
    write_png(&path, &small, w, h)?;
    Ok(path)
}

/// Box-averages each 16 × 16 block of a straight-alpha RGBA picture into one pixel. Colour is weighted by
/// alpha, so the transparent margin of a window shot does not darken its edge.
fn mosaic(rgba: &[u8], width: u32, height: u32) -> (Vec<u8>, u32, u32) {
    let (w, h) = (width.div_ceil(MOSAIC_BLOCK), height.div_ceil(MOSAIC_BLOCK));
    let mut out = Vec::with_capacity((w * h * 4) as usize);
    for by in 0..h {
        for bx in 0..w {
            let mut sum = [0u64; 4];
            let mut count = 0u64;
            for y in by * MOSAIC_BLOCK..((by + 1) * MOSAIC_BLOCK).min(height) {
                let row = (y * width) as usize * 4;
                for x in bx * MOSAIC_BLOCK..((bx + 1) * MOSAIC_BLOCK).min(width) {
                    let p = &rgba[row + x as usize * 4..][..4];
                    let a = u64::from(p[3]);
                    for c in 0..3 {
                        sum[c] += u64::from(p[c]) * a;
                    }
                    sum[3] += a;
                    count += 1;
                }
            }
            let colour = |c: usize| sum[c].checked_div(sum[3]).unwrap_or(0) as u8;
            out.extend([colour(0), colour(1), colour(2), (sum[3] / count) as u8]);
        }
    }
    (out, w, h)
}

/// Writes a straight-alpha RGBA picture as a PNG.
fn write_png(path: &Path, rgba: &[u8], width: u32, height: u32) -> Result<()> {
    let file = std::fs::File::create(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().map_err(|e| e.to_string())?;
    writer.write_image_data(rgba).map_err(|e| e.to_string())?;
    writer.finish().map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_mosaic_averages_blocks_and_rounds_up() {
        // 20 × 16: one full block of red, then a 4-wide block that is half blue, half transparent.
        let mut rgba = Vec::new();
        for _ in 0..16 {
            for x in 0..20 {
                rgba.extend(match x {
                    0..16 => [255, 0, 0, 255],
                    16..18 => [0, 0, 255, 255],
                    _ => [0, 255, 0, 0],
                });
            }
        }
        let (small, w, h) = mosaic(&rgba, 20, 16);
        assert_eq!((w, h), (2, 1));
        assert_eq!(&small[..4], &[255, 0, 0, 255]);
        // The transparent green adds no colour, only lowers alpha.
        assert_eq!(&small[4..], &[0, 0, 255, 127]);
    }

    #[test]
    fn an_area_clips_to_its_display() {
        let display = Rect { x: 0.0, y: 0.0, width: 100.0, height: 50.0 };
        let area = Rect { x: 90.0, y: -10.0, width: 30.0, height: 30.0 };
        assert_eq!(display.intersect(&area), Some(Rect { x: 90.0, y: 0.0, width: 10.0, height: 20.0 }));
        assert!(!display.contains(area.centre()));
        assert_eq!(display.intersect(&Rect { x: 200.0, ..area }), None);
    }

    #[test]
    fn a_mosaic_sits_beside_its_png() {
        assert_eq!(mosaic_path(Path::new("/tmp/glint/17.png")), Path::new("/tmp/glint/17.mosaic.png"));
    }
}
