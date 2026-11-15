//! Scrolling capture: frames of one area while the person scrolls it, stitched into one tall PNG.
//!
//! Each frame is matched against the last one row by row. Rows that stay put at the top and bottom (a toolbar,
//! a sticky header, a status bar) are the frame's fixed edges; the rows between them are the band that
//! scrolls. The best vertical offset between two bands extends the picture down (or up, scrolling back past
//! the start). Rows are compared by a hash of their pixels, leaving out the right edge, where an overlay
//! scroll bar slides over the content.

use crate::frames::{self, Encoder, Frames, Message};
use crate::{write_mosaic, write_png, Result, Video};
use std::collections::VecDeque;
use std::path::Path;

/// Starts stitching `width` × `height` frames into the PNG at `path` (and its mosaic) on a worker thread.
pub(crate) fn start(path: &Path, width: u32, height: u32) -> Result<(Frames, Encoder)> {
    let path = path.to_owned();
    frames::spawn("glint-scroll", width, height, move |rx| {
        let mut stitch = Stitch::new(width as usize, height as usize);
        loop {
            match rx.recv() {
                Ok(Message::Frame(rgba, _)) => stitch.add(rgba),
                Ok(Message::End(_)) => break,
                Err(_) => return Err("the scrolling capture ended without finishing".into()),
            }
        }
        let (rgba, rows) = stitch.finish().ok_or("the scrolling capture saw no frames")?;
        write_png(&path, &rgba, width, rows as u32)?;
        write_mosaic(&path, &rgba, width, rows as u32)?;
        Ok(Video { width, height: rows as u32, duration_ms: 0 })
    })
}

/// The picture stops growing past this many pixels (about 200 MB of RGBA).
const MAX_PIXELS: usize = 50_000_000;
/// Of the rows two bands share at an offset, at least this share of the patterned ones must match.
const AGREEMENT: f64 = 0.9;
/// Fewer patterned rows than this in an overlap prove nothing.
const EVIDENCE: usize = 8;
/// Bands overlap by at least this share of their height; a faster scroll between two frames is lost.
const OVERLAP: f64 = 0.15;

/// One frame's rows: the RGBA, each row's hash, and whether each row is one flat colour (which matches
/// anything, so it counts for nothing).
struct Frame {
    rgba: Vec<u8>,
    hashes: Vec<u64>,
    plain: Vec<bool>,
}

impl Frame {
    fn new(rgba: Vec<u8>, width: usize, height: usize) -> Self {
        // An overlay scroll bar sits on the right ~16 points; leave out the right 4% (at least 32 pixels).
        let used = width.saturating_sub((width / 25).max(32)).max(1.min(width));
        let (mut hashes, mut plain) = (Vec::with_capacity(height), Vec::with_capacity(height));
        for row in rgba.chunks_exact(width * 4).take(height) {
            let pixels = &row[..used * 4];
            hashes.push(fnv(pixels));
            plain.push(pixels.chunks_exact(4).all(|p| p == &pixels[..4]));
        }
        Frame { rgba, hashes, plain }
    }

    fn row(&self, i: usize, width: usize) -> &[u8] {
        &self.rgba[i * width * 4..(i + 1) * width * 4]
    }
}

fn fnv(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |h, &b| (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3))
}

/// The fixed rows at the top and bottom of two frames: those equal in both.
fn edges(a: &Frame, b: &Frame) -> (usize, usize) {
    let height = a.hashes.len();
    let top = (0..height).take_while(|&i| a.hashes[i] == b.hashes[i]).count();
    let bottom = (top..height).rev().take_while(|&i| a.hashes[i] == b.hashes[i]).count();
    (top, bottom)
}

