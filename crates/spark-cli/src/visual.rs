//! `spark screenshot`, `spark inspect`, `spark diff`, `spark golden`: see what the game looks like and how it is built,
//! as JSON, without a window. They run the game headless through this same executable.

use std::path::{Path, PathBuf};

use image::RgbaImage;
use serde_json::{Value, json};

use crate::util::*;

fn game_dir(arg: Option<&String>) -> PathBuf {
    let p = PathBuf::from(arg.map(String::as_str).unwrap_or("."));
    if p.is_file() {
        p.parent().map(Path::to_path_buf).filter(|d| !d.as_os_str().is_empty()).unwrap_or_else(|| PathBuf::from("."))
    } else {
        p
    }
}

/// `--help` / `-h` anywhere in `args`: prints the usage line and returns the exit code to stop with.
fn help(args: &[String], usage: &str) -> Option<i32> {
    args.iter().any(|a| a == "--help" || a == "-h").then(|| {
        println!("{usage}");
        EXIT_OK
    })
}

const SCREENSHOT_USAGE: &str = "usage: spark screenshot path/to/game [--frames N] [--size WxH] [--out FILE] [--json]";
const INSPECT_USAGE: &str = "usage: spark inspect path/to/game [--frames N] [--json]";
const DIFF_USAGE: &str = "usage: spark diff a.png b.png [--tolerance 0.01] [--max-bad 0.02] [--out diff.png] [--json]";
const GOLDEN_USAGE: &str =
    "usage: spark golden path/to/game [--golden FILE] [--update] [--frames N] [--size WxH] [--tolerance T] [--max-bad F] [--json]";

fn fail(command: &str, json: bool, code: i32, msg: &str) -> i32 {
    if json {
        print_json(&json!({ "command": command, "ok": false, "exit_code": code, "error": msg }));
    } else {
        eprintln!("spark {command}: {msg}");
    }
    code
}

/// Renders `frames` frames headless and saves the last one to `out`. Returns the child's stderr tail on failure.
fn render(dir: &Path, frames: u32, size: &str, out: &Path) -> Result<(), String> {
    if let Some(parent) = out.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent).map_err(|e| format!("cannot create '{}': {e}", parent.display()))?;
    }
    let args: Vec<String> = [
        &dir.display().to_string(),
        "--headless",
        "--mute",
        "--frames",
        &frames.to_string(),
        "--size",
        size,
        "--screenshot",
        &out.display().to_string(),
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    let c = run_self(&args, None)?;
    if c.code != 0 || !out.is_file() {
        return Err(tail(&c.stderr, 20));
    }
    Ok(())
}

pub fn screenshot(mut args: Vec<String>) -> i32 {
    if let Some(code) = help(&args, SCREENSHOT_USAGE) {
        return code;
    }
    let json = take_flag(&mut args, "--json");
    let parse = |args: &mut Vec<String>| -> Result<(u32, String, Option<String>), String> {
        let frames = take_value(args, "--frames")?
            .map(|v| v.parse::<u32>().map_err(|_| "--frames: expected a whole number".to_string()))
            .transpose()?
            .unwrap_or(30);
        let size = take_value(args, "--size")?.unwrap_or_else(|| "1280x720".into());
        let out = take_value(args, "--out")?;
        Ok((frames.max(1), size, out))
    };
    let (frames, size, out) = match parse(&mut args) {
        Ok(v) => v,
        Err(e) => return fail("screenshot", json, EXIT_USAGE, &e),
    };
    let dir = game_dir(args.first());
    let out = PathBuf::from(out.unwrap_or_else(|| "screenshots/shot.png".into()));
    match render(&dir, frames, &size, &out) {
        Ok(()) => {
            let (w, h) = image::image_dimensions(&out).unwrap_or((0, 0));
            let abs = out.canonicalize().unwrap_or(out.clone());
            if json {
                print_json(
                    &json!({ "command": "screenshot", "ok": true, "path": plain_path(&abs), "width": w, "height": h, "frames": frames }),
                );
            } else {
                println!("spark screenshot: {} ({w}x{h}, frame {frames})", plain_path(&abs));
            }
            EXIT_OK
        }
        Err(e) => fail("screenshot", json, EXIT_FAILED, &format!("the game did not render:\n{e}")),
    }
}

