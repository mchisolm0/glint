//! Drives one capture against the running desktop, for checking a back end by hand:
//! `cargo run --example live -- <screen|area|pick|window|mp4|gif|scroll> <dir> [seconds] [x y width height]`.
//! `area` takes a fixed 300 × 200 rect at (40, 60); `pick` passes an empty one, so the portal asks.
//! It prints what it wrote; delete the files once checked, since they show the screen.

use glint_capture::{RecordOptions, Rect, VideoKind};
use std::path::Path;

fn main() -> Result<(), String> {
    let args: Vec<String> = std::env::args().collect();
    let (what, dir) = (
        args.get(1).map_or("screen", String::as_str),
        Path::new(args.get(2).map_or("/tmp/glint-live", String::as_str)),
    );
    let seconds: u64 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(2);
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let shot = |name: &str| dir.join(format!("{name}.png"));
    let zero = Rect { x: 0.0, y: 0.0, width: 0.0, height: 0.0 };
    match what {
        "screen" => println!("{:?}", glint_capture::screen(&shot("screen"))?),
        "area" => println!(
            "{:?}",
            glint_capture::area(Rect { x: 40.0, y: 60.0, width: 300.0, height: 200.0 }, &shot("area"))?
        ),
        "pick" => println!("{:?}", glint_capture::area(zero, &shot("pick"))?),
        "window" => println!("{:?}", glint_capture::window(0, &shot("window"))?),
        "mp4" | "gif" | "mp4-area" => {
            let kind = VideoKind::parse(&what[..3])?;
            let area = (what == "mp4-area").then_some(Rect { x: 100.0, y: 50.0, width: 641.0, height: 401.0 });
            let options = RecordOptions { area, kind, cursor: true, clicks: false, audio: false, microphone: false };
            let recording = glint_capture::record(&options, &dir.join(format!("rec.{}", kind.extension())))?;
            println!("recording");
            std::thread::sleep(std::time::Duration::from_secs(seconds));
            println!("{:?}", recording.stop()?);
        }
        "scroll" => {
            let n = |i: usize| args.get(i).and_then(|s| s.parse().ok()).unwrap_or(0.0);
            let area = Some(Rect { x: n(4), y: n(5), width: n(6), height: n(7) });
            let options = RecordOptions { area, kind: VideoKind::Scroll, cursor: false, clicks: false, audio: false, microphone: false };
            let recording = glint_capture::record(&options, &shot("scroll"))?;
            println!("scroll now");
            std::thread::sleep(std::time::Duration::from_secs(seconds));
            println!("{:?}", recording.stop()?);
        }
        other => return Err(format!("unknown capture {other}")),
    }
    Ok(())
}
