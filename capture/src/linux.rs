//! The Linux back end, not written yet. It will ask xdg-desktop-portal (through ashpd) for a Screenshot,
//! and for a ScreenCast whose PipeWire frames feed the shared GIF encoder. Until then every call refuses.
//! It also stands in on any other desktop without a back end.

use crate::{RecordOptions, Rect, Result, Shot, Video};
use std::path::Path;

const NOT_YET: &str = "screen capture on this platform is not written yet (macOS only for now)";

pub(crate) struct Recording;

impl Recording {
    pub(crate) fn stop(self) -> Result<Video> {
        Err(NOT_YET.into())
    }
}

pub(crate) fn screen(_: &Path) -> Result<Shot> {
    Err(NOT_YET.into())
}

pub(crate) fn area(_: Rect, _: &Path) -> Result<Shot> {
    Err(NOT_YET.into())
}

pub(crate) fn window(_: u32, _: &Path) -> Result<Shot> {
    Err(NOT_YET.into())
}

pub(crate) fn record(_: &RecordOptions, _: &Path) -> Result<Recording> {
    Err(NOT_YET.into())
}