pub fn inspect(mut args: Vec<String>) -> i32 {
    if let Some(code) = help(&args, INSPECT_USAGE) {
        return code;
    }
    let json = take_flag(&mut args, "--json");
    let frames = match take_value(&mut args, "--frames") {
        Ok(v) => v.and_then(|n| n.parse::<u32>().ok()).unwrap_or(60).max(1),
        Err(e) => return fail("inspect", json, EXIT_USAGE, &e),
    };
    let dir = game_dir(args.first());
    let child_args: Vec<String> =
        [&dir.display().to_string(), "--headless", "--mute", "--frames", &frames.to_string(), "--dump-scene-json", "-"]
            .iter()
            .map(|s| s.to_string())
            .collect();
    let c = match run_self(&child_args, None) {
        Ok(c) => c,
        Err(e) => return fail("inspect", json, EXIT_USAGE, &e),
    };
    let scene: Option<Value> = c.stdout.lines().rev().find(|l| l.trim_start().starts_with('{')).and_then(|l| serde_json::from_str(l).ok());
    match (c.code, scene) {
        (0, Some(scene)) => {
            let objects = scene.get("objects").and_then(Value::as_array).map(Vec::len).unwrap_or(0);
            if json {
                print_json(&json!({ "command": "inspect", "ok": true, "frames": frames, "object_count": objects, "scene": scene }));
            } else {
                println!("spark inspect: {objects} objects after {frames} frames");
                println!("{}", serde_json::to_string_pretty(&scene).unwrap_or_default());
            }
            EXIT_OK
        }
        (code, _) => fail(
            "inspect",
            json,
            if code == 0 { EXIT_FAILED } else { code },
            &format!("the game did not produce a scene dump:\n{}", tail(&c.stderr, 20)),
        ),
    }
}

/// Differences between two images.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Diff {
    /// Mean absolute channel difference, 0..1.
    pub mean_error: f64,
    /// Fraction of pixels where any channel differs by more than `PIXEL_THRESHOLD`.
    pub bad_fraction: f64,
}

const PIXEL_THRESHOLD: i32 = 24;

pub fn compare(a: &RgbaImage, b: &RgbaImage) -> Result<(Diff, RgbaImage), String> {
    if a.dimensions() != b.dimensions() {
        return Err(format!("sizes differ: {}x{} vs {}x{}", a.width(), a.height(), b.width(), b.height()));
    }
    let mut map = RgbaImage::new(a.width(), a.height());
    let (mut sum, mut bad) = (0u64, 0u64);
    for ((pa, pb), pm) in a.pixels().zip(b.pixels()).zip(map.pixels_mut()) {
        let mut worst = 0i32;
        for c in 0..4 {
            let d = (pa[c] as i32 - pb[c] as i32).abs();
            sum += d as u64;
            worst = worst.max(d);
        }
        if worst > PIXEL_THRESHOLD {
            bad += 1;
        }
        let v = (worst * 4).min(255) as u8;
        *pm = image::Rgba([v, 0, 0, 255]);
    }
    let n = (a.width() as u64 * a.height() as u64).max(1);
    Ok((Diff { mean_error: sum as f64 / (n * 4 * 255) as f64, bad_fraction: bad as f64 / n as f64 }, map))
}

pub fn diff(mut args: Vec<String>) -> i32 {
    if let Some(code) = help(&args, DIFF_USAGE) {
        return code;
    }
    let json = take_flag(&mut args, "--json");
    let (tol, max_bad, out) =
        match (take_value(&mut args, "--tolerance"), take_value(&mut args, "--max-bad"), take_value(&mut args, "--out")) {
            (Ok(t), Ok(b), Ok(o)) => {
                (t.and_then(|v| v.parse::<f64>().ok()).unwrap_or(0.01), b.and_then(|v| v.parse::<f64>().ok()).unwrap_or(0.02), o)
            }
            (Err(e), _, _) | (_, Err(e), _) | (_, _, Err(e)) => return fail("diff", json, EXIT_USAGE, &e),
        };
    let [a, b] = args.as_slice() else {
        return fail("diff", json, EXIT_USAGE, DIFF_USAGE);
    };
    let load = |p: &String| image::open(p).map(|i| i.to_rgba8()).map_err(|e| format!("cannot read '{p}': {e}"));
    let (ia, ib) = match (load(a), load(b)) {
        (Ok(x), Ok(y)) => (x, y),
        (Err(e), _) | (_, Err(e)) => return fail("diff", json, EXIT_USAGE, &e),
    };
    let (d, map) = match compare(&ia, &ib) {
        Ok(r) => r,
        Err(e) => return fail("diff", json, EXIT_FAILED, &e),
    };
    let same = d.mean_error <= tol && d.bad_fraction <= max_bad;
    if let Some(o) = &out {
        let _ = map.save(o);
    }
    if json {
        print_json(
            &json!({ "command": "diff", "ok": same, "exit_code": if same { 0 } else { 1 }, "mean_error": d.mean_error, "bad_fraction": d.bad_fraction, "tolerance": tol, "max_bad": max_bad, "diff_image": out }),
        );
    } else {
        println!(
            "spark diff: mean error {:.4}, {:.2}% of pixels differ by more than {PIXEL_THRESHOLD}/255 -> {}",
            d.mean_error,
            d.bad_fraction * 100.0,
            if same { "same" } else { "DIFFERENT" }
        );
    }
    if same { EXIT_OK } else { EXIT_FAILED }
}

