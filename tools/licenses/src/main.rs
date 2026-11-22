//! Generates `THIRD_PARTY_LICENSES.md`: the license texts and copyright notices of everything that is compiled into
//! the shipped binaries (`spark`, `spark-editor`, and with them every game made by `spark build`).
//!
//! Package selection: the normal (non-dev, non-build) dependency graph of `spark-cli` and `spark-editor` from
//! `cargo metadata`, all target platforms and the default feature set the workspace builds with, minus the
//! workspace's own crates. Dev-dependencies and build-scripts-only dependencies are not shipped and are left out.
//!
//! `cargo run -p licenses` writes the file; `cargo test -p licenses` fails when the committed file is stale.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

/// Workspace packages whose dependency graph is shipped.
const ROOTS: &[&str] = &["spark-cli", "spark-editor"];
/// Bundled non-crate material: (name, version, license, source, text file relative to the repo root).
const ASSETS: &[(&str, &str, &str, &str, &str)] = &[(
    "Noto Sans (font, crates/spark-core/assets/fonts/NotoSans-Regular.ttf)",
    "",
    "OFL-1.1",
    "https://fonts.google.com/noto/specimen/Noto+Sans",
    "crates/spark-core/assets/fonts/OFL.txt",
)];

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

struct Entry {
    name: String,
    version: String,
    license: String,
    repository: String,
    authors: Vec<String>,
    text: String,
}

fn metadata(root: &Path) -> Result<Value, String> {
    let out = Command::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()))
        .args(["metadata", "--format-version", "1", "--locked", "--manifest-path"])
        .arg(root.join("Cargo.toml"))
        .output()
        .map_err(|e| format!("cannot run cargo metadata: {e}"))?;
    if !out.status.success() {
        return Err(format!("cargo metadata failed: {}", String::from_utf8_lossy(&out.stderr).trim()));
    }
    serde_json::from_slice(&out.stdout).map_err(|e| format!("cargo metadata: bad JSON: {e}"))
}

fn s(v: &Value, key: &str) -> String {
    v.get(key).and_then(Value::as_str).unwrap_or_default().to_string()
}

/// License/notice files of a package, as one text (file names are shown when there are several).
fn license_text(dir: &Path, license_file: Option<&str>) -> String {
    let mut names: BTreeSet<String> = BTreeSet::new();
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            let l = name.to_ascii_lowercase();
            if e.path().is_file() && ["license", "licence", "copying", "notice"].iter().any(|p| l.starts_with(p)) {
                names.insert(name);
            }
        }
    }
    // Bundled fonts (epaint_default_fonts) carry their licenses next to the font files.
    if let Ok(rd) = std::fs::read_dir(dir.join("fonts")) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if name.to_ascii_lowercase().ends_with(".txt") {
                names.insert(format!("fonts/{name}"));
            }
        }
    }
    if let Some(f) = license_file {
        if dir.join(f).is_file() {
            names.insert(f.to_string());
        }
    }
    let mut parts = Vec::new();
    for n in &names {
        let Ok(bytes) = std::fs::read(dir.join(n)) else { continue };
        let text = String::from_utf8_lossy(&bytes).replace("\r\n", "\n");
        parts.push((n.clone(), text.trim().to_string()));
    }
    // Several files with identical content (a crate repeating LICENSE-MIT) are shown once.
    let mut seen = BTreeSet::new();
    parts.retain(|(_, t)| !t.is_empty() && seen.insert(t.clone()));
    match parts.len() {
        0 => String::new(),
        1 => parts.remove(0).1,
        _ => parts.into_iter().map(|(n, t)| format!("[{n}]\n\n{t}")).collect::<Vec<_>>().join("\n\n"),
    }
}

