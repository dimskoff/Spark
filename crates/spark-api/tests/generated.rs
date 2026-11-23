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
    // The component regions of spark.d.luau come from the registry; everything else is derived from the result
    // (not from the compiled-in copy, which is stale during a bless run).
    let luau = spark_api::components::splice(spark_api::D_LUAU).expect("markers in docs/spark.d.luau");
    check("docs/spark.d.luau", luau.clone());
    let api = spark_api::parse(&luau).expect("generated spark.d.luau parses");
    check("docs/API_REFERENCE.md", spark_api::markdown(&api, VERSION));
    check("docs/api.json", spark_api::api_json(&api));
    check("llms.txt", spark_api::llms_txt(VERSION));
}

/// The spec types and class fields come from the registry: every spawn key is a documented field, and the registry's
/// Luau names are all declared on `Object` (or on the `light` handle).
#[test]
fn registry_fields_are_declared() {
    let api = spark_api::api();
    let object = api.class("Object").unwrap();
    let light = api.class("LightRef").unwrap();
    for c in spark_api::components_json().components.iter().chain(std::iter::once(&spark_api::components_json().node)) {
        for f in c.fields.iter().filter(|f| f.lua.is_some()) {
            let name = f.lua.as_deref().unwrap();
            let class = if c.lua_scope == "object" { object } else { light };
            assert!(class.fields.iter().any(|d| d.name == name), "{}.{} (Luau '{name}') is not declared", c.name, f.name);
        }
    }
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
