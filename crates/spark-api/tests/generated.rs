//! The generated files in the repository must match what the registry produces.
//! Regenerate with `SPARK_BLESS=1 cargo test -p spark-api`.

use std::path::{Path, PathBuf};

const VERSION: &str = env!("CARGO_PKG_VERSION");

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn check(rel: &str, expected: String) {
    let path = root().join(rel);
    let have = std::fs::read_to_string(&path).unwrap_or_default().replace("\r\n", "\n");
    let expected = expected.replace("\r\n", "\n");
    if have == expected {
        return;
    }
    if std::env::var_os("SPARK_BLESS").is_some() {
        std::fs::write(&path, expected).unwrap();
        return;
    }
    panic!("{rel} is out of date with docs/spark.d.luau: run `SPARK_BLESS=1 cargo test -p spark-api` and commit the result");
}

#[test]
fn generated_files_are_current() {
    let api = spark_api::api();
    check("docs/API_REFERENCE.md", spark_api::markdown(&api, VERSION));
    check("docs/api.json", serde_json::to_string_pretty(&api).unwrap() + "\n");
    check("llms.txt", spark_api::llms_txt(VERSION));
}

/// The narrative guide must mention every global function and table of the API.
#[test]
fn guide_mentions_every_global() {
    let api = spark_api::api();
    let guide = std::fs::read_to_string(root().join("docs/SPARK_API.md")).unwrap();
    let missing: Vec<&str> = api
        .functions
        .iter()
        .map(|f| f.name.as_str())
        .chain(api.namespaces.iter().map(|n| n.name.as_str()))
        .filter(|n| !guide.contains(&format!("{n}(")) && !guide.contains(&format!("{n}.")) && !guide.contains(&format!("`{n}`")))
        .collect();
    assert!(missing.is_empty(), "docs/SPARK_API.md does not mention: {missing:?}");
}