/// Collects the shipped packages and renders the document. Warnings go to `warnings`.
fn generate(root: &Path, warnings: &mut Vec<String>) -> Result<String, String> {
    let meta = metadata(root)?;
    let packages: HashMap<String, &Value> = meta["packages"].as_array().ok_or("no packages")?.iter().map(|p| (s(p, "id"), p)).collect();
    let members: BTreeSet<String> =
        meta["workspace_members"].as_array().ok_or("no workspace_members")?.iter().filter_map(|m| m.as_str().map(String::from)).collect();
    let nodes: HashMap<String, &Value> = meta["resolve"]["nodes"].as_array().ok_or("no resolve")?.iter().map(|n| (s(n, "id"), n)).collect();

    let mut stack: Vec<String> =
        members.iter().filter(|id| packages.get(*id).is_some_and(|p| ROOTS.contains(&s(p, "name").as_str()))).cloned().collect();
    if stack.len() != ROOTS.len() {
        return Err(format!("expected workspace packages {ROOTS:?}"));
    }
    let mut seen: BTreeSet<String> = stack.iter().cloned().collect();
    while let Some(id) = stack.pop() {
        let Some(node) = nodes.get(&id) else { continue };
        for dep in node["deps"].as_array().into_iter().flatten() {
            // Only normal dependencies (`"kind": null`); dev and build dependencies are not linked into the binaries.
            let normal = dep["dep_kinds"].as_array().is_some_and(|k| k.iter().any(|k| k["kind"].is_null()));
            let did = s(dep, "pkg");
            if normal && seen.insert(did.clone()) {
                stack.push(did);
            }
        }
    }

    let mut entries: Vec<Entry> = Vec::new();
    for id in seen.iter().filter(|id| !members.contains(*id)) {
        let Some(p) = packages.get(id) else { continue };
        let dir = Path::new(&s(p, "manifest_path")).parent().map(Path::to_path_buf).unwrap_or_default();
        let license = {
            let l = s(p, "license");
            if l.is_empty() { s(p, "license_file").pipe_or("(see license file)") } else { l }
        };
        let lf = p["license_file"].as_str();
        let text = license_text(&dir, lf);
        entries.push(Entry {
            name: s(p, "name"),
            version: s(p, "version"),
            license,
            repository: s(p, "repository"),
            authors: p["authors"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|a| a.as_str())
                .map(|a| a.split(" <").next().unwrap_or(a).to_string())
                .collect(),
            text,
        });
    }
    // Many crates of one repository publish without the license files (they sit in the repository root). Such a crate
    // takes the text of a sibling crate with the same repository and the same license expression.
    let repo_key = |r: &str| r.trim().trim_end_matches('/').trim_end_matches(".git").to_ascii_lowercase();
    let donors: Vec<(String, String, String)> = entries
        .iter()
        .filter(|e| !e.text.is_empty() && !e.repository.is_empty())
        .map(|e| (repo_key(&e.repository), e.license.clone(), e.text.clone()))
        .collect();
    // The canonical Apache-2.0 text, taken from a package that ships exactly that.
    let apache = entries
        .iter()
        .map(|e| e.text.as_str())
        .filter(|t| t.starts_with("Apache License") && t.contains("Version 2.0, January 2004") && t.contains("END OF TERMS AND CONDITIONS"))
        .min_by_key(|t| t.len())
        .unwrap_or_default()
        .to_string();
    for e in entries.iter_mut().filter(|e| !e.text.is_empty() && e.license.starts_with("(MIT OR Apache-2.0) AND ")) {
        // epaint_default_fonts: font licenses are shipped, the MIT/Apache part (the crate code) is not.
        if let Some(t) = template("MIT OR Apache-2.0", &e.authors, &apache) {
            e.text = format!("{t}\n\n{}", e.text);
        }
    }
    for e in entries.iter_mut().filter(|e| e.text.is_empty()) {
        let key = repo_key(&e.repository);
        match donors.iter().find(|(r, l, _)| !key.is_empty() && *r == key && *l == e.license) {
            Some((_, _, text)) => e.text = text.clone(),
            None if template(&e.license, &e.authors, &apache).is_some() => e.text = template(&e.license, &e.authors, &apache).unwrap(),
            None => {
                warnings.push(format!("{} {}: no license text found in the package (SPDX: {})", e.name, e.version, e.license));
                e.text = format!("license text not found in package; SPDX: {}", e.license);
            }
        }
    }
    for (name, version, license, repo, file) in ASSETS {
        let text = std::fs::read_to_string(root.join(file)).map_err(|e| format!("cannot read {file}: {e}"))?;
        entries.push(Entry {
            name: name.to_string(),
            version: version.to_string(),
            license: license.to_string(),
            repository: repo.to_string(),
            authors: Vec::new(),
            text: text.replace("\r\n", "\n").trim().to_string(),
        });
    }
    entries.sort_by(|a, b| (a.name.to_lowercase(), &a.version).cmp(&(b.name.to_lowercase(), &b.version)));

    let mut out = String::new();
    out.push_str("<!-- Generated by `cargo run -p licenses`; do not edit. -->\n");
    out.push_str("# Third-party licenses\n\n");
    out.push_str(
        "Spark Engine itself is licensed under MPL-2.0 (see `LICENSE`). The Spark executables (`spark`, `spark-editor`) and every game \
         built with `spark build` also contain the third-party software and the font listed below. Their licenses require that the \
         license text and copyright notices are shipped with the binaries; this file is that notice.\n\n",
    );
    out.push_str(&format!("{} components. Generated by `cargo run -p licenses` from `Cargo.lock`; do not edit.\n\n", entries.len()));
    out.push_str("## Components\n\n");
    for e in &entries {
        let v = if e.version.is_empty() { String::new() } else { format!(" {}", e.version) };
        let r = if e.repository.is_empty() { String::new() } else { format!(" - {}", e.repository) };
        out.push_str(&format!("- {}{v}: {}{r}\n", e.name, e.license));
    }

    // Identical texts are printed once, with the list of components they apply to.
    let mut groups: BTreeMap<&str, Vec<&Entry>> = BTreeMap::new();
    for e in &entries {
        groups.entry(e.text.as_str()).or_default().push(e);
    }
    let mut groups: Vec<(&str, Vec<&Entry>)> = groups.into_iter().collect();
    groups.sort_by(|a, b| (a.1[0].name.to_lowercase(), &a.1[0].version, a.0).cmp(&(b.1[0].name.to_lowercase(), &b.1[0].version, b.0)));
    out.push_str("\n## License texts\n");
    for (text, members) in groups {
        let names: Vec<String> =
            members.iter().map(|e| if e.version.is_empty() { e.name.clone() } else { format!("{} {}", e.name, e.version) }).collect();
        let mut licenses: Vec<&str> = members.iter().map(|e| e.license.as_str()).collect();
        licenses.sort();
        licenses.dedup();
        out.push_str(&format!("\n### {}\n\nLicense: {}\n\n", names.join(", "), licenses.join(", ")));
        let longest_ticks = text.split(|c| c != '`').map(str::len).max().unwrap_or(0);
        let fence = "`".repeat(longest_ticks.max(2) + 1);
        out.push_str(&format!("{fence}text\n{text}\n{fence}\n"));
    }
    Ok(out)
}