/// `spark golden path/to/game [--golden FILE] [--update] [--frames N] [--size WxH] [--tolerance T]`: compares a headless render
/// with `tests/golden.png` (or FILE); on a mismatch writes `screenshots/golden-current.png` and `screenshots/golden-diff.png`.
pub fn golden(mut args: Vec<String>) -> i32 {
    if let Some(code) = help(&args, GOLDEN_USAGE) {
        return code;
    }
    let json = take_flag(&mut args, "--json");
    let update = take_flag(&mut args, "--update");
    let golden_file = match take_value(&mut args, "--golden") {
        Ok(v) => v,
        Err(e) => return fail("golden", json, EXIT_USAGE, &e),
    };
    let opts = (|| -> Result<(u32, String, f64, f64), String> {
        let frames = take_value(&mut args, "--frames")?
            .map(|v| v.parse::<u32>().map_err(|_| "--frames: expected a whole number".to_string()))
            .transpose()?
            .unwrap_or(30);
        let size = take_value(&mut args, "--size")?.unwrap_or_else(|| "640x360".into());
        let tol = take_value(&mut args, "--tolerance")?.and_then(|v| v.parse().ok()).unwrap_or(0.02);
        let bad = take_value(&mut args, "--max-bad")?.and_then(|v| v.parse().ok()).unwrap_or(0.05);
        Ok((frames.max(1), size, tol, bad))
    })();
    let (frames, size, tol, max_bad) = match opts {
        Ok(o) => o,
        Err(e) => return fail("golden", json, EXIT_USAGE, &e),
    };
    let dir = game_dir(args.first());
    let golden = golden_file.map(PathBuf::from).unwrap_or_else(|| dir.join("tests/golden.png"));
    let current = dir.join("screenshots/golden-current.png");
    if let Err(e) = render(&dir, frames, &size, &current) {
        return fail("golden", json, EXIT_FAILED, &format!("the game did not render:\n{e}"));
    }
    if update || !golden.is_file() {
        if !update {
            return fail(
                "golden",
                json,
                EXIT_FAILED,
                &format!("no golden image at {}: run `spark golden {} --update` and review the picture", golden.display(), dir.display()),
            );
        }
        if let Some(p) = golden.parent() {
            let _ = std::fs::create_dir_all(p);
        }
        if let Err(e) = std::fs::copy(&current, &golden) {
            return fail("golden", json, EXIT_USAGE, &format!("cannot write {}: {e}", golden.display()));
        }
        if json {
            print_json(&json!({ "command": "golden", "ok": true, "updated": true, "golden": plain_path(&golden) }));
        } else {
            println!("spark golden: wrote {}", golden.display());
        }
        return EXIT_OK;
    }
    let load = |p: &Path| image::open(p).map(|i| i.to_rgba8()).map_err(|e| format!("cannot read '{}': {e}", p.display()));
    let (a, b) = match (load(&golden), load(&current)) {
        (Ok(a), Ok(b)) => (a, b),
        (Err(e), _) | (_, Err(e)) => return fail("golden", json, EXIT_USAGE, &e),
    };
    match compare(&a, &b) {
        Ok((d, map)) => {
            let same = d.mean_error <= tol && d.bad_fraction <= max_bad;
            let diff_path = dir.join("screenshots/golden-diff.png");
            if !same {
                let _ = map.save(&diff_path);
            }
            if json {
                print_json(
                    &json!({ "command": "golden", "ok": same, "exit_code": if same { 0 } else { 1 }, "mean_error": d.mean_error, "bad_fraction": d.bad_fraction, "tolerance": tol, "max_bad": max_bad,
                    "current": plain_path(&current), "diff_image": if same { Value::Null } else { json!(plain_path(&diff_path)) } }),
                );
            } else if same {
                println!("spark golden: same (mean error {:.4}, {:.2}% pixels differ)", d.mean_error, d.bad_fraction * 100.0);
            } else {
                eprintln!(
                    "spark golden: DIFFERENT (mean error {:.4} > {tol}, or {:.2}% pixels differ > {:.0}%). See {} and {}",
                    d.mean_error,
                    d.bad_fraction * 100.0,
                    max_bad * 100.0,
                    current.display(),
                    diff_path.display()
                );
            }
            if same { EXIT_OK } else { EXIT_FAILED }
        }
        Err(e) => fail("golden", json, EXIT_FAILED, &format!("{e}; render at the golden's size (--size) or run --update")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn help_flags_print_usage_instead_of_running() {
        for flag in ["--help", "-h"] {
            let a = vec![flag.to_string()];
            assert_eq!(screenshot(a.clone()), EXIT_OK);
            assert_eq!(inspect(a.clone()), EXIT_OK);
            assert_eq!(diff(a.clone()), EXIT_OK);
            assert_eq!(golden(a), EXIT_OK);
        }
    }

    #[test]
    fn compare_images() {
        let a = RgbaImage::from_pixel(4, 4, image::Rgba([10, 20, 30, 255]));
        let mut b = a.clone();
        let (d, _) = compare(&a, &b).unwrap();
        assert_eq!((d.mean_error, d.bad_fraction), (0.0, 0.0));
        b.put_pixel(0, 0, image::Rgba([200, 20, 30, 255]));
        let (d, map) = compare(&a, &b).unwrap();
        assert!((d.bad_fraction - 1.0 / 16.0).abs() < 1e-9);
        assert!(d.mean_error > 0.0 && d.mean_error < 0.02);
        assert_eq!(map.get_pixel(0, 0)[0], 255);
        assert!(compare(&a, &RgbaImage::new(2, 2)).is_err());
    }
}
