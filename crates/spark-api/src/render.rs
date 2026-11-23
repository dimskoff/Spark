//! Generated outputs: Markdown reference, llms.txt, search.

use std::fmt::Write;

use serde::Serialize;

use crate::{Api, Function, Member};

fn doc_block(out: &mut String, doc: &str) {
    if !doc.is_empty() {
        let _ = writeln!(out, "{}\n", doc.replace('\n', " "));
    }
}

fn lower_first(s: &str) -> String {
    let mut c = s.chars();
    c.next().map(|f| f.to_lowercase().collect::<String>() + c.as_str()).unwrap_or_default()
}

/// `docs/API_REFERENCE.md`: every global, table and class with its signature.
pub fn markdown(api: &Api, version: &str) -> String {
    let mut o = String::new();
    let _ = writeln!(o, "# Spark Luau API reference\n");
    let _ = writeln!(o, "Generated from `docs/spark.d.luau` for Spark {version}. Do not edit by hand: edit the definition file and run");
    let _ = writeln!(o, "`SPARK_BLESS=1 cargo test -p spark-api`. Narrative guides, units and conventions are in `SPARK_API.md`.\n");
    let _ = writeln!(o, "## Global functions\n");
    for f in &api.functions {
        let _ = writeln!(o, "### `{}`\n\n```luau\n{}\n```\n", f.name, f.signature(""));
        doc_block(&mut o, &f.doc);
    }
    if !api.values.is_empty() {
        let _ = writeln!(o, "## Global values\n");
        for v in &api.values {
            let _ = writeln!(
                o,
                "- `{}: {}`{}",
                v.name,
                v.ty,
                if v.doc.is_empty() { String::new() } else { format!(" - {}", v.doc.replace('\n', " ")) }
            );
        }
        o.push('\n');
    }
    let _ = writeln!(o, "## Tables\n");
    for ns in &api.namespaces {
        let _ = writeln!(o, "### `{}`\n", ns.name);
        doc_block(&mut o, &ns.doc);
        let _ = writeln!(o, "```luau");
        for m in &ns.members {
            match m {
                Member::Function(f) => {
                    let _ = writeln!(o, "{}", f.signature(&format!("{}.", ns.name)));
                }
                Member::Field(f) => {
                    let _ = writeln!(o, "{}.{}: {}", ns.name, f.name, f.ty);
                }
            }
        }
        let _ = writeln!(o, "```\n");
        for m in &ns.members {
            if !m.doc().is_empty() {
                let _ = writeln!(o, "- `{}`: {}", m.name(), m.doc().replace('\n', " "));
            }
        }
        if ns.members.iter().any(|m| !m.doc().is_empty()) {
            o.push('\n');
        }
    }
    let _ = writeln!(o, "## Classes\n");
    for c in &api.classes {
        let var = lower_first(&c.name);
        let _ = writeln!(o, "### `{}`\n", c.name);
        doc_block(&mut o, &c.doc);
        let _ = writeln!(o, "```luau");
        for f in &c.fields {
            let _ = writeln!(o, "{var}.{}: {}", f.name, f.ty);
        }
        for m in &c.methods {
            let _ = writeln!(o, "{}", m.signature(&format!("{var}:")));
        }
        let _ = writeln!(o, "```\n");
        let docs: Vec<String> = c
            .fields
            .iter()
            .map(|f| (f.name.as_str(), f.doc.as_str()))
            .chain(c.methods.iter().map(|m| (m.name.as_str(), m.doc.as_str())))
            .filter(|(_, d)| !d.is_empty())
            .map(|(n, d)| format!("- `{n}`: {}", d.replace('\n', " ")))
            .collect();
        if !docs.is_empty() {
            let _ = writeln!(o, "{}\n", docs.join("\n"));
        }
    }
    let _ = writeln!(o, "## Types\n");
    for a in &api.aliases {
        let _ = writeln!(o, "### `{}`\n\n```luau\ntype {} = {}\n```\n", a.name, a.name, a.def);
        doc_block(&mut o, &a.doc);
    }
    o
}

