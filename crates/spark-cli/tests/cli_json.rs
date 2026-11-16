//! Every command speaks JSON with stable fields and exit codes (0 ok, 1 failed, 2 usage / environment).

use std::process::Command;

use serde_json::Value;

fn spark(args: &[&str]) -> (i32, Value, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_spark")).args(args).output().unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let json =
        serde_json::from_str(stdout.trim()).unwrap_or_else(|e| panic!("spark {args:?} printed no single JSON object ({e}):\n{stdout}"));
    (out.status.code().unwrap_or(-1), json, String::from_utf8_lossy(&out.stderr).into_owned())
}

fn temp(name: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("spark-cli-json-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    d
}

#[test]
fn new_check_test_json_cycle() {
    let dir = temp("cycle");
    let d = dir.to_str().unwrap();
    let (code, v, _) = spark(&["new", d, "--template", "3d", "--json"]);
    assert_eq!((code, v["ok"].as_bool()), (0, Some(true)), "{v}");
    assert_eq!(v["command"], "new");
    assert!(dir.join("CLAUDE.md").is_file() && dir.join("llms.txt").is_file() && dir.join("docs/API_REFERENCE.md").is_file());

    let (code, v, _) = spark(&["check", d, "--frames", "30", "--json"]);
    assert_eq!((code, v["ok"].as_bool()), (0, Some(true)), "{v}");

    let (code, v, _) = spark(&["test", d, "--json"]);
    assert_eq!(code, 0, "{v}");

    // a broken game: exit code 1 and the error with file:line
    std::fs::write(dir.join("main.luau"), "function start()\n    spawn(Mesh.cube(0/0))\nend\n").unwrap();
    let (code, v, _) = spark(&["check", d, "--frames", "5", "--json"]);
    assert_eq!((code, v["ok"].as_bool()), (1, Some(false)), "{v}");
    assert!(v["errors"][0]["message"].as_str().unwrap().contains("finite"), "{v}");
    assert_eq!(v["errors"][0]["file"], "main.luau");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn api_commands() {
    let (code, v, _) = spark(&["api", "search", "gravity", "--json"]);
    assert_eq!(code, 0);
    assert!(v["hits"].as_array().unwrap().iter().any(|h| h["name"] == "physics.gravity"), "{v}");
    let (code, v, _) = spark(&["api", "show", "Mesh.cube", "--json"]);
    assert_eq!((code, v["items"][0]["signature"].as_str()), (0, Some("Mesh.cube(size: number?) -> Mesh")));
    let (code, v, _) = spark(&["api", "show", "no_such_thing", "--json"]);
    assert_eq!((code, v["ok"].as_bool()), (1, Some(false)));
    let (code, v, _) = spark(&["api", "bogus", "--json"]);
    assert_eq!((code, v["exit_code"].as_i64()), (2, Some(2)));
}

#[test]
fn doctor_and_diff() {
    let (code, v, _) = spark(&["doctor", "--json"]);
    assert!(code == 0 || code == 2);
    assert!(v["checks"].as_array().unwrap().iter().any(|c| c["name"] == "spark"));

    let dir = temp("diff");
    std::fs::create_dir_all(&dir).unwrap();
    let (a, b) = (dir.join("a.png"), dir.join("b.png"));
    image::RgbaImage::from_pixel(8, 8, image::Rgba([10, 10, 10, 255])).save(&a).unwrap();
    image::RgbaImage::from_pixel(8, 8, image::Rgba([250, 10, 10, 255])).save(&b).unwrap();
    let (code, v, _) = spark(&["diff", a.to_str().unwrap(), a.to_str().unwrap(), "--json"]);
    assert_eq!((code, v["ok"].as_bool()), (0, Some(true)));
    let (code, v, _) = spark(&["diff", a.to_str().unwrap(), b.to_str().unwrap(), "--json"]);
    assert_eq!((code, v["ok"].as_bool()), (1, Some(false)));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn unknown_run_flag_is_exit_2_and_help_is_exit_0() {
    let out = Command::new(env!("CARGO_BIN_EXE_spark")).args(["--headles"]).output().unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("did you mean '--headless'"));
    let out = Command::new(env!("CARGO_BIN_EXE_spark")).arg("--help").output().unwrap();
    assert_eq!(out.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&out.stdout).contains("spark mcp"));
}
