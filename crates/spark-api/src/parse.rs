//! Parser for the subset of Luau definition syntax that `docs/spark.d.luau` uses.

use crate::{Alias, Api, Class, Field, Function, Member, Namespace, Param, Value};

#[derive(Debug, Clone, PartialEq)]
pub struct ParseError {
    pub line: usize,
    pub message: String,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "spark.d.luau:{}: {}", self.line, self.message)
    }
}

impl std::error::Error for ParseError {}

fn err<T>(line: usize, message: impl Into<String>) -> Result<T, ParseError> {
    Err(ParseError { line, message: message.into() })
}

/// Splits `code -- comment` (quote aware: `"--"` is not a comment).
fn split_comment(line: &str) -> (&str, Option<&str>) {
    let b = line.as_bytes();
    let mut quote: Option<u8> = None;
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        match quote {
            Some(q) => {
                if c == b'\\' {
                    i += 1;
                } else if c == q {
                    quote = None;
                }
            }
            None => {
                if c == b'"' || c == b'\'' {
                    quote = Some(c);
                } else if c == b'-' && b.get(i + 1) == Some(&b'-') {
                    return (&line[..i], Some(line[i + 2..].trim()));
                }
            }
        }
        i += 1;
    }
    (line, None)
}

fn delta(c: char) -> i32 {
    match c {
        '(' | '{' | '[' => 1,
        ')' | '}' | ']' => -1,
        _ => 0,
    }
}

/// Bracket depth change of `s`, ignoring quoted text.
fn depth_change(s: &str) -> i32 {
    let mut quote: Option<char> = None;
    let mut d = 0;
    let mut prev = ' ';
    for c in s.chars() {
        match quote {
            Some(q) => {
                if c == q && prev != '\\' {
                    quote = None;
                }
            }
            None => {
                if c == '"' || c == '\'' {
                    quote = Some(c);
                } else {
                    d += delta(c);
                }
            }
        }
        prev = c;
    }
    d
}

/// Splits at commas that are not inside brackets or quotes.
fn split_top(s: &str, sep: char) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut depth = 0;
    let mut quote: Option<char> = None;
    let mut prev = ' ';
    for c in s.chars() {
        match quote {
            Some(q) => {
                cur.push(c);
                if c == q && prev != '\\' {
                    quote = None;
                }
            }
            None => {
                if c == '"' || c == '\'' {
                    quote = Some(c);
                    cur.push(c);
                } else if c == sep && depth == 0 {
                    out.push(std::mem::take(&mut cur));
                } else {
                    depth += delta(c);
                    cur.push(c);
                }
            }
        }
        prev = c;
    }
    if !cur.trim().is_empty() {
        out.push(cur);
    }
    out.into_iter().map(|p| p.trim().to_string()).filter(|p| !p.is_empty()).collect()
}

/// First top-level `:` in `s` (the one separating a name from its type).
fn split_name_type(s: &str) -> Option<(&str, &str)> {
    let mut depth = 0;
    let mut quote: Option<char> = None;
    for (i, c) in s.char_indices() {
        match quote {
            Some(q) => {
                if c == q {
                    quote = None;
                }
            }
            None => {
                if c == '"' || c == '\'' {
                    quote = Some(c);
                } else if c == ':' && depth == 0 {
                    return Some((s[..i].trim(), s[i + 1..].trim()));
                } else {
                    depth += delta(c);
                }
            }
        }
    }
    None
}

fn param(text: &str) -> Param {
    if text == "..." {
        return Param { name: String::new(), ty: "...".into(), optional: true };
    }
    match split_name_type(text) {
        Some((name, ty)) if !name.is_empty() && name.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '.') => {
            let optional = ty.ends_with('?') || name == "...";
            Param { name: name.to_string(), ty: ty.to_string(), optional }
        }
        _ => Param { name: String::new(), ty: text.to_string(), optional: text.ends_with('?') },
    }
}