/// `llms.txt`: the index an LLM reads first (https://llmstxt.org).
pub fn llms_txt(version: &str) -> String {
    format!(
        "# Spark\n\n\
> Spark {version} is a code-first game and app engine: a Rust + wgpu core scripted in Luau, one native executable per game. \
It is built so that an AI agent can write, run, inspect and test a game without a human: every CLI command has `--json`, \
errors carry `file:line` and a fix hint, and runs are headless and deterministic.\n\n\
Conventions: Y-up, right-handed, 1 unit = 1 meter, objects look along -Z, angles in radians (camera.fov in degrees), colors sRGB, \
paths relative to the game folder. Numbers must be finite; asset paths cannot leave the game folder.\n\n\
## Docs\n\n\
- [Guide and concepts](docs/SPARK_API.md): game structure, frame order, objects, physics, UI, audio, saving, testing\n\
- [API reference](docs/API_REFERENCE.md): every function, table and class with its signature\n\
- [Type definitions](docs/spark.d.luau): luau-lsp definitions (`spark check --types`)\n\
- [Shaders](docs/SPARK_SHADERS.md): WGSL surface and post shaders\n\
- [Machine-readable API](docs/api.json): the registry as JSON\n\n\
## Working loop\n\n\
1. `spark new mygame` (templates: 3d, 2d, empty)\n\
2. Write `main.luau` and `src/*.luau`\n\
3. `spark check mygame --json` finds script errors without a window\n\
4. `spark test mygame --json` runs scenario tests in `tests/*.luau`\n\
5. `spark mygame --headless --frames 120 --screenshot shot.png` then look at the picture\n\
6. `spark api search <word>` looks a function up in the API\n\n\
"
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Function,
    Method,
    Field,
    Table,
    Class,
    Type,
}

#[derive(Debug, Clone, Serialize)]
pub struct Hit {
    pub kind: Kind,
    pub name: String,
    pub signature: String,
    pub doc: String,
    pub score: u32,
}

fn score(query: &[String], name: &str, owner: &str, doc: &str) -> u32 {
    let name_l = name.to_lowercase();
    let owner_l = owner.to_lowercase();
    let doc_l = doc.to_lowercase();
    let mut total = 0;
    for q in query {
        let s = if name_l == *q {
            100
        } else if name_l.starts_with(q.as_str()) {
            60
        } else if name_l.contains(q.as_str()) {
            40
        } else if owner_l == *q {
            30
        } else if doc_l.contains(q.as_str()) {
            10
        } else {
            return 0; // every word must match somewhere
        };
        total += s;
    }
    total
}

/// Words of `query` (any case) against names, owners and doc comments. Best hits first.
pub fn search(api: &Api, query: &str, limit: usize) -> Vec<Hit> {
    let words: Vec<String> =
        query.split(|c: char| !c.is_alphanumeric() && c != '_').filter(|w| !w.is_empty()).map(str::to_lowercase).collect();
    if words.is_empty() {
        return Vec::new();
    }
    let mut hits = Vec::new();
    let mut push = |kind, name: &str, owner: &str, sig: String, doc: &str| {
        let s = score(&words, name, owner, doc);
        if s > 0 {
            let full = if owner.is_empty() { name.to_string() } else { format!("{owner}.{name}") };
            hits.push(Hit { kind, name: full, signature: sig, doc: doc.replace('\n', " "), score: s });
        }
    };
    let fsig = |f: &Function, p: &str| f.signature(p);
    for f in &api.functions {
        push(Kind::Function, &f.name, "", fsig(f, ""), &f.doc);
    }
    for ns in &api.namespaces {
        push(Kind::Table, &ns.name, "", format!("{} (table)", ns.name), &ns.doc);
        for m in &ns.members {
            match m {
                Member::Function(f) => push(Kind::Function, &f.name, &ns.name, fsig(f, &format!("{}.", ns.name)), &f.doc),
                Member::Field(f) => push(Kind::Field, &f.name, &ns.name, format!("{}.{}: {}", ns.name, f.name, f.ty), &f.doc),
            }
        }
    }
    for c in &api.classes {
        push(Kind::Class, &c.name, "", format!("{} (class)", c.name), &c.doc);
        let var = lower_first(&c.name);
        for f in &c.fields {
            push(Kind::Field, &f.name, &c.name, format!("{var}.{}: {}", f.name, f.ty), &f.doc);
        }
        for m in &c.methods {
            push(Kind::Method, &m.name, &c.name, fsig(m, &format!("{var}:")), &m.doc);
        }
    }
    for t in &api.aliases {
        push(Kind::Type, &t.name, "", format!("type {} = {}", t.name, t.def), &t.doc);
    }
    for v in &api.values {
        push(Kind::Field, &v.name, "", format!("{}: {}", v.name, v.ty), &v.doc);
    }
    hits.sort_by(|a, b| b.score.cmp(&a.score).then_with(|| a.name.cmp(&b.name)));
    hits.truncate(limit);
    hits
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_finds_by_name_owner_and_doc() {
        let api = crate::api();
        let h = search(&api, "spawn", 5);
        assert_eq!(h[0].name, "spawn");
        let h = search(&api, "rotate_y", 3);
        assert!(h[0].name.ends_with("rotate_y"), "{h:?}");
        let h = search(&api, "gravity", 10);
        assert!(!h.is_empty());
        assert!(search(&api, "zzzzqqqq", 5).is_empty());
        assert!(search(&api, "   ", 5).is_empty());
    }

    #[test]
    fn reference_lists_everything() {
        let api = crate::api();
        let md = markdown(&api, "0.0.0");
        for f in &api.functions {
            assert!(md.contains(&format!("### `{}`", f.name)), "{}", f.name);
        }
        assert!(md.contains("Mesh.cube(size: number?) -> Mesh"));
        assert!(md.contains("object:rotate_x("));
    }
}