/// How far the band `[top, top + rows)` moved from `prev` to `cur`: `Some(d)` when `cur`'s row `i` shows
/// `prev`'s row `i + d` (positive: scrolled down). `Some(0)` is no movement; `None` is no confident match.
fn offset(prev: &Frame, cur: &Frame, top: usize, rows: usize) -> Option<isize> {
    let reach = rows as isize - (rows as f64 * OVERLAP).ceil() as isize;
    let score = |d: isize| {
        let (mut patterned, mut matched) = (0usize, 0usize);
        for i in 0.max(-d)..(rows as isize).min(rows as isize - d) {
            let (c, p) = (top + i as usize, top + (i + d) as usize);
            if cur.plain[c] {
                continue;
            }
            patterned += 1;
            matched += usize::from(cur.hashes[c] == prev.hashes[p]);
        }
        (patterned >= EVIDENCE).then(|| (matched as f64 / patterned as f64, patterned))
    };
    if score(0).is_none_or(|(share, _)| share >= AGREEMENT) {
        return Some(0);
    }
    // The best agreement, then the most evidence, then the smallest move.
    let mut best: Option<(f64, usize, isize)> = None;
    for d in (-reach..=reach).filter(|&d| d != 0) {
        let Some((share, patterned)) = score(d) else { continue };
        let better = match best {
            None => true,
            Some((s, p, b)) => share > s || (share == s && (patterned > p || (patterned == p && d.abs() < b.abs()))),
        };
        if share >= AGREEMENT && better {
            best = Some((share, patterned, d));
        }
    }
    best.map(|(_, _, d)| d)
}

/// The picture so far: the fixed top and bottom rows (from the frames that reached furthest up and down)
/// around the band's rows, top to bottom.
struct Stitch {
    width: usize,
    height: usize,
    /// The latest frame that matched (or the first): the next one is compared with it.
    last: Option<Frame>,
    /// The fixed rows at the top and bottom, once a first scroll has shown them.
    edges: Option<(usize, usize)>,
    band: VecDeque<Vec<u8>>,
    /// Where `last`'s band starts in `band`.
    at: usize,
    head: Vec<u8>,
    foot: Vec<u8>,
}

impl Stitch {
    fn new(width: usize, height: usize) -> Self {
        Stitch { width, height, last: None, edges: None, band: VecDeque::new(), at: 0, head: vec![], foot: vec![] }
    }

    fn add(&mut self, rgba: Vec<u8>) {
        let cur = Frame::new(rgba, self.width, self.height);
        let Some(last) = &self.last else {
            self.last = Some(cur);
            return;
        };
        // Until a first scroll, the fixed edges are a guess from this pair.
        let (top, bottom) = self.edges.unwrap_or_else(|| edges(last, &cur));
        let rows = self.height.saturating_sub(top + bottom);
        if rows < EVIDENCE {
            return;
        }
        match offset(last, &cur, top, rows) {
            None => {}
            Some(0) => self.last = Some(cur),
            Some(d) => {
                if self.edges.is_none() {
                    self.edges = Some((top, bottom));
                    let first = self.last.take().expect("checked");
                    self.band = (top..top + rows).map(|i| first.row(i, self.width).to_vec()).collect();
                    self.head = first.rgba[..top * self.width * 4].to_vec();
                    self.foot = first.rgba[(top + rows) * self.width * 4..].to_vec();
                }
                self.place(&cur, d, top, rows);
                self.last = Some(cur);
            }
        }
    }

    /// Adds the rows of `cur`'s band that the picture lacks, `cur` having moved `d` rows from the last frame.
    fn place(&mut self, cur: &Frame, d: isize, top: usize, rows: usize) {
        let room = MAX_PIXELS / self.width;
        let mut at = self.at as isize + d;
        if at < 0 {
            // Scrolled above the start: the new rows go on top, and these fixed top rows with them.
            let new = (-at) as usize;
            if self.band.len() + new + self.height > room {
                return;
            }
            for i in (0..new).rev() {
                self.band.push_front(cur.row(top + i, self.width).to_vec());
            }
            self.head = cur.rgba[..top * self.width * 4].to_vec();
            at = 0;
        }
        let end = at as usize + rows;
        if end > self.band.len() {
            let new = end - self.band.len();
            if self.band.len() + new + self.height > room {
                return;
            }
            for i in rows - new..rows {
                self.band.push_back(cur.row(top + i, self.width).to_vec());
            }
            self.foot = cur.rgba[(top + rows) * self.width * 4..].to_vec();
        }
        self.at = at as usize;
    }