/// `name<G>(params): ret` or `(params) -> ret` (when `name` is empty the text starts at `<` / `(`).
fn signature(name: &str, rest: &str, arrow: bool, line: usize) -> Result<Function, ParseError> {
    let rest = rest.trim();
    let (generics, rest) = if let Some(r) = rest.strip_prefix('<') {
        let end = r.find('>').map_or_else(|| err(line, "unclosed '<'"), Ok)?;
        (r[..end].trim().to_string(), r[end + 1..].trim_start())
    } else {
        (String::new(), rest)
    };
    let Some(body) = rest.strip_prefix('(') else { return err(line, format!("expected '(' in the signature of {name}")) };
    let mut depth = 1;
    let mut end = None;
    for (i, c) in body.char_indices() {
        depth += delta(c);
        if depth == 0 {
            end = Some(i);
            break;
        }
    }
    let Some(end) = end else { return err(line, format!("unclosed '(' in the signature of {name}")) };
    let mut params: Vec<Param> = split_top(&body[..end], ',').iter().map(|p| param(p)).collect();
    let after = body[end + 1..].trim();
    let ret = if arrow { after.strip_prefix("->").map(str::trim) } else { after.strip_prefix(':').map(str::trim) };
    let ret = match ret {
        Some(r) if !r.is_empty() => r.to_string(),
        _ if after.is_empty() => "()".to_string(),
        _ => return err(line, format!("bad return type in the signature of {name}: '{after}'")),
    };
    if params.first().is_some_and(|p| p.name.is_empty() && p.ty == "self") {
        params.remove(0);
    }
    Ok(Function { name: name.to_string(), generics, params, ret, doc: String::new() })
}

/// A `name: type` entry of a namespace / class body.
fn entry(text: &str, doc: String, line: usize) -> Result<Member, ParseError> {
    let Some((name, ty)) = split_name_type(text) else { return err(line, format!("expected `name: type`, got '{text}'")) };
    let name = name.to_string();
    let t = ty.trim_start();
    let is_fn = (t.starts_with('(') || t.starts_with('<')) && {
        // `(a: T) -> R` is a function, `(A | B)?` is a parenthesised type.
        let mut depth = 0;
        let mut close = None;
        for (i, c) in t.char_indices() {
            if c == '(' {
                depth += 1;
            } else if c == ')' {
                depth -= 1;
                if depth == 0 {
                    close = Some(i);
                    break;
                }
            }
        }
        close.is_some_and(|i| t[i + 1..].trim_start().starts_with("->")) || t.starts_with('<')
    };
    if is_fn {
        let mut f = signature(&name, t, true, line)?;
        f.doc = doc;
        Ok(Member::Function(f))
    } else {
        Ok(Member::Field(Field { name, ty: ty.to_string(), doc }))
    }
}

struct Lines<'a> {
    lines: Vec<&'a str>,
    at: usize,
}

