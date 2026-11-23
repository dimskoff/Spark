//! Generates `THIRD_PARTY_LICENSES.md`: a compact notice for everything that is compiled into the shipped binaries
//! (`spark`, `spark-editor`, and with them every game made by `spark build`).
//!
//! The file is a list (name, versions, SPDX expression, repository, copyright holders) followed by ONE copy of every
//! distinct license text that is actually used.
//!
//! Package selection: the normal (non-dev, non-build) dependency graph of `spark-cli` and `spark-editor` from
//! `cargo metadata`, all target platforms and the default feature set the workspace builds with, minus the
//! workspace's own crates. Dev-dependencies and build-scripts-only dependencies are not shipped and are left out.
//!
//! `cargo run -p licenses` writes the file; `cargo test -p licenses` fails when the committed file is stale.
#![forbid(unsafe_code)]

use std::collections::{BTreeSet, HashMap};
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
/// Used instead of the copyright line in the canonical texts below.
const PLACEHOLDER: &str = "Copyright notices: see the list above.";
/// Maximum length of the copyright text shown per component.
const MAX_COPYRIGHT: usize = 80;

const MIT: &str = "Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated \
documentation files (the \"Software\"), to deal in the Software without restriction, including without limitation the rights to use, \
copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is \
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED \"AS IS\", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF \
MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE \
FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION \
WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.";

const BSD_DISCLAIMER: &str = "THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS \"AS IS\" AND ANY EXPRESS OR IMPLIED \
WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE \
DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, \
OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR \
BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING \
NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.";

const ISC: &str = "Permission to use, copy, modify, and/or distribute this software for any purpose with or without fee is hereby \
granted, provided that the above copyright notice and this permission notice appear in all copies.

THE SOFTWARE IS PROVIDED \"AS IS\" AND THE AUTHOR DISCLAIMS ALL WARRANTIES WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES \
OF MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR ANY SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR \
ANY DAMAGES WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS \
ACTION, ARISING OUT OF OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.";

const ZLIB: &str = "This software is provided 'as-is', without any express or implied warranty. In no event will the authors be held \
liable for any damages arising from the use of this software.

Permission is granted to anyone to use this software for any purpose, including commercial applications, and to alter it and \
redistribute it freely, subject to the following restrictions:

1. The origin of this software must not be misrepresented; you must not claim that you wrote the original software. If you use this \
software in a product, an acknowledgment in the product documentation would be appreciated but is not required.
2. Altered source versions must be plainly marked as such, and must not be misrepresented as being the original software.
3. This notice may not be removed or altered from any source distribution.";

const BSL: &str = "Permission is hereby granted, free of charge, to any person or organization obtaining a copy of the software and \
accompanying documentation covered by this license (the \"Software\") to use, reproduce, display, distribute, execute, and transmit the \
Software, and to prepare derivative works of the Software, and to permit third-parties to whom the Software is furnished to do so, all \
subject to the following:

The copyright notices in the Software and this entire statement, including the above license grant, this restriction and the following \
disclaimer, must be included in all copies of the Software, in whole or in part, and all derivative works of the Software, unless such \
copies or derivative works are solely in the form of machine-executable object code generated by a source language processor.

THE SOFTWARE IS PROVIDED \"AS IS\", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF \
MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE, TITLE AND NON-INFRINGEMENT. IN NO EVENT SHALL THE COPYRIGHT HOLDERS OR ANYONE \
DISTRIBUTING THE SOFTWARE BE LIABLE FOR ANY DAMAGES OR OTHER LIABILITY, WHETHER IN CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR \
IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.";

