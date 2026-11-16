//! Scrolling capture: frames of one area while the person scrolls it, stitched into one tall PNG.
//!
//! Each frame is matched against the last one row by row. Rows that stay put at the top and bottom (a toolbar,
//! a sticky header, a status bar) are the frame's fixed edges; the rows between them are the band that
//! scrolls. The best vertical offset between two bands extends the picture down (or up, scrolling back past
//! the start). Rows are compared by a hash of their pixels, leaving out the right edge, where an overlay
//! scroll bar slides over the content. A window is cut to the columns its first scroll changed, so a sidebar
//! beside them is left out, as the Mac's picked area leaves it out.

use crate::frames::{self, Encoder, Frames, Message};
use crate::{write_mosaic, write_png, Result, Video};
use std::collections::VecDeque;
use std::path::Path;

/// Starts stitching `width` × `height` frames into the PNG at `path` (and its mosaic) on a worker thread.
pub(crate) fn start(path: &Path, width: u32, height: u32) -> Result<(Frames, Encoder)> {
    let path = path.to_owned();
    frames::spawn("glint-scroll", width, height, move |rx| {
        let mut capture = Capture::new(width as usize, height as usize);
        loop {
            match rx.recv() {
                Ok(Message::Frame(rgba, _)) => capture.add(rgba),
                Ok(Message::End(_)) => break,
                Err(_) => return Err("the scrolling capture ended without finishing".into()),
            }
        }
        let (rgba, width, rows) = capture.finish().ok_or("the scrolling capture saw no frames")?;
        eprintln!("glint: stitched {width}x{rows}");
        let (width, rows) = (width as u32, rows as u32);
        write_png(&path, &rgba, width, rows)?;
        write_mosaic(&path, &rgba, width, rows)?;
        Ok(Video { width, height: rows, duration_ms: 0 })
    })
}

/// A scrolling capture's frames: cut to the window, then, from the first scroll on, to the columns that
/// scroll. A sidebar or panel beside them stays put, and rows that span it would never match.
struct Capture {
    width: usize,
    height: usize,
    part: Option<Opaque>,
    /// The last frame before the first scroll, cut to `part`.
    before: Option<Vec<u8>>,
    /// The scrolling columns, `[left, right)` of `part`, and their stitch.
    stitch: Option<((usize, usize), Stitch)>,
}

impl Capture {
    fn new(width: usize, height: usize) -> Self {
        Capture { width, height, part: None, before: None, stitch: None }
    }

    fn add(&mut self, rgba: Vec<u8>) {
        let part = *self.part.get_or_insert_with(|| Opaque::of(&rgba, self.width, self.height));
        let frame = part.cut(rgba, self.width);
        if let Some(((left, right), stitch)) = &mut self.stitch {
            stitch.add(columns(&frame, part.width, *left, *right));
            return;
        }
        if let Some(before) = &self.before {
            if let Some((left, right)) = moving(before, &frame, part.width, part.height) {
                // A change that is no scroll (a redraw as the window gains focus) is not the start.
                let mut stitch = Stitch::new(right - left, part.height);
                stitch.add(columns(before, part.width, left, right));
                stitch.add(columns(&frame, part.width, left, right));
                if stitch.edges.is_some() {
                    eprintln!("glint: scrolling columns {left}..{right} of the {}x{} at {},{}", part.width, part.height, part.x, part.y);
                    self.stitch = Some(((left, right), stitch));
                    self.before = None;
                    return;
                }
            }
        }
        self.before = Some(frame);
    }

    /// The stitched RGBA, its width and its height. With no scroll, the last frame of the window.
    fn finish(self) -> Option<(Vec<u8>, usize, usize)> {
        let part = self.part?;
        match self.stitch {
            Some(((left, right), stitch)) => stitch.finish().map(|(rgba, rows)| (rgba, right - left, rows)),
            None => self.before.map(|f| (f, part.width, part.height)),
        }
    }
}

/// `[left, right)` of each row of a `width`-wide frame.
fn columns(rgba: &[u8], width: usize, left: usize, right: usize) -> Vec<u8> {
    if left == 0 && right == width {
        return rgba.to_vec();
    }
    rgba.chunks_exact(width * 4).flat_map(|row| &row[left * 4..right * 4]).copied().collect()
}

