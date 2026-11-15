//! GIF encoding from RGBA frames on a background thread, shared by every back end. Each frame lasts until
//! the next one arrives, so a still screen (which sends no new frames) costs one frame, not many.

use crate::{Result, Video};
use std::path::Path;
use std::sync::mpsc::{sync_channel, Receiver, SyncSender};
use std::thread::JoinHandle;
use std::time::Instant;

/// Frames waiting for the encoder. Past this the capture drops frames rather than queue megabytes; the
/// next frame's delay covers the gap.
const BACKLOG: usize = 8;

/// NeuQuant's speed, 1 (best) to 30 (fastest). 10 is the gif crate's suggested balance.
const SPEED: i32 = 10;

enum Message {
    Frame(Vec<u8>, Instant),
    End(Instant),
}

/// Where a capture callback sends frames. Cheap to clone into a callback.
#[derive(Clone)]
pub(crate) struct Frames {
    tx: SyncSender<Message>,
    width: u32,
    height: u32,
}

impl Frames {
    /// Queues one straight-alpha RGBA frame taken at `at`, or drops it if the encoder is behind or the
    /// frame has the wrong size.
    pub(crate) fn push(&self, rgba: Vec<u8>, at: Instant) {
        if rgba.len() != (self.width * self.height * 4) as usize {
            return;
        }
        let _ = self.tx.try_send(Message::Frame(rgba, at));
    }
}

/// The encoder thread; `finish` closes the file.
pub(crate) struct Encoder {
    tx: SyncSender<Message>,
    thread: JoinHandle<Result<Video>>,
}

impl Encoder {
    /// The last frame lasts until `end`. Answers the GIF's size and duration.
    pub(crate) fn finish(self, end: Instant) -> Result<Video> {
        // `send` blocks until there is room, so the end is never dropped.
        let _ = self.tx.send(Message::End(end));
        self.thread.join().map_err(|_| "the GIF encoder panicked".to_string())?
    }
}

/// Creates the GIF at `path` and starts its encoder thread.
pub(crate) fn start(path: &Path, width: u32, height: u32) -> Result<(Frames, Encoder)> {
    let (w, h) = (u16::try_from(width), u16::try_from(height));
    let (Ok(w), Ok(h)) = (w, h) else { return Err(format!("a GIF cannot be {width}×{height}")) };
    let file = std::fs::File::create(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut gif = gif::Encoder::new(std::io::BufWriter::new(file), w, h, &[]).map_err(|e| e.to_string())?;
    gif.set_repeat(gif::Repeat::Infinite).map_err(|e| e.to_string())?;
    let (tx, rx) = sync_channel(BACKLOG);
    let thread = std::thread::Builder::new()
        .name("glint-gif".into())
        .spawn(move || encode(gif, rx, width, height))
        .map_err(|e| e.to_string())?;
    Ok((Frames { tx: tx.clone(), width, height }, Encoder { tx, thread }))
}

type Gif = gif::Encoder<std::io::BufWriter<std::fs::File>>;

fn encode(mut gif: Gif, rx: Receiver<Message>, width: u32, height: u32) -> Result<Video> {
    let mut first: Option<Instant> = None;
    // The frame waiting for its delay, which the next frame (or the end) decides.
    let mut pending: Option<Vec<u8>> = None;
    // Centiseconds written so far, so rounding never drifts from the clock.
    let mut written: u64 = 0;
    let mut write = |gif: &mut Gif, rgba: &mut Vec<u8>, start: Instant, until: Instant| -> Result<()> {
        let total = (until.duration_since(start).as_millis() as u64).div_ceil(10);
        // Browsers stretch delays under 2 cs to 10 cs, so never write one.
        let delay = total.saturating_sub(written).max(2);
        let mut frame = gif::Frame::from_rgba_speed(width as u16, height as u16, rgba, SPEED);
        frame.delay = delay.min(u64::from(u16::MAX)) as u16;
        written += delay;
        gif.write_frame(&frame).map_err(|e| e.to_string())
    };
    let end = loop {
        match rx.recv() {
            Ok(Message::Frame(rgba, at)) => {
                let start = *first.get_or_insert(at);
                if let Some(mut previous) = pending.replace(rgba) {
                    write(&mut gif, &mut previous, start, at)?;
                }
            }
            Ok(Message::End(at)) => break at,
            Err(_) => return Err("the recording ended without finishing its GIF".into()),
        }
    };
    let (Some(start), Some(mut last)) = (first, pending) else {
        return Err("the recording captured no frames".into());
    };
    write(&mut gif, &mut last, start, end)?;
    drop(gif);
    Ok(Video { width, height, duration_ms: written * 10 })
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    #[test]
    fn frames_last_until_the_next_one() {
        let dir = std::env::temp_dir().join(format!("glint-gif-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("t.gif");
        let (frames, encoder) = super::start(&path, 4, 2).unwrap();
        let t = std::time::Instant::now();
        frames.push(vec![255; 32], t);
        frames.push(vec![0; 32], t + Duration::from_millis(500));
        frames.push(vec![0; 31], t + Duration::from_millis(600)); // the wrong size: dropped
        let video = encoder.finish(t + Duration::from_millis(1200)).unwrap();
        assert_eq!((video.width, video.height, video.duration_ms), (4, 2, 1200));

        let mut decoder = gif::DecodeOptions::new().read_info(std::fs::File::open(&path).unwrap()).unwrap();
        let mut delays = Vec::new();
        while let Some(frame) = decoder.read_next_frame().unwrap() {
            delays.push(frame.delay);
        }
        assert_eq!(delays, [50, 70]);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