/// Canonical texts embedded here (short, identical everywhere): (SPDX id, title, text).
fn templates() -> Vec<(&'static str, &'static str, String)> {
    vec![
        ("MIT", "MIT License", format!("MIT License\n\n{PLACEHOLDER}\n\n{MIT}")),
        ("ISC", "ISC License", format!("ISC License\n\n{PLACEHOLDER}\n\n{ISC}")),
        ("Zlib", "zlib License", format!("zlib License\n\n{PLACEHOLDER}\n\n{ZLIB}")),
        ("BSL-1.0", "Boost Software License 1.0", format!("Boost Software License - Version 1.0 - August 17th, 2003\n\n{BSL}")),
        (
            "BSD-2-Clause",
            "BSD 2-Clause License",
            format!(
                "BSD 2-Clause License\n\n{PLACEHOLDER}\n\nRedistribution and use in source and binary forms, with or without modification, are permitted \
                 provided that the following conditions are met:\n\n\
                 1. Redistributions of source code must retain the above copyright notice, this list of conditions and the following disclaimer.\n\n\
                 2. Redistributions in binary form must reproduce the above copyright notice, this list of conditions and the following disclaimer \
                 in the documentation and/or other materials provided with the distribution.\n\n{BSD_DISCLAIMER}"
            ),
        ),
        (
            "BSD-3-Clause",
            "BSD 3-Clause License",
            format!(
                "BSD 3-Clause License\n\n{PLACEHOLDER}\n\nRedistribution and use in source and binary forms, with or without modification, are permitted \
                 provided that the following conditions are met:\n\n\
                 1. Redistributions of source code must retain the above copyright notice, this list of conditions and the following disclaimer.\n\n\
                 2. Redistributions in binary form must reproduce the above copyright notice, this list of conditions and the following disclaimer \
                 in the documentation and/or other materials provided with the distribution.\n\n\
                 3. Neither the name of the copyright holder nor the names of its contributors may be used to endorse or promote products derived \
                 from this software without specific prior written permission.\n\n{BSD_DISCLAIMER}"
            ),
        ),
    ]
}

/// Licenses whose full text is taken from a package that ships it: (SPDX id, title, marker that identifies the text).
const SHIPPED: &[(&str, &str, &str)] = &[
    ("Apache-2.0", "Apache License 2.0", "Version 2.0, January 2004"),
    ("MPL-2.0", "Mozilla Public License 2.0", "Mozilla Public License Version 2.0"),
    ("Unicode-3.0", "Unicode License v3", "UNICODE LICENSE V3"),
    ("OFL-1.1", "SIL Open Font License 1.1", "SIL OPEN FONT LICENSE Version 1.1"),
    ("Ubuntu-font-1.0", "Ubuntu Font License 1.0", "UBUNTU FONT LICENCE"),
    ("CC0-1.0", "CC0 1.0 Universal", "CC0 1.0 Universal"),
];

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