/// The columns that changed between two frames as a scroll changes them, `[left, right)`: the core is every
/// column with at least a tenth of the busiest one's changed rows (a spinner or clock elsewhere changes far
/// fewer), widened by the changed columns beside it up to an unchanged one. `None` when the busiest column
/// changed in under a tenth of the rows.
fn moving(a: &[u8], b: &[u8], width: usize, height: usize) -> Option<(usize, usize)> {
    let mut changed = vec![0usize; width];
    for (ra, rb) in a.chunks_exact(width * 4).zip(b.chunks_exact(width * 4)).take(height) {
        for (x, (pa, pb)) in ra.chunks_exact(4).zip(rb.chunks_exact(4)).enumerate() {
            changed[x] += usize::from(pa != pb);
        }
    }
    let most = *changed.iter().max()?;
    if most * 10 < height {
        return None;
    }
    let mut left = changed.iter().position(|&c| c * 10 >= most)?;
    let mut right = changed.iter().rposition(|&c| c * 10 >= most)? + 1;
    while left > 0 && changed[left - 1] > 0 {
        left -= 1;
    }
    while right < width && changed[right] > 0 {
        right += 1;
    }
    Some((left, right))
}

/// The opaque rectangle of the first frame, which every frame is cut to. GNOME casts a window as a
/// display-sized frame with the window at the top left, its shadow half transparent and the rest clear; cut
/// to the window, the overlay scroll bar is back at the right edge the row hashes leave out.
#[derive(Clone, Copy)]
struct Opaque {
    x: usize,
    y: usize,
    width: usize,
    height: usize,
}

impl Opaque {
    fn of(rgba: &[u8], width: usize, height: usize) -> Self {
        let solid = |x: usize, y: usize| rgba[(y * width + x) * 4 + 3] == 255;
        let rows: Vec<usize> = (0..height).filter(|&y| (0..width).any(|x| solid(x, y))).collect();
        let cols: Vec<usize> = (0..width).filter(|&x| rows.iter().any(|&y| solid(x, y))).collect();
        match (rows.first(), rows.last(), cols.first(), cols.last()) {
            (Some(&top), Some(&bottom), Some(&left), Some(&right)) => {
                Opaque { x: left, y: top, width: right - left + 1, height: bottom - top + 1 }
            }
            _ => Opaque { x: 0, y: 0, width, height },
        }
    }

