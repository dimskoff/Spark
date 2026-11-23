//! Reflection errors with a did-you-mean list for unknown names.

use std::fmt;

/// Errors carry the type, the path, the reason and a did-you-mean list (field names of that type);
/// they map 1:1 to `SparkError` codes in spark-script / spark-devtools.
#[derive(Clone, Debug, PartialEq)]
pub enum ReflectError {
    /// No field (or enum variant) with this name. `path` is the full path as written by the caller.
    UnknownField {
        ty: &'static str,
        path: String,
        suggestions: Vec<&'static str>,
    },
    WrongType {
        path: String,
        expected: &'static str,
        got: &'static str,
    },
    OutOfRange {
        path: String,
        value: f64,
        min: f64,
        max: f64,
    },
    NotFinite {
        path: String,
    },
    ReadOnly {
        path: String,
    },
}

impl ReflectError {
    /// Stable machine-readable code.
    pub fn code(&self) -> &'static str {
        match self {
            ReflectError::UnknownField { .. } => "unknown_field",
            ReflectError::WrongType { .. } => "wrong_type",
            ReflectError::OutOfRange { .. } => "out_of_range",
            ReflectError::NotFinite { .. } => "not_finite",
            ReflectError::ReadOnly { .. } => "read_only",
        }
    }

    /// The path the error refers to.
    pub fn path(&self) -> &str {
        match self {
            ReflectError::UnknownField { path, .. }
            | ReflectError::WrongType { path, .. }
            | ReflectError::OutOfRange { path, .. }
            | ReflectError::NotFinite { path }
            | ReflectError::ReadOnly { path } => path,
        }
    }

    /// Prepends `prefix` to the path (used while an error bubbles up out of a nested value).
    #[must_use]
    pub fn at(mut self, prefix: &str) -> Self {
        let path = match &mut self {
            ReflectError::UnknownField { path, .. }
            | ReflectError::WrongType { path, .. }
            | ReflectError::OutOfRange { path, .. }
            | ReflectError::NotFinite { path }
            | ReflectError::ReadOnly { path } => path,
        };
        *path = join_path(prefix, path);
        self
    }
}

fn join_path(prefix: &str, rest: &str) -> String {
    if rest.is_empty() {
        prefix.to_string()
    } else if rest.starts_with('[') {
        format!("{prefix}{rest}")
    } else {
        format!("{prefix}.{rest}")
    }
}

fn shown(path: &str) -> &str {
    if path.is_empty() { "<value>" } else { path }
}

impl fmt::Display for ReflectError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ReflectError::UnknownField { ty, path, suggestions } => {
                write!(f, "unknown field `{path}` on {ty}")?;
                match suggestions.as_slice() {
                    [] => Ok(()),
                    [one] => write!(f, "; did you mean `{one}`?"),
                    many => {
                        let list: Vec<String> = many.iter().map(|s| format!("`{s}`")).collect();
                        write!(f, "; did you mean {}?", list.join(" or "))
                    }
                }
            }
            ReflectError::WrongType { path, expected, got } => write!(f, "`{}`: expected {expected}, got {got}", shown(path)),
            ReflectError::OutOfRange { path, value, min, max } => write!(f, "`{}`: {value} is out of range [{min}, {max}]", shown(path)),
            ReflectError::NotFinite { path } => {
                write!(f, "`{}`: value must be a finite number", shown(path))
            }
            ReflectError::ReadOnly { path } => write!(f, "`{}` is read-only", shown(path)),
        }
    }
}

impl std::error::Error for ReflectError {}

/// Up to three names from `candidates` that are close to `name` (Levenshtein distance, case-insensitive),
/// best first. Empty when nothing is close enough.
pub fn did_you_mean(name: &str, candidates: impl IntoIterator<Item = &'static str>) -> Vec<&'static str> {
    let lname = name.to_lowercase();
    let limit = (lname.chars().count() / 3).clamp(1, 3);
    let mut scored: Vec<(usize, &'static str)> = candidates
        .into_iter()
        .filter_map(|c| {
            let d = levenshtein(&lname, &c.to_lowercase());
            (d <= limit).then_some((d, c))
        })
        .collect();
    scored.sort_by_key(|(d, _)| *d); // stable: declaration order breaks ties
    scored.into_iter().take(3).map(|(_, c)| c).collect()
}

fn levenshtein(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.iter().enumerate() {
        let mut cur = vec![i + 1];
        for (j, cb) in b.iter().enumerate() {
            let cost = usize::from(ca != cb);
            cur.push((prev[j] + cost).min(prev[j + 1] + 1).min(cur[j] + 1));
        }
        prev = cur;
    }
    prev[b.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn suggestions() {
        assert_eq!(did_you_mean("colour", ["color", "mass"]), vec!["color"]);
        assert_eq!(did_you_mean("zzzzzz", ["color", "mass"]), Vec::<&str>::new());
        assert_eq!(did_you_mean("MASS", ["mass"]), vec!["mass"]);
    }

    #[test]
    fn path_prefixing() {
        let e = ReflectError::NotFinite { path: "radius".into() };
        assert_eq!(e.at("shape").path(), "shape.radius");
        let e = ReflectError::NotFinite { path: String::new() };
        assert_eq!(e.at("mass").path(), "mass");
        let e = ReflectError::NotFinite { path: "[2]".into() };
        assert_eq!(e.at("items").path(), "items[2]");
    }
}
