//! Two storage requests a capture source would send: copy a file it wrote to a native folder into
//! app:/data, and write a file's bytes into app:/data (docs/reference.md, "Where data lives").

use exact_data::storage;
use exact_runner::{Answer, DataError, DataSource, Outcome, Store, Value};
use serde_json::json;

#[allow(dead_code, clippy::all)]
mod shapes {
    include!(concat!(env!("OUT_DIR"), "/shapes.rs"));
}
use shapes::{ContractValue, Line};

/// A folder the source writes to, as ScreenCaptureKit or a portal would.
const NATIVE: &str = "/private/tmp/exact-repro2";

#[derive(Default)]
pub struct RustSource;

fn line(text: String) -> Value {
    Line { text }.to_value()
}

impl DataSource for RustSource {
    fn app_id(&self) -> &str {
        "com.example.rust-source-705f8e"
    }

    fn grants(&self) -> &str {
        "fs.read /private/tmp/exact-repro2\nfs.write /private/tmp/exact-repro2\nfs.read app:/data\nfs.write app:/data"
    }

    // The build's first frame: nothing has run yet.
    fn query(&mut self, _: &str, _: &[Value]) -> Result<Value, DataError> {
        Ok(line("pending".into()))
    }

    fn answer(&mut self, _: &mut Store, source: &str, args: &[Value]) -> Result<Answer, DataError> {
        let request = match (source, args) {
            ("copyNative", _) => {
                std::fs::create_dir_all(NATIVE).map_err(|e| DataError::Unavailable(e.to_string()))?;
                std::fs::write(format!("{NATIVE}/shot.png"), [0x89, b'P', b'N', b'G'])
                    .map_err(|e| DataError::Unavailable(e.to_string()))?;
                storage::request(
                    "fs.copyFile",
                    json!({"path": format!("{NATIVE}/shot.png"), "destination": "app:/data/shot.png"}),
                )
            }
            // A PNG's bytes are mostly 100..=255, so each takes four characters of JSON.
            ("writeBytes", [Value::Number(n)]) => storage::request(
                "fs.writeFile",
                json!({"path": "app:/data/big.bin", "bytes": vec![200u8; *n as usize]}),
            ),
            _ => return Err(DataError::UnknownSource(source.into())),
        };
        Ok(Answer::Later(request))
    }

    fn parse(&mut self, _: &mut Store, _: &str, _: &[Value], outcome: Outcome) -> Result<Answer, DataError> {
        Ok(Answer::Now(line(match storage::response(outcome) {
            Ok(_) => "ok".into(),
            Err(e) => format!("FAILED {e}"),
        })))
    }
}
