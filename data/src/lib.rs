//! Glint's Rust half: the two sources that take a picture. `glint-capture` (xcap) grabs the pixels and this
//! writes the PNG to a staging folder; app.ts moves it into app storage and answers every other source
//! (docs/reference.md, "Rust data sources").
//!
//! Why staging: a Rust source's storage requests take `app:/` paths only, and write binary as a JSON array of
//! numbers capped at 16 MiB per request, which a Retina PNG can overflow. So Rust writes the file itself, outside
//! app storage, and TypeScript (whose grants may name a native folder) copies it in.

use exact_runner::{Answer, DataError, DataSource, Store, Value};

#[allow(dead_code, clippy::all)]
mod shapes {
    include!(concat!(env!("OUT_DIR"), "/shapes.rs"));
}
use shapes::{ContractValue, Staged};

/// The sources this crate answers; app.ts answers the rest.
pub const RUST_SOURCES: &[&str] = &["captureScreen", "captureArea"];

/// Also named in app.ts's grants, which read and remove what is staged here.
#[cfg(target_os = "macos")]
const STAGING: &str = "/private/tmp/glint";
#[cfg(not(target_os = "macos"))]
const STAGING: &str = "/tmp/glint";

#[cfg(not(target_arch = "wasm32"))]
fn take(source: &str, args: &[Value]) -> Result<Staged, String> {
    let n = |i: usize| args.get(i).and_then(Value::as_number).unwrap_or(0.0);
    let shot = match source {
        "captureArea" => glint_capture::area(n(0) as i32, n(1) as i32, n(2) as u32, n(3) as u32)?,
        _ => glint_capture::screen(0, 0)?,
    };
    let taken = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_millis();
    let path = format!("{STAGING}/{taken}.png");
    std::fs::create_dir_all(STAGING).map_err(|e| e.to_string())?;
    std::fs::write(&path, &shot.png).map_err(|e| e.to_string())?;
    Ok(Staged { path, taken: taken as f64, width: shot.width as f64, height: shot.height as f64 })
}

#[cfg(target_arch = "wasm32")]
fn take(_: &str, _: &[Value]) -> Result<Staged, String> {
    Err("the browser cannot capture the screen".into())
}

#[derive(Default)]
pub struct Glint;

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
        let staged = take(source, args).map_err(DataError::Unavailable)?;
        Ok(Answer::Now(staged.to_value()))
    }
}
