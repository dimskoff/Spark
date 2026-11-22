//! The raw TOML tree the reader works on: values with the byte span they were parsed from (for line numbers).
//!
//! A tree parsed from text has spans; a tree made from a migrated `toml::Table` has none.

use std::ops::Range;

use toml::de::{DeTable, DeValue};
use toml::{Spanned, Table, Value};

use crate::error::{ErrorKind, SceneError};

pub(crate) type Span = Option<Range<usize>>;

#[derive(Clone, Debug)]
pub(crate) enum Tv {
    Str(String),
    Int(i64),
    Float(f64),
    Bool(bool),
    Datetime(String),
    Array(Vec<N>),
    Table(Vec<Entry>),
}

#[derive(Clone, Debug)]
pub(crate) struct Entry {
    pub key: String,
    pub key_span: Span,
    pub val: N,
}

#[derive(Clone, Debug)]
pub(crate) struct N {
    pub v: Tv,
    pub span: Span,
}

impl N {
    pub fn type_name(&self) -> &'static str {
        match self.v {
            Tv::Str(_) => "a string",
            Tv::Int(_) => "an integer",
            Tv::Float(_) => "a float",
            Tv::Bool(_) => "a boolean",
            Tv::Datetime(_) => "a date",
            Tv::Array(_) => "an array",
            Tv::Table(_) => "a table",
        }
    }

    pub fn as_table(&self) -> Option<&[Entry]> {
        match &self.v {
            Tv::Table(e) => Some(e),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match &self.v {
            Tv::Str(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_number(&self) -> Option<f64> {
        match self.v {
            Tv::Int(i) => Some(i as f64),
            Tv::Float(f) => Some(f),
            _ => None,
        }
    }

    pub fn get(&self, key: &str) -> Option<&Entry> {
        self.as_table()?.iter().find(|e| e.key == key)
    }
}

/// Line and column (1-based) of byte offsets in a text.
pub(crate) struct Source {
    line_starts: Vec<usize>,
    len: usize,
}

impl Source {
    pub fn new(text: &str) -> Self {
        let mut line_starts = vec![0];
        line_starts.extend(text.bytes().enumerate().filter(|(_, b)| *b == b'\n').map(|(i, _)| i + 1));
        Self { line_starts, len: text.len() }
    }

    pub fn line_col(&self, offset: usize) -> (usize, usize) {
        let offset = offset.min(self.len);
        let line = self.line_starts.partition_point(|s| *s <= offset) - 1;
        (line + 1, offset - self.line_starts[line] + 1)
    }

    /// An error of `kind` located at `span` (no position when the span is unknown).
    pub fn error(&self, span: &Span, kind: ErrorKind, message: impl Into<String>) -> SceneError {
        let mut e = SceneError::new(kind, message);
        if let Some(s) = span {
            let (l, c) = self.line_col(s.start);
            e.line = Some(l);
            e.column = Some(c);
        }
        e
    }
}

// ---------------------------------------------------------------------------------------------------------
// Conversions

fn de_value(v: &Spanned<DeValue<'_>>) -> Result<N, (Span, String)> {
    let span = Some(v.span());
    let tv = match v.get_ref() {
        DeValue::String(s) => Tv::Str(s.to_string()),
        DeValue::Integer(i) => {
            let digits: String = i.as_str().chars().filter(|c| *c != '_').collect();
            Tv::Int(i64::from_str_radix(&digits, i.radix()).map_err(|_| (span.clone(), "integer out of range".to_string()))?)
        }
        DeValue::Float(f) => {
            let digits: String = f.as_str().chars().filter(|c| *c != '_').collect();
            Tv::Float(digits.parse::<f64>().map_err(|_| (span.clone(), "invalid float".to_string()))?)
        }
        DeValue::Boolean(b) => Tv::Bool(*b),
        DeValue::Datetime(d) => Tv::Datetime(d.to_string()),
        DeValue::Array(items) => Tv::Array(items.iter().map(de_value).collect::<Result<_, _>>()?),
        DeValue::Table(t) => Tv::Table(de_entries(t)?),
    };
    Ok(N { v: tv, span })
}

fn de_entries(t: &DeTable<'_>) -> Result<Vec<Entry>, (Span, String)> {
    t.iter().map(|(k, v)| Ok(Entry { key: k.get_ref().to_string(), key_span: Some(k.span()), val: de_value(v)? })).collect()
}

/// The root table of parsed text.
pub(crate) fn from_text(text: &str) -> Result<N, (Span, String)> {
    let doc = DeTable::parse(text).map_err(|e| (e.span(), e.message().to_string()))?;
    Ok(N { v: Tv::Table(de_entries(doc.get_ref())?), span: Some(doc.span()) })
}

pub(crate) fn from_toml(v: &Value) -> N {
    let tv = match v {
        Value::String(s) => Tv::Str(s.clone()),
        Value::Integer(i) => Tv::Int(*i),
        Value::Float(f) => Tv::Float(*f),
        Value::Boolean(b) => Tv::Bool(*b),
        Value::Datetime(d) => Tv::Datetime(d.to_string()),
        Value::Array(a) => Tv::Array(a.iter().map(from_toml).collect()),
        Value::Table(t) => Tv::Table(from_toml_table(t)),
    };
    N { v: tv, span: None }
}

fn from_toml_table(t: &Table) -> Vec<Entry> {
    t.iter().map(|(k, v)| Entry { key: k.clone(), key_span: None, val: from_toml(v) }).collect()
}

pub(crate) fn to_toml(n: &N) -> Result<Value, String> {
    Ok(match &n.v {
        Tv::Str(s) => Value::String(s.clone()),
        Tv::Int(i) => Value::Integer(*i),
        Tv::Float(f) => Value::Float(*f),
        Tv::Bool(b) => Value::Boolean(*b),
        Tv::Datetime(d) => Value::Datetime(d.parse().map_err(|_| format!("invalid date '{d}'"))?),
        Tv::Array(a) => Value::Array(a.iter().map(to_toml).collect::<Result<_, _>>()?),
        Tv::Table(e) => Value::Table(to_toml_table(e)?),
    })
}

pub(crate) fn to_toml_table(entries: &[Entry]) -> Result<Table, String> {
    let mut t = Table::new();
    for e in entries {
        t.insert(e.key.clone(), to_toml(&e.val)?);
    }
    Ok(t)
}

pub(crate) fn from_table(t: &Table) -> N {
    N { v: Tv::Table(from_toml_table(t)), span: None }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spans_become_lines() {
        let text = "a = 1\n\n[t]\nb = [1,\n  2]\n";
        let root = from_text(text).unwrap();
        let src = Source::new(text);
        let t = root.get("t").unwrap();
        let b = t.val.get("b").unwrap();
        assert_eq!(src.line_col(b.key_span.as_ref().unwrap().start), (4, 1));
        assert_eq!(src.line_col(b.val.span.as_ref().unwrap().start), (4, 5));
        assert!(matches!(&b.val.v, Tv::Array(items) if items.len() == 2));
    }

    #[test]
    fn tables_survive_the_toml_round_trip() {
        let root = from_text("x = 1\nf = 2.5\ns = 'a'\n[t]\nl = [true, 0x10]\n").unwrap();
        let table = to_toml_table(root.as_table().unwrap()).unwrap();
        let back = from_table(&table);
        assert!(matches!(back.get("x").unwrap().val.v, Tv::Int(1)));
        assert!(matches!(back.get("f").unwrap().val.v, Tv::Float(f) if f == 2.5));
        let l = back.get("t").unwrap().val.get("l").unwrap();
        assert!(matches!(&l.val.v, Tv::Array(a) if matches!(a[1].v, Tv::Int(16))));
        assert!(back.get("x").unwrap().val.span.is_none());
    }

    #[test]
    fn syntax_errors_have_spans() {
        let (span, msg) = from_text("a = \nb = 2").unwrap_err();
        assert!(span.is_some() && !msg.is_empty());
    }
}