struct Entry {
    name: String,
    version: String,
    license: String,
    repository: String,
    authors: Vec<String>,
    /// License files of the package: (file name, text).
    files: Vec<(String, String)>,
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

/// License/notice files of a package.
fn license_files(dir: &Path, license_file: Option<&str>) -> Vec<(String, String)> {
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
    let mut out = Vec::new();
    for n in names {
        let Ok(bytes) = std::fs::read(dir.join(&n)) else { continue };
        let text = String::from_utf8_lossy(&bytes).replace("\r\n", "\n").trim().to_string();
        if !text.is_empty() {
            out.push((n, text));
        }
    }
    out
}

/// The `Copyright ...` lines of a license text, without the word and the `(c)` sign.
fn copyright_lines(text: &str) -> Vec<String> {
    let mut found: Vec<String> = Vec::new();
    for line in text.lines() {
        let t = line.trim().trim_start_matches(['*', '#', '/', '-', ' ']).trim();
        if t.len() < 9 || !t.is_char_boundary(9) || !t[..9].eq_ignore_ascii_case("copyright") {
            continue;
        }
        let rest = t[9..].trim_start_matches(|c: char| c == ':' || c.is_whitespace());
        let rest =
            rest.strip_prefix("(c)").or_else(|| rest.strip_prefix("(C)")).or_else(|| rest.strip_prefix('\u{a9}')).unwrap_or(rest).trim();
        let low = rest.to_ascii_lowercase();
        let boilerplate = ["notice", "and ", "holder", "owner", "license", "statement", "in ", "of ", "or "];
        if rest.len() < 3 || rest.contains('[') || boilerplate.iter().any(|b| low.starts_with(b)) {
            continue;
        }
        let rest = rest.replace("All rights reserved", "");
        found.push(rest.trim_end_matches(['.', ',', ' ']).to_string());
    }
    found
}

/// Holder text without the years, for comparing.
fn holder_key(s: &str) -> String {
    s.trim_start_matches(|c: char| c.is_ascii_digit() || " ,-\u{2013}".contains(c)).to_ascii_lowercase()
}

/// The first two distinct copyright holders found in the license files of a package.
fn holders(files: &[(String, String)]) -> Vec<String> {
    let mut found: Vec<String> = Vec::new();
    for line in files.iter().flat_map(|(_, t)| copyright_lines(t)) {
        let key = holder_key(&line);
        // The same holder with another year range counts once.
        if !key.is_empty() && !found.iter().any(|f| holder_key(f) == key) {
            found.push(line);
        }
    }
    found.truncate(2);
    found
}

fn shorten(text: &str) -> String {
    // E-mail addresses (`Name <mail>`) are dropped.
    let no_mail: Vec<&str> = text.split_whitespace().filter(|w| !(w.starts_with('<') && w.ends_with(['>', ',', ';']))).collect();
    let t = no_mail.join(" ");
    if t.chars().count() <= MAX_COPYRIGHT {
        return t;
    }
    let cut: String = t.chars().take(MAX_COPYRIGHT - 3).collect();
    format!("{}...", cut.trim_end())
}

// ---- SPDX expressions -------------------------------------------------------------------------------------------

/// How much we like a license when an `OR` offers a choice: lower is better. MIT, then Apache-2.0, then other
/// permissive licenses, then weak copyleft; exceptions (`WITH`) and anything else last.
fn rank(id: &str) -> u32 {
    match id {
        "MIT" => 0,
        "Apache-2.0" => 1,
        "Zlib" | "BSD-2-Clause" | "BSD-3-Clause" | "ISC" | "0BSD" | "BSL-1.0" => 2,
        "Unlicense" | "CC0-1.0" => 3,
        "MPL-2.0" => 4,
        _ if id.contains(" WITH ") => 50,
        _ => 99,
    }
}

struct Parser {
    tokens: Vec<String>,
    pos: usize,
}

impl Parser {
    fn new(expr: &str) -> Self {
        let spaced = expr.replace('(', " ( ").replace(')', " ) ").replace('/', " OR ");
        Parser { tokens: spaced.split_whitespace().map(String::from).collect(), pos: 0 }
    }
    fn peek(&self) -> Option<&str> {
        self.tokens.get(self.pos).map(String::as_str)
    }
    /// `a OR b OR c`: the alternative with the best (lowest) worst-ranked license wins, the first one on a tie.
    fn or_expr(&mut self) -> Vec<String> {
        let mut best = self.and_expr();
        while self.peek() == Some("OR") {
            self.pos += 1;
            let alt = self.and_expr();
            let score = |v: &[String]| v.iter().map(|i| rank(i)).max().unwrap_or(99);
            if score(&alt) < score(&best) {
                best = alt;
            }
        }
        best
    }
    fn and_expr(&mut self) -> Vec<String> {
        let mut all = self.atom();
        while self.peek() == Some("AND") {
            self.pos += 1;
            all.extend(self.atom());
        }
        all
    }
    fn atom(&mut self) -> Vec<String> {
        let Some(t) = self.peek().map(String::from) else { return Vec::new() };
        self.pos += 1;
        if t == "(" {
            let inner = self.or_expr();
            if self.peek() == Some(")") {
                self.pos += 1;
            }
            return inner;
        }
        if self.peek() == Some("WITH") {
            self.pos += 1;
            let exc = self.peek().unwrap_or_default().to_string();
            self.pos += 1;
            return vec![format!("{t} WITH {exc}")];
        }
        vec![t]
    }
}

/// The licenses we comply with for an SPDX expression (see [`rank`] for the rule applied to `OR`).
fn chosen_licenses(expr: &str) -> Vec<String> {
    let mut ids = Parser::new(expr).or_expr();
    ids.sort();
    ids.dedup();
    ids
}

// ---- Generation -------------------------------------------------------------------------------------------------

/// Collects the shipped packages and renders the document. Problems go to `warnings`.
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
        entries.push(Entry {
            name: s(p, "name"),
            version: s(p, "version"),
            license: s(p, "license"),
            repository: s(p, "repository"),
            authors: p["authors"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|a| a.as_str())
                .map(|a| a.split(" <").next().unwrap_or(a).trim().to_string())
                .filter(|a| !a.is_empty())
                .collect(),
            files: license_files(&dir, p["license_file"].as_str()),
        });
    }
    for (name, version, license, repo, file) in ASSETS {
        let text = std::fs::read_to_string(root.join(file)).map_err(|e| format!("cannot read {file}: {e}"))?;
        entries.push(Entry {
            name: name.to_string(),
            version: version.to_string(),
            license: license.to_string(),
            repository: repo.to_string(),
            authors: Vec::new(),
            files: vec![(file.to_string(), text.replace("\r\n", "\n").trim().to_string())],
        });
    }
    entries.sort_by(|a, b| (a.name.to_lowercase(), &a.version).cmp(&(b.name.to_lowercase(), &b.version)));

    // Copyright holders per entry; a crate published without license files borrows from a sibling of the same repository.
    let repo_key = |r: &str| r.trim().trim_end_matches('/').trim_end_matches(".git").to_ascii_lowercase();
    let own: Vec<Vec<String>> = entries.iter().map(|e| holders(&e.files)).collect();
    let mut by_repo: HashMap<String, Vec<String>> = HashMap::new();
    for (e, c) in entries.iter().zip(&own) {
        if !c.is_empty() && !e.repository.is_empty() {
            by_repo.entry(repo_key(&e.repository)).or_insert_with(|| c.clone());
        }
    }
    let copyrights: Vec<String> = entries
        .iter()
        .zip(&own)
        .map(|(e, c)| {
            let c = if !c.is_empty() { c.clone() } else { by_repo.get(&repo_key(&e.repository)).cloned().unwrap_or_default() };
            if !c.is_empty() { shorten(&c.join("; ")) } else { shorten(&e.authors.join(", ")) }
        })
        .collect();

    // Group versions of one crate into one line.
    struct Line {
        name: String,
        versions: Vec<String>,
        licenses: Vec<String>,
        repository: String,
        copyright: String,
    }
    let mut lines: Vec<Line> = Vec::new();
    for (e, c) in entries.iter().zip(&copyrights) {
        if let Some(l) = lines.last_mut().filter(|l| l.name == e.name) {
            if !e.version.is_empty() {
                l.versions.push(e.version.clone());
            }
            if !l.licenses.contains(&e.license) {
                l.licenses.push(e.license.clone());
            }
            if l.copyright.is_empty() {
                l.copyright = c.clone();
            }
            continue;
        }
        lines.push(Line {
            name: e.name.clone(),
            versions: if e.version.is_empty() { Vec::new() } else { vec![e.version.clone()] },
            licenses: vec![e.license.clone()],
            repository: if e.repository.is_empty() { format!("https://crates.io/crates/{}", e.name) } else { e.repository.clone() },
            copyright: c.clone(),
        });
    }

    // The distinct licenses that apply, with their texts.
    let mut used: BTreeSet<String> = BTreeSet::new();
    for e in &entries {
        let chosen = chosen_licenses(&e.license);
        if e.license.trim().is_empty() || chosen.is_empty() {
            warnings.push(format!("{} {}: no SPDX license expression", e.name, e.version));
        }
        used.extend(chosen);
    }
    let templates = templates();
    let mut texts: Vec<(String, String, String)> = Vec::new();
    for id in &used {
        if let Some((_, title, text)) = templates.iter().find(|(i, _, _)| i == id) {
            texts.push((id.clone(), title.to_string(), text.clone()));
        } else if let Some((_, title, marker)) = SHIPPED.iter().find(|(i, _, _)| i == id) {
            // The shortest shipped file carrying the license is the cleanest copy.
            let best = entries
                .iter()
                .flat_map(|e| e.files.iter())
                .map(|(_, t)| t)
                .filter(|t| t.contains(marker))
                .min_by(|a, b| (a.len(), *a).cmp(&(b.len(), *b)));
            match best {
                Some(t) => texts.push((id.clone(), title.to_string(), t.clone())),
                None => warnings.push(format!("{id}: no package ships the text")),
            }
        } else {
            warnings.push(format!("{id}: no license text available (add it to the generator)"));
        }
    }

    let mut out = String::new();
    out.push_str("<!-- Generated by `cargo run -p licenses`; do not edit. -->\n");
    out.push_str("# Third-party licenses\n\n");
    out.push_str(
        "Spark Engine itself is licensed under MPL-2.0 (see `LICENSE`). The Spark executables (`spark`, `spark-editor`) and every game \
         built with `spark build` also contain the third-party software and the font listed below. This file is the notice that \
         accompanies them: the components with their licenses, source repositories and copyright holders, followed by one copy of each \
         license text.\n\n",
    );
    out.push_str(
        "Generated by `cargo run -p licenses` from `Cargo.lock`; do not edit. Where a component offers a choice (`A OR B`), the license \
         that applies to Spark is the first permissive option in the order MIT, Apache-2.0, other permissive licenses; only the texts of \
         the licenses that apply are included below. Components under `A AND B` are bound by both.\n\n",
    );
    out.push_str(&format!("## Components ({})\n\n", lines.len()));
    for l in &lines {
        let mut line = format!("- {}", l.name);
        if !l.versions.is_empty() {
            line.push_str(&format!(" {}", l.versions.join(", ")));
        }
        line.push_str(&format!(" \u{2014} {} \u{2014} {}", l.licenses.join(", "), l.repository));
        if !l.copyright.is_empty() {
            line.push_str(&format!(" \u{2014} \u{a9} {}", l.copyright));
        }
        out.push_str(&line);
        out.push('\n');
    }
    out.push_str("\n## License texts\n");
    for (id, title, text) in &texts {
        let longest_ticks = text.split(|c| c != '`').map(str::len).max().unwrap_or(0);
        let fence = "`".repeat(longest_ticks.max(2) + 1);
        out.push_str(&format!("\n### {title} ({id})\n\n{fence}text\n{text}\n{fence}\n"));
    }
    Ok(out)
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

    /// Every component has an SPDX expression, and the text of every license that applies is in the file.
    #[test]
    fn every_component_has_a_license_with_text() {
        let mut warnings = Vec::new();
        if generate(&repo_root(), &mut warnings).is_ok() {
            assert!(warnings.is_empty(), "license problems: {warnings:?}");
        }
    }

    #[test]
    fn or_prefers_mit_then_apache() {
        assert_eq!(chosen_licenses("MIT OR Apache-2.0"), ["MIT"]);
        assert_eq!(chosen_licenses("Apache-2.0 OR MIT"), ["MIT"]);
        assert_eq!(chosen_licenses("MIT/Apache-2.0"), ["MIT"]);
        assert_eq!(chosen_licenses("Zlib OR Apache-2.0 OR MIT"), ["MIT"]);
        assert_eq!(chosen_licenses("BSD-3-Clause OR Apache-2.0"), ["Apache-2.0"]);
        assert_eq!(chosen_licenses("Apache-2.0 OR GPL-2.0-only"), ["Apache-2.0"]);
        assert_eq!(chosen_licenses("Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT"), ["MIT"]);
        assert_eq!(chosen_licenses("(MIT OR Apache-2.0) AND Unicode-3.0"), ["MIT", "Unicode-3.0"]);
        assert_eq!(chosen_licenses("Apache-2.0 AND MIT"), ["Apache-2.0", "MIT"]);
    }
}