pub fn parse(src: &str) -> Result<Api, ParseError> {
    let mut api = Api::default();
    let mut l = Lines { lines: src.lines().collect(), at: 0 };
    let mut doc: Vec<String> = Vec::new();
    while l.at < l.lines.len() {
        let n = l.at + 1;
        let raw = l.lines[l.at];
        l.at += 1;
        let (code, comment) = split_comment(raw);
        let code = code.trim_end();
        if code.trim().is_empty() {
            match comment {
                // Banner lines (`-- ====`) and blank lines end a doc block.
                Some(c) if c.starts_with("@generated") => {}
                Some(c) if !c.starts_with("====") && !c.is_empty() => doc.push(c.to_string()),
                _ => doc.clear(),
            }
            continue;
        }
        let take_doc = std::mem::take(&mut doc).join("\n");
        if let Some(rest) = code.strip_prefix("type ") {
            let Some((name, def)) = rest.split_once('=') else { return err(n, "expected `type Name = ...`") };
            let mut def = def.trim().to_string();
            let mut depth = depth_change(&def);
            // Continuation: open brackets, or a trailing `=` / `|`, or the next line starts with `|`.
            loop {
                let next = l.lines.get(l.at).map(|s| split_comment(s).0.trim_end());
                let cont = depth > 0
                    || def.is_empty()
                    || def.ends_with('|')
                    || next.is_some_and(|s| s.trim_start().starts_with('|') && s.starts_with(char::is_whitespace));
                if !cont {
                    break;
                }
                let Some(next) = next else { return err(n, format!("type {} is not closed", name.trim())) };
                l.at += 1;
                depth += depth_change(next);
                if !next.trim().is_empty() {
                    def.push(' ');
                    def.push_str(next.trim());
                }
            }
            api.aliases.push(Alias { name: name.trim().to_string(), def: def.trim().to_string(), doc: take_doc });
        } else if let Some(rest) = code.strip_prefix("declare extern type ") {
            let name = rest.trim().trim_end_matches("with").trim().to_string();
            let mut class = Class { name, fields: Vec::new(), methods: Vec::new(), doc: take_doc };
            let mut cdoc: Vec<String> = Vec::new();
            loop {
                let Some(raw) = l.lines.get(l.at).copied() else {
                    return err(n, format!("`declare extern type {}` has no `end`", class.name));
                };
                let ln = l.at + 1;
                l.at += 1;
                let (code, comment) = split_comment(raw);
                let code = code.trim();
                if code.is_empty() {
                    match comment {
                        Some(c) if c.starts_with("@generated") => {}
                        Some(c) if !c.is_empty() => cdoc.push(c.to_string()),
                        _ => cdoc.clear(),
                    }
                    continue;
                }
                if code == "end" {
                    break;
                }
                let mut d = std::mem::take(&mut cdoc).join("\n");
                if let Some(c) = comment.filter(|c| !c.is_empty()) {
                    if !d.is_empty() {
                        d.push('\n');
                    }
                    d.push_str(c);
                }
                if let Some(rest) = code.strip_prefix("function ") {
                    let open = rest.find(['(', '<']).map_or_else(|| err(ln, "expected '(' after the method name"), Ok)?;
                    let mut f = signature(rest[..open].trim(), &rest[open..], false, ln)?;
                    f.doc = d;
                    class.methods.push(f);
                } else {
                    match entry(code, d, ln)? {
                        Member::Field(f) => class.fields.push(f),
                        Member::Function(f) => class.methods.push(f),
                    }
                }
            }
            api.classes.push(class);
        } else if let Some(rest) = code.strip_prefix("declare function ") {
            let open = rest.find(['(', '<']).map_or_else(|| err(n, "expected '(' after the function name"), Ok)?;
            let mut f = signature(rest[..open].trim(), &rest[open..], false, n)?;
            f.doc = take_doc;
            api.functions.push(f);
        } else if let Some(rest) = code.strip_prefix("declare ") {
            let Some((name, ty)) = split_name_type(rest) else { return err(n, "expected `declare name: type`") };
            if ty.trim() == "{" {
                let mut members: Vec<Member> = Vec::new();
                let mut cur = String::new();
                let mut cur_doc = String::new();
                let mut pending: Vec<String> = Vec::new();
                let mut depth = 0i32;
                let mut closed = false;
                while let Some(raw) = l.lines.get(l.at).copied() {
                    let ln = l.at + 1;
                    l.at += 1;
                    let (code, comment) = split_comment(raw);
                    let trimmed = code.trim();
                    if trimmed.is_empty() && cur.is_empty() {
                        match comment {
                            Some(c) if !c.is_empty() => pending.push(c.to_string()),
                            _ => pending.clear(),
                        }
                        continue;
                    }
                    let mut line_text = String::new();
                    for ch in trimmed.chars() {
                        let d = delta(ch);
                        if depth == 0 && d < 0 {
                            closed = true;
                            break;
                        }
                        if depth == 0 && ch == ',' {
                            if !cur.trim().is_empty() {
                                members.push(entry(cur.trim(), std::mem::take(&mut cur_doc), ln)?);
                            }
                            cur.clear();
                            continue;
                        }
                        if cur.trim().is_empty() && !ch.is_whitespace() {
                            cur_doc = std::mem::take(&mut pending).join("\n");
                        }
                        depth += d;
                        cur.push(ch);
                        line_text.push(ch);
                    }
                    cur.push(' ');
                    if closed {
                        if !cur.trim().is_empty() {
                            members.push(entry(cur.trim(), std::mem::take(&mut cur_doc), ln)?);
                        }
                        break;
                    }
                    if let Some(c) = comment.filter(|c| !c.is_empty()) {
                        // A trailing comment belongs to the entry on this line.
                        let target = if cur.trim().is_empty() { members.last_mut().map(m_doc) } else { Some(&mut cur_doc) };
                        if let Some(d) = target {
                            if !d.is_empty() {
                                d.push('\n');
                            }
                            d.push_str(c);
                        }
                    }
                }
                if !closed {
                    return err(n, format!("`declare {}: {{` is not closed with `}}`", name));
                }
                api.namespaces.push(Namespace { name: name.to_string(), members, doc: take_doc });
            } else {
                api.values.push(Value { name: name.to_string(), ty: ty.trim().to_string(), doc: take_doc });
            }
        } else {
            return err(n, format!("unrecognised line: '{}'", code.trim()));
        }
    }
    Ok(api)
}

fn m_doc(m: &mut Member) -> &mut String {
    match m {
        Member::Function(f) => &mut f.doc,
        Member::Field(f) => &mut f.doc,
    }
}