    /// `rgba` (a frame `width` wide) cut to this rectangle.
    fn cut(&self, rgba: Vec<u8>, width: usize) -> Vec<u8> {
        if self.x == 0 && self.y == 0 && self.width == width {
            let mut rgba = rgba;
            rgba.truncate(self.height * width * 4);
            return rgba;
        }
        let mut out = Vec::with_capacity(self.width * self.height * 4);
        for y in self.y..self.y + self.height {
            let start = (y * width + self.x) * 4;
            out.extend_from_slice(&rgba[start..start + self.width * 4]);
        }
        out
    }
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
            // At a move, a row that stayed where it was is no evidence: a fixed composer or panel inside
            // the band, before the first scroll has found where the band ends.
            if cur.plain[c] || (d != 0 && cur.hashes[c] == prev.hashes[c]) {
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

/// The first scroll, before the band's edges are known: the move most of the changed rows in `[top, top +
/// rows)` agree on, at least twice as many as on any other, and the band it shows (its top and rows). Parts
/// that change without moving (an edge fading the content out, a translucent composer the content shows
/// through, a pill floating over it) agree with no move, so the band is the run between them with the most
/// rows that moved, widened by the rows the move brought in.
fn first_move(prev: &Frame, cur: &Frame, top: usize, rows: usize) -> Option<(isize, usize, usize)> {
    let span = top as isize..(top + rows) as isize;
    let changed: Vec<usize> = (top..top + rows).filter(|&c| !cur.plain[c] && cur.hashes[c] != prev.hashes[c]).collect();
    if changed.len() < EVIDENCE {
        return None;
    }
    let shows = |c: usize, d: isize| {
        let p = c as isize + d;
        span.contains(&p).then(|| cur.hashes[c] == prev.hashes[p as usize])
    };
    let reach = rows as isize - (rows as f64 * OVERLAP).ceil() as isize;
    let scores: Vec<(isize, usize)> =
        (-reach..=reach).filter(|&d| d != 0).map(|d| (d, changed.iter().filter(|&&c| shows(c, d) == Some(true)).count())).collect();
    let &(d, best) = scores.iter().max_by_key(|(d, n)| (*n, std::cmp::Reverse(d.abs())))?;
    let runner = scores.iter().filter(|(e, _)| (e - d).abs() > 2).map(|(_, n)| *n).max().unwrap_or(0);
    if best < EVIDENCE || best < 2 * runner || best * 4 < changed.len() {
        return None;
    }
    // Runs between the changed rows that disagree with the move; the one with the most rows that moved.
    let (mut run, mut most): ((Option<usize>, usize, usize), (Option<usize>, usize, usize)) = ((None, 0, 0), (None, 0, 0));
    for c in top..top + rows {
        let moved = match shows(c, d) {
            _ if cur.plain[c] || cur.hashes[c] == prev.hashes[c] => continue,
            Some(true) => true,
            Some(false) => false,
            None => continue,
        };
        if moved {
            run = (run.0.or(Some(c)), c, run.2 + 1);
            if run.2 > most.2 {
                most = run;
            }
        } else {
            run = (None, 0, 0);
        }
    }
    let (Some(from), to, _) = most else { return None };
    // The rows the move brought in are new, not moved: they edge the band on that side.
    let (from, to) = ((from as isize + d.min(0)) as usize, (to as isize + d.max(0)) as usize);
    (to + 1 - from >= EVIDENCE).then_some((d, from, to + 1 - from))
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
        if self.edges.is_none() {
            // Before the first scroll nothing is placed yet, so a frame that did not scroll (or changed some
            // other way, as a window does gaining focus when the desktop's picker closes) is the new start.
            let Some((d, top, rows)) = first_move(last, &cur, top, rows) else {
                self.last = Some(cur);
                return;
            };
            self.edges = Some((top, self.height - top - rows));
            let first = self.last.take().expect("checked");
            self.band = (top..top + rows).map(|i| first.row(i, self.width).to_vec()).collect();
            self.head = first.rgba[..top * self.width * 4].to_vec();
            self.foot = first.rgba[(top + rows) * self.width * 4..].to_vec();
            self.place(&cur, d, top, rows);
            self.last = Some(cur);
            return;
        }
        match offset(last, &cur, top, rows) {
            None => {}
            Some(0) => self.last = Some(cur),
            Some(d) => {
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
    fn a_window_on_a_clear_frame_is_cut_to_the_window() {
        // A 3 × 2 opaque window at (1, 1) of a 5 × 4 frame, with a half-clear shadow pixel beside it.
        let mut rgba = vec![0u8; 5 * 4 * 4];
        for (x, y) in [(1, 1), (2, 1), (3, 1), (1, 2), (2, 2), (3, 2)] {
            rgba[(y * 5 + x) * 4..][..4].copy_from_slice(&[x as u8, y as u8, 9, 255]);
        }
        rgba[(3 * 5 + 4) * 4 + 3] = 80;
        let part = Opaque::of(&rgba, 5, 4);
        assert_eq!((part.x, part.y, part.width, part.height), (1, 1, 3, 2));
        assert_eq!(part.cut(rgba, 5).chunks(4).map(|p| (p[0], p[1])).collect::<Vec<_>>(), [(1, 1), (2, 1), (3, 1), (1, 2), (2, 2), (3, 2)]);
    }

    #[test]
    fn a_first_frame_whose_toolbar_then_changes_is_not_the_start() {
        let page = page(400);
        // A ten-row patterned toolbar, drawn differently in the first frame (an unfocused window).
        let frame = |scroll: usize, toolbar: usize| {
            let mut out: Vec<u8> = page[toolbar..toolbar + 10].concat();
            out.extend(view(&page, scroll, 90, 0, 6));
            out
        };
        let mut stitch = Stitch::new(W, 100);
        for (scroll, toolbar) in [(0, 310), (0, 300), (30, 300), (70, 300)] {
            stitch.add(frame(scroll, toolbar));
        }
        let (rgba, rows) = stitch.finish().unwrap();
        assert_eq!(rows, 10 + 70 + 84 + 6);
        assert_eq!(&rgba[..W * 4], &page[300][..]);
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

    #[test]
    fn a_sidebar_beside_the_scrolling_part_is_left_out() {
        const SIDE: usize = 24;
        let (page, side) = (page(400), page(500));
        // A patterned sidebar that stays put, with a spinner in two of its rows, left of the scrolling view.
        let frame = |scroll: usize, spin: u8| -> Vec<u8> {
            let view = view(&page, scroll, 100, 10, 6);
            (0..100)
                .flat_map(|y| {
                    let mut row = side[300 + y][..SIDE * 4].to_vec();
                    if y == 50 || y == 51 {
                        row[8..12].copy_from_slice(&[spin, 0, 0, 255]);
                    }
                    row.extend(&view[y * W * 4..(y + 1) * W * 4]);
                    row
                })
                .collect()
        };
        let mut capture = Capture::new(SIDE + W, 100);
        for (scroll, spin) in [(0, 1), (0, 2), (30, 3), (70, 4)] {
            capture.add(frame(scroll, spin));
        }
        let (rgba, width, rows) = capture.finish().unwrap();
        assert_eq!((width, rows), (W, 10 + 70 + 84 + 6));
        assert_eq!(&rgba[10 * W * 4..11 * W * 4], &page[0][..]);
        assert_eq!(&rgba[(10 + 153) * W * 4..(10 + 154) * W * 4], &page[153][..]);
    }

    #[test]
    fn a_fixed_composer_with_a_blinking_caret_is_a_fixed_edge() {
        let (page, other) = (page(400), page(500));
        // Ten rows of toolbar, the page, then a patterned 20-row composer whose caret rows blink.
        let frame = |scroll: usize, blink: bool| -> Vec<u8> {
            let mut out = view(&page, scroll, 80, 10, 0);
            for y in 0..20 {
                let mut row = other[300 + y].clone();
                if blink && (6..10).contains(&y) {
                    row[40..44].copy_from_slice(&[255, 0, 0, 255]);
                }
                out.extend(row);
            }
            out
        };
        let mut stitch = Stitch::new(W, 100);
        for (scroll, blink) in [(0, false), (0, true), (20, false), (45, true), (70, false)] {
            stitch.add(frame(scroll, blink));
        }
        let (rgba, rows) = stitch.finish().unwrap();
        // The band is the 70 page rows between the toolbar and the composer.
        assert_eq!(rows, 10 + 70 + 70 + 20);
        assert_eq!(&rgba[10 * W * 4..11 * W * 4], &page[0][..]);
        assert_eq!(&rgba[(10 + 139) * W * 4..(10 + 140) * W * 4], &page[139][..]);
        assert_eq!(&rgba[(10 + 140) * W * 4..(10 + 141) * W * 4], &other[300][..]);
    }

    #[test]
    fn fades_a_translucent_composer_and_a_floating_pill_are_left_out() {
        let (page, other) = (page(400), page(500));
        let dim = |row: &[u8]| -> Vec<u8> { row.chunks(4).flat_map(|p| [p[0] / 2, p[1] / 2, p[2] / 2, 255]).collect() };
        // A toolbar, a faded edge, the page with a pill floating over its rows 70..74, another faded edge,
        // then a composer the page shows through.
        let frame = |scroll: usize| -> Vec<u8> {
            let mut out = flat(90).repeat(4);
            (4..8).for_each(|y| out.extend(dim(&page[scroll + y])));
            for y in 8..76 {
                let mut row = page[scroll + y].clone();
                if (70..74).contains(&y) {
                    row[80..160].copy_from_slice(&other[y][80..160]);
                }
                out.extend(row);
            }
            (76..80).for_each(|y| out.extend(dim(&page[scroll + y])));
            for y in 80..100 {
                let mut row = other[300 + y].clone();
                row[..4].copy_from_slice(&[scroll as u8, 0, 0, 255]);
                out.extend(row);
            }
            out
        };
        let mut stitch = Stitch::new(W, 100);
        for scroll in [0, 0, 20, 45, 70] {
            stitch.add(frame(scroll));
        }
        let (rgba, rows) = stitch.finish().unwrap();
        // The band is rows 8..70, above the pill: the page down to row 70 + 62.
        assert_eq!(rows, 8 + 132 + 30);
        for k in [0, 61, 62, 131] {
            assert_eq!(&rgba[(8 + k) * W * 4..(9 + k) * W * 4], &page[8 + k][..], "page row {k}");
        }
    }
}