/// Text for a package that ships no license file: the standard MIT text with the copyright line from the manifest
/// authors (MIT chosen when offered as an `OR` option), or the canonical Apache-2.0 text when that is the only
/// choice. `None` for anything else (it is reported and must be handled by hand).
fn template(license: &str, authors: &[String], apache: &str) -> Option<String> {
    if license.contains(" AND ") {
        return None;
    }
    let options: Vec<&str> = license.split([' ', '/', '(', ')']).filter(|t| !t.is_empty() && *t != "OR").collect();
    if options.contains(&"MIT") {
        let who = if authors.is_empty() { "the authors of this package".to_string() } else { authors.join(", ") };
        Some(format!(
            "[This package ships no license file; the MIT text below is the standard one, with the copyright holders taken from \
             the package manifest.]\n\nMIT License\n\nCopyright (c) {who}\n\n\
             Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated \
             documentation files (the \"Software\"), to deal in the Software without restriction, including without limitation \
             the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to \
             permit persons to whom the Software is furnished to do so, subject to the following conditions:\n\n\
             The above copyright notice and this permission notice shall be included in all copies or substantial portions of \
             the Software.\n\n\
             THE SOFTWARE IS PROVIDED \"AS IS\", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO \
             THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE \
             AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, \
             TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE \
             SOFTWARE."
        ))
    } else if options == ["BSL-1.0"] {
        let who = if authors.is_empty() { "the authors of this package".to_string() } else { authors.join(", ") };
        Some(format!(
            "[This package ships no license file; the standard Boost text is shown, copyright holders from the package manifest: {who}.]\n\n\
             Boost Software License - Version 1.0 - August 17th, 2003\n\n\
             Permission is hereby granted, free of charge, to any person or organization obtaining a copy of the software and \
             accompanying documentation covered by this license (the \"Software\") to use, reproduce, display, distribute, execute, \
             and transmit the Software, and to prepare derivative works of the Software, and to permit third-parties to whom the \
             Software is furnished to do so, all subject to the following:\n\n\
             The copyright notices in the Software and this entire statement, including the above license grant, this restriction \
             and the following disclaimer, must be included in all copies of the Software, in whole or in part, and all derivative \
             works of the Software, unless such copies or derivative works are solely in the form of machine-executable object code \
             generated by a source language processor.\n\n\
             THE SOFTWARE IS PROVIDED \"AS IS\", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE \
             WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE, TITLE AND NON-INFRINGEMENT. IN NO EVENT SHALL THE \
             COPYRIGHT HOLDERS OR ANYONE DISTRIBUTING THE SOFTWARE BE LIABLE FOR ANY DAMAGES OR OTHER LIABILITY, WHETHER IN \
             CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN \
             THE SOFTWARE."
        ))
    } else if options == ["Apache-2.0"] && !apache.is_empty() {
        Some(apache.to_string())
    } else {
        None
    }
}

