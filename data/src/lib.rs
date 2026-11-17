//! Glint's Rust half: the sources that capture the screen. `glint-capture` writes each picture or recording
//! straight into a staging folder and this answers what it wrote; app.ts moves it into app storage and
//! answers every other source (docs/reference.md, "Rust data sources"; docs/design.md).
//!
//! Why staging: a Rust source's storage requests take `app:/` paths only, and write binary as a JSON array of
//! numbers capped at 16 MiB per request, which a Retina PNG can overflow. So Rust writes the file itself, outside
//! app storage, and the Mac's Swift module moves it in. On Linux it writes into the folder the person chose
//! (a `doc:` folder; each capture source's last argument is its native path), from which the host copies it in.

use exact_runner::{Answer, DataError, DataSource, Store, Value};

#[allow(dead_code, clippy::all)]
mod shapes {
    include!(concat!(env!("OUT_DIR"), "/shapes.rs"));
}

/// The sources this crate answers; app.ts answers the rest.
pub const RUST_SOURCES: &[&str] = &["captureScreen", "captureArea", "captureWindow", "startRecording", "stopRecording", "platform"];

/// Answers the capture sources. It holds the recording between `startRecording` and `stopRecording`.
#[derive(Default)]
pub struct Glint {
    #[cfg(not(target_arch = "wasm32"))]
    live: Option<native::Live>,
}

#[cfg(not(target_arch = "wasm32"))]
mod native {
    use crate::shapes::{ContractValue, Platform, Recording, Staged};
    use exact_runner::Value;
    use glint_capture::{RecordOptions, Rect, Shot, VideoKind};
    use std::path::{Path, PathBuf};

    /// Also named in app.ts's grants, which read and remove what is staged here.
    #[cfg(target_os = "macos")]
    const STAGING: &str = "/private/tmp/glint";
    #[cfg(not(target_os = "macos"))]
    const STAGING: &str = "/tmp/glint";

    /// A recording between `startRecording` and `stopRecording`.
    pub(crate) struct Live {
        recording: glint_capture::Recording,
        path: PathBuf,
        started: f64,
    }

