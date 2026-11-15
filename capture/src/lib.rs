//! Screen capture with no UI: a whole display or an area of one, encoded as PNG.
//! Coordinates are the desktop's global logical points, origin top-left, as xcap reports them.

use std::io::Cursor;
use xcap::Monitor;

pub struct Shot {
    pub png: Vec<u8>,
    /// Pixel size, which is points times the display's scale.
    pub width: u32,
    pub height: u32,
}

pub type Result<T> = std::result::Result<T, String>;

/// The display under the point, or the primary one when no display holds it.
fn monitor_at(x: i32, y: i32) -> Result<Monitor> {
    Monitor::from_point(x, y)
        .or_else(|_| {
            Monitor::all()?
                .into_iter()
                .find(|m| m.is_primary().unwrap_or(false))
                .ok_or_else(|| xcap::XCapError::new("no display"))
        })
        .map_err(|e| e.to_string())
}

fn encode(image: xcap::image::RgbaImage) -> Result<Shot> {
    let (width, height) = image.dimensions();
    let mut png = Vec::new();
    image::DynamicImage::ImageRgba8(image)
        .write_to(&mut Cursor::new(&mut png), image::ImageFormat::Png)
        .map_err(|e| e.to_string())?;
    Ok(Shot { png, width, height })
}

/// The whole display under the point.
pub fn screen(x: i32, y: i32) -> Result<Shot> {
    encode(monitor_at(x, y)?.capture_image().map_err(|e| e.to_string())?)
}

/// An area, clipped to the display that holds its top-left corner.
pub fn area(x: i32, y: i32, width: u32, height: u32) -> Result<Shot> {
    let m = monitor_at(x, y)?;
    let (mx, my) = (m.x().map_err(|e| e.to_string())?, m.y().map_err(|e| e.to_string())?);
    let (mw, mh) = (m.width().map_err(|e| e.to_string())?, m.height().map_err(|e| e.to_string())?);
    let (rx, ry) = ((x - mx).max(0) as u32, (y - my).max(0) as u32);
    let (rw, rh) = (width.min(mw.saturating_sub(rx)), height.min(mh.saturating_sub(ry)));
    if rw == 0 || rh == 0 {
        return Err("the area is empty".into());
    }
    encode(m.capture_region(rx, ry, rw, rh).map_err(|e| e.to_string())?)
}

#[cfg(test)]
mod tests {
    // Needs a display and the Screen Recording grant, so it runs only when asked: `cargo test -- --ignored`.
    #[test]
    #[ignore]
    fn an_area_is_its_size_times_the_scale() {
        let full = super::screen(0, 0).unwrap();
        let part = super::area(10, 10, 100, 50).unwrap();
        let scale = part.width / 100;
        assert!(scale >= 1 && full.width >= part.width);
        assert_eq!((part.width, part.height), (100 * scale, 50 * scale));
    }
}
