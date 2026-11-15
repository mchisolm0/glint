//! Frames from a capture callback to a worker thread: the GIF encoder (`gif.rs`) or the scrolling stitcher
//! (`scroll.rs`). The callback never blocks: past a small backlog it drops frames.

use crate::{Result, Video};
use std::sync::mpsc::{sync_channel, Receiver, SyncSender};
use std::thread::JoinHandle;
use std::time::Instant;

/// Frames waiting for the worker. Past this the capture drops frames rather than queue megabytes.
const BACKLOG: usize = 8;

pub(crate) enum Message {
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
    /// Queues one straight-alpha RGBA frame taken at `at`, or drops it if the worker is behind or the frame
    /// has the wrong size.
    pub(crate) fn push(&self, rgba: Vec<u8>, at: Instant) {
        if rgba.len() != (self.width * self.height * 4) as usize {
            return;
        }
        let _ = self.tx.try_send(Message::Frame(rgba, at));
    }
}

/// The worker thread; `finish` waits for its file.
pub(crate) struct Encoder {
    tx: SyncSender<Message>,
    thread: JoinHandle<Result<Video>>,
}

impl Encoder {
    /// Ends the frames at `end` and answers what the worker made of them.
    pub(crate) fn finish(self, end: Instant) -> Result<Video> {
        // `send` blocks until there is room, so the end is never dropped.
        let _ = self.tx.send(Message::End(end));
        self.thread.join().map_err(|_| "the frame worker panicked".to_string())?
    }
}

/// Starts `work` on a thread named `name`, fed `width` × `height` frames.
pub(crate) fn spawn(
    name: &str,
    width: u32,
    height: u32,
    work: impl FnOnce(Receiver<Message>) -> Result<Video> + Send + 'static,
) -> Result<(Frames, Encoder)> {
    let (tx, rx) = sync_channel(BACKLOG);
    let thread = std::thread::Builder::new().name(name.into()).spawn(move || work(rx)).map_err(|e| e.to_string())?;
    Ok((Frames { tx: tx.clone(), width, height }, Encoder { tx, thread }))
}