    /// The positional arguments, read leniently: a missing or mistyped one is zero, "" or false.
    struct Args<'a>(&'a [Value]);

    impl Args<'_> {
        fn number(&self, i: usize) -> f64 {
            self.0.get(i).and_then(Value::as_number).unwrap_or(0.0)
        }
        fn text(&self, i: usize) -> &str {
            self.0.get(i).and_then(Value::as_str).unwrap_or("")
        }
        fn flag(&self, i: usize) -> bool {
            self.0.get(i).and_then(Value::as_bool).unwrap_or(false)
        }
        fn rect(&self, from: usize) -> Rect {
            let n = |i| self.number(from + i);
            Rect { x: n(0), y: n(1), width: n(2), height: n(3) }
        }
    }

    /// Under the agent (the app's tests) nobody answers the desktop's picker, so an empty area is the fixed one
    /// the Mac's module picks there (modules/apple/Glint.swift).
    fn stand_in(rect: Rect) -> Rect {
        let empty = rect.width < 1.0 || rect.height < 1.0;
        match empty && std::env::var("EXACT_AGENT").as_deref() == Ok("1") {
            true => Rect { x: 100.0, y: 100.0, width: 400.0, height: 300.0 },
            false => rect,
        }
    }

    fn now() -> f64 {
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0.0, |d| d.as_millis() as f64)
    }

    /// `<folder>/<taken>.<extension>`, in the chosen folder or else STAGING, made if need be.
    fn staged(folder: &str, taken: f64, extension: &str) -> Result<PathBuf, String> {
        let folder = if folder.is_empty() { STAGING } else { folder };
        std::fs::create_dir_all(folder).map_err(|e| format!("{folder}: {e}"))?;
        Ok(Path::new(folder).join(format!("{taken}.{extension}")))
    }

    /// Renames a finished capture to its library name (app.ts `keepCapture`, docs/design.md), so the chosen
    /// folder holds what the library does: glint-<taken>-<w>x<h>.png, or -<ms>.<extension> for a recording.
    fn named(path: &Path, staged: &Staged) -> Result<PathBuf, String> {
        let Staged { taken, width, height, duration, kind, .. } = staged;
        let stem = format!("glint-{taken}-{width}x{height}");
        let name = if kind == "png" { format!("{stem}.png") } else { format!("{stem}-{duration}.{kind}") };
        let to = path.with_file_name(name);
        std::fs::rename(path, &to).map_err(|e| format!("{}: {e}", to.display()))?;
        Ok(to)
    }

    fn text(path: &Path) -> String {
        path.to_string_lossy().into_owned()
    }

    fn shot(folder: &str, take: impl FnOnce(&Path) -> glint_capture::Result<Shot>) -> Result<Value, String> {
        let taken = now();
        let path = staged(folder, taken, "png")?;
        let shot = take(&path)?;
        let mut staged = Staged {
            path: String::new(),
            taken,
            width: f64::from(shot.width),
            height: f64::from(shot.height),
            kind: "png".into(),
            duration: 0.0,
            mosaic: text(&shot.mosaic),
        };
        staged.path = text(&named(&path, &staged)?);
        Ok(staged.to_value())
    }

    pub(crate) fn run(live: &mut Option<Live>, source: &str, args: &[Value]) -> Result<Value, String> {
        let args = Args(args);
        match source {
            // Where the app runs, so the view leaves out the Mac's panels and menu bar item elsewhere.
            "platform" => Ok(Platform { os: std::env::consts::OS.into() }.to_value()),
            "captureScreen" => shot(args.text(0), glint_capture::screen),
            "captureArea" => shot(args.text(4), |to| glint_capture::area(stand_in(args.rect(0)), to)),
            "captureWindow" => shot(args.text(1), |to| glint_capture::window(args.number(0) as u32, to)),
            "startRecording" => {
                if live.is_some() {
                    return Err("a recording is already running".into());
                }
                let area = args.rect(0);
                let options = RecordOptions {
                    area: (area.width > 0.0 && area.height > 0.0).then_some(area),
                    kind: VideoKind::parse(args.text(4))?,
                    cursor: args.flag(5),
                    clicks: args.flag(6),
                    audio: args.flag(7),
                    microphone: args.flag(8),
                };
                let started = now();
                let path = staged(args.text(9), started, options.kind.extension())?;
                let recording = glint_capture::record(&options, &path)?;
                let kind = recording.kind().extension().into();
                *live = Some(Live { recording, path, started });
                Ok(Recording { recording: true, started, kind }.to_value())
            }
            _ => {
                let Live { recording, path, started } = live.take().ok_or("no recording is running")?;
                let scroll = recording.kind() == VideoKind::Scroll;
                let kind = recording.kind().extension().into();
                let video = recording.stop()?;
                let mut staged = Staged {
                    path: String::new(),
                    taken: started,
                    width: f64::from(video.width),
                    height: f64::from(video.height),
                    kind,
                    duration: video.duration_ms as f64,
                    mosaic: if scroll { text(&glint_capture::mosaic_path(&path)) } else { String::new() },
                };
                staged.path = text(&named(&path, &staged)?);
                Ok(staged.to_value())
            }
        }
    }
}

impl Glint {
    #[cfg(not(target_arch = "wasm32"))]
    fn run(&mut self, source: &str, args: &[Value]) -> Result<Value, String> {
        native::run(&mut self.live, source, args)
    }

    #[cfg(target_arch = "wasm32")]
    fn run(&mut self, source: &str, _: &[Value]) -> Result<Value, String> {
        use shapes::ContractValue as _;
        match source {
            "platform" => Ok(shapes::Platform { os: "web".into() }.to_value()),
            _ => Err("the browser cannot capture the screen".into()),
        }
    }
}

impl DataSource for Glint {
    fn app_id(&self) -> &str {
        "dev.mchisolm0.glint"
    }

    fn query(&mut self, source: &str, _: &[Value]) -> Result<Value, DataError> {
        Err(match RUST_SOURCES.contains(&source) {
            true => DataError::Unavailable("a capture happens at run time".into()),
            false => DataError::UnknownSource(source.into()),
        })
    }

    fn answer(&mut self, _: &mut Store, source: &str, args: &[Value]) -> Result<Answer, DataError> {
        if !RUST_SOURCES.contains(&source) {
            return Err(DataError::UnknownSource(source.into()));
        }
        self.run(source, args).map(Answer::Now).map_err(DataError::Unavailable)
    }
}
