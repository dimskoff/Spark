//! Errors of reading and writing scene files.

use std::fmt;

use spark_core::NodeId;

/// What kind of problem a [`SceneError`] is (stable, for tools).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrorKind {
    /// Not valid TOML.
    Syntax,
    /// A name that does not exist: component, field, key, enum variant. The message carries a did-you-mean.
    UnknownName,
    /// A value of the wrong type or out of range, or a missing required key.
    Invalid,
    /// Duplicate node ids, a parent that does not exist, a parent cycle.
    Structure,
    /// The file's `format_version` is newer than this Spark knows.
    TooNew,
    /// A migration step failed.
    Migration,
}

impl ErrorKind {
    pub fn code(self) -> &'static str {
        match self {
            ErrorKind::Syntax => "syntax",
            ErrorKind::UnknownName => "unknown_name",
            ErrorKind::Invalid => "invalid",
            ErrorKind::Structure => "structure",
            ErrorKind::TooNew => "too_new",
            ErrorKind::Migration => "migration",
        }
    }
}

/// A scene file could not be read. `line` / `column` are 1-based and point at the offending key or value (absent
/// when the file was migrated first: the text no longer matches the tree).
#[derive(Clone, Debug, PartialEq)]
pub struct SceneError {
    pub kind: ErrorKind,
    pub message: String,
    pub line: Option<usize>,
    pub column: Option<usize>,
    /// Where in the document: `node 'crate', [node.body]`.
    pub context: Option<String>,
}

impl SceneError {
    pub(crate) fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        Self { kind, message: message.into(), line: None, column: None, context: None }
    }

    pub(crate) fn with_context(mut self, context: impl Into<String>) -> Self {
        self.context.get_or_insert_with(|| context.into());
        self
    }

    pub fn code(&self) -> &'static str {
        self.kind.code()
    }
}

impl fmt::Display for SceneError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(line) = self.line {
            write!(f, "line {line}")?;
            if let Some(c) = self.column {
                write!(f, ", column {c}")?;
            }
            f.write_str(": ")?;
        }
        f.write_str(&self.message)?;
        if let Some(c) = &self.context {
            write!(f, " ({c})")?;
        }
        Ok(())
    }
}

impl std::error::Error for SceneError {}

/// Something in a document that cannot be written to a file.
#[derive(Clone, Debug, PartialEq)]
pub struct SaveIssue {
    /// The node, `None` for the environment.
    pub node: Option<NodeId>,
    /// Display name of the node (may be empty).
    pub node_name: String,
    /// `mesh`, `material`, `environment`, ...
    pub component: String,
    pub message: String,
}

impl fmt::Display for SaveIssue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.node {
            Some(id) if self.node_name.is_empty() => write!(f, "node '{id}'")?,
            Some(id) => write!(f, "node '{id}' (\"{}\")", self.node_name)?,
            None => f.write_str("scene")?,
        }
        write!(f, ", {}: {}", self.component, self.message)
    }
}

/// The document has parts that cannot be saved (runtime-only meshes and textures, invalid values). Nothing is
/// dropped silently: use [`crate::to_string_lossy`] to save the rest anyway.
#[derive(Clone, Debug, PartialEq)]
pub struct SaveError {
    pub issues: Vec<SaveIssue>,
}

impl fmt::Display for SaveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} thing(s) cannot be saved to a scene file:", self.issues.len())?;
        for i in &self.issues {
            write!(f, "\n  {i}")?;
        }
        Ok(())
    }
}

impl std::error::Error for SaveError {}