    /// The stitched RGBA and its height in rows. With no scroll, the one frame as it was.
    fn finish(self) -> Option<(Vec<u8>, usize)> {
        if self.edges.is_none() {
            return self.last.map(|f| (f.rgba, self.height));
        }
        let mut out = self.head;
        for row in &self.band {
            out.extend_from_slice(row);
        }
        out.extend_from_slice(&self.foot);
        let rows = out.len() / (self.width * 4);
        Some((out, rows))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const W: usize = 64;

    /// A tall page of scattered dark pixels, no two rows alike.
    fn page(rows: usize) -> Vec<Vec<u8>> {
        // splitmix64 of the pixel's index: a dark pixel in 16.
        let ink = |x: usize, y: usize| {
            let mut z = ((y * W + x) as u64).wrapping_add(0x9e37_79b9_7f4a_7c15);
            z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
            (z ^ (z >> 31)) >> 60 == 0
        };
        (0..rows)
            .map(|y| (0..W).flat_map(|x| if ink(x, y) { [20, 20, 20, 255] } else { [250, 250, 250, 255] }).collect())
            .collect()
    }

    fn flat(colour: u8) -> Vec<u8> {
        [colour, colour, colour, 255].repeat(W)
    }

    /// A window of `height` rows: `top` rows of toolbar, the page from row `scroll`, `bottom` rows of status bar.
    fn view(page: &[Vec<u8>], scroll: usize, height: usize, top: usize, bottom: usize) -> Vec<u8> {
        let mut out = Vec::new();
        (0..top).for_each(|i| out.extend(if i == top - 1 { flat(0) } else { flat(90) }));
        (0..height - top - bottom).for_each(|i| out.extend(&page[scroll + i]));
        (0..bottom).for_each(|i| out.extend(if i == 0 { flat(0) } else { flat(60) }));
        out
    }

    #[test]
    fn scrolling_down_and_back_stitches_the_page_once_between_its_fixed_edges() {
        let page = page(400);
        let mut stitch = Stitch::new(W, 100);
        // Still frames, steps of different sizes, a step back, and a jump back too far to match (dropped).
        for scroll in [0, 0, 7, 30, 31, 70, 140, 125, 160, 220, 100] {
            stitch.add(view(&page, scroll, 100, 10, 6));
        }
        let (rgba, rows) = stitch.finish().unwrap();
        // The band is 84 rows, so the page reaches row 220 + 84.
        assert_eq!(rows, 10 + 220 + 84 + 6);
        let mut want = view(&page, 0, 100, 10, 6)[..10 * W * 4].to_vec();
        page[..220 + 84].iter().for_each(|r| want.extend(r));
        want.extend(&view(&page, 0, 100, 10, 6)[94 * W * 4..]);
        assert!(rgba == want, "the stitched page differs from the original");
    }

    #[test]
    fn a_page_that_never_scrolls_is_its_one_frame() {
        let page = page(200);
        let mut stitch = Stitch::new(W, 100);
        stitch.add(view(&page, 0, 100, 10, 6));
        stitch.add(view(&page, 0, 100, 10, 6));
        let (rgba, rows) = stitch.finish().unwrap();
        assert_eq!((rows, rgba), (100, view(&page, 0, 100, 10, 6)));
    }

    #[test]
    fn scrolling_above_the_start_adds_rows_on_top() {
        let page = page(300);
        let mut stitch = Stitch::new(W, 100);
        for scroll in [100, 120, 60, 20] {
            stitch.add(view(&page, scroll, 100, 10, 6));
        }
        let (rgba, rows) = stitch.finish().unwrap();
        assert_eq!(rows, 10 + (120 + 84 - 20) + 6);
        assert_eq!(&rgba[10 * W * 4..11 * W * 4], &page[20][..]);
    }
}