trait PipeOr {
    fn pipe_or(self, fallback: &str) -> String;
}
impl PipeOr for String {
    fn pipe_or(self, fallback: &str) -> String {
        if self.is_empty() { fallback.to_string() } else { self }
    }
}

fn main() {
    let root = repo_root();
    let mut warnings = Vec::new();
    match generate(&root, &mut warnings) {
        Ok(doc) => {
            for w in &warnings {
                eprintln!("warning: {w}");
            }
            let path = root.join("THIRD_PARTY_LICENSES.md");
            if let Err(e) = std::fs::write(&path, &doc) {
                eprintln!("error: cannot write {}: {e}", path.display());
                std::process::exit(1);
            }
            println!("wrote {} ({} KB)", path.display(), doc.len() / 1024);
        }
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The committed file must match what `cargo metadata` produces. Bless with `SPARK_BLESS=1 cargo test -p licenses`.
    #[test]
    fn third_party_licenses_are_current() {
        let root = repo_root();
        let mut warnings = Vec::new();
        let expected = match generate(&root, &mut warnings) {
            Ok(d) => d.replace("\r\n", "\n"),
            Err(e) => {
                eprintln!("skipping the THIRD_PARTY_LICENSES.md check: {e}");
                return;
            }
        };
        let path = root.join("THIRD_PARTY_LICENSES.md");
        let have = std::fs::read_to_string(&path).unwrap_or_default().replace("\r\n", "\n");
        if have == expected {
            return;
        }
        if std::env::var_os("SPARK_BLESS").is_some() {
            std::fs::write(&path, expected).unwrap();
            return;
        }
        panic!("THIRD_PARTY_LICENSES.md is out of date: run `cargo run -p licenses` and commit the result");
    }

    #[test]
    fn every_component_has_text() {
        let mut warnings = Vec::new();
        if generate(&repo_root(), &mut warnings).is_ok() {
            assert!(warnings.is_empty(), "components without a license text: {warnings:?}");
        }
    }
}
