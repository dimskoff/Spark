//! The Luau API of Spark as data.
//!
//! `docs/spark.d.luau` is the single machine-readable description of the API: every global, namespace, class,
//! signature and the doc comment above it. This crate parses it into an [`Api`] registry and generates from it
//! `docs/API_REFERENCE.md`, `llms.txt`, `llms-full.txt` and `docs/api.json`, and answers `spark api search`.
//!
//! The registry is checked against the running engine by `crates/spark-script/tests/api_drift.rs`: a function that exists
//! in the VM but not in the registry (or the other way round) fails CI. See `docs/adr/0001-api-registry.md`.

mod parse;
mod render;

pub use parse::{ParseError, parse};
pub use render::{Hit, Kind, llms_txt, markdown, search};

use serde::Serialize;

/// The API description shipped with this engine version (the file that games get as `docs/spark.d.luau`).
pub const D_LUAU: &str = include_str!("../../../docs/spark.d.luau");

/// The registry of the shipped API.
pub fn api() -> Api {
    parse(D_LUAU).expect("docs/spark.d.luau must parse (tests check this)")
}

/// A `name: type` pair of a signature.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Param {
    pub name: String,
    /// Luau type text as written, e.g. `ColorLike?`.
    pub ty: String,
    /// `T?` or `...`: may be left out.
    pub optional: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Function {
    pub name: String,
    pub generics: String,
    pub params: Vec<Param>,
    /// Return type text, `()` when nothing is returned.
    pub ret: String,
    pub doc: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Field {
    pub name: String,
    pub ty: String,
    pub doc: String,
}

impl Field {
    /// The value may be `nil`.
    pub fn optional(&self) -> bool {
        self.ty.ends_with('?') || self.ty.contains("| nil") || self.ty == "any"
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub enum Member {
    Function(Function),
    Field(Field),
}

impl Member {
    pub fn name(&self) -> &str {
        match self {
            Member::Function(f) => &f.name,
            Member::Field(f) => &f.name,
        }
    }
    pub fn doc(&self) -> &str {
        match self {
            Member::Function(f) => &f.doc,
            Member::Field(f) => &f.doc,
        }
    }
}

/// `declare Mesh: { cube: (...) -> Mesh, ... }`: a global table (or object) with members.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Namespace {
    pub name: String,
    pub members: Vec<Member>,
    pub doc: String,
}

/// `declare extern type Object with ... end`: values scripts receive, with fields and methods (`obj:method()`).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Class {
    pub name: String,
    pub fields: Vec<Field>,
    pub methods: Vec<Function>,
    pub doc: String,
}

/// `type Ease = "linear" | ...`
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Alias {
    pub name: String,
    pub def: String,
    pub doc: String,
}

/// `declare camera: Camera`
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Value {
    pub name: String,
    pub ty: String,
    pub doc: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Api {
    pub aliases: Vec<Alias>,
    pub classes: Vec<Class>,
    pub namespaces: Vec<Namespace>,
    pub functions: Vec<Function>,
    pub values: Vec<Value>,
}

impl Api {
    pub fn function(&self, name: &str) -> Option<&Function> {
        self.functions.iter().find(|f| f.name == name)
    }
    pub fn namespace(&self, name: &str) -> Option<&Namespace> {
        self.namespaces.iter().find(|n| n.name == name)
    }
    pub fn class(&self, name: &str) -> Option<&Class> {
        self.classes.iter().find(|c| c.name == name)
    }
    /// Names of every global the API defines (functions, namespaces, values).
    pub fn global_names(&self) -> Vec<&str> {
        let mut v: Vec<&str> = self.functions.iter().map(|f| f.name.as_str()).collect();
        v.extend(self.namespaces.iter().map(|n| n.name.as_str()));
        v.extend(self.values.iter().map(|n| n.name.as_str()));
        v
    }
}

impl Function {
    /// `name(a: T, b: U?): R`; `prefix` is `"Mesh."`, `"object:"` or empty.
    pub fn signature(&self, prefix: &str) -> String {
        let params: Vec<String> =
            self.params.iter().map(|p| if p.name.is_empty() { p.ty.clone() } else { format!("{}: {}", p.name, p.ty) }).collect();
        let generics = if self.generics.is_empty() { String::new() } else { format!("<{}>", self.generics) };
        let ret = if self.ret == "()" || self.ret.is_empty() { String::new() } else { format!(" -> {}", self.ret) };
        format!("{prefix}{}{generics}({}){ret}", self.name, params.join(", "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shipped_api_parses_and_has_the_core() {
        let a = api();
        assert!(a.functions.len() > 30, "{} functions", a.functions.len());
        assert!(a.namespaces.len() > 12);
        assert!(a.classes.len() > 12);
        let spawn = a.function("spawn").expect("spawn");
        assert_eq!(spawn.params.len(), 3);
        assert_eq!(spawn.ret, "Object");
        assert!(spawn.params[1].optional && spawn.params[2].optional && !spawn.params[0].optional);
        let cube = match a.namespace("Mesh").unwrap().members.iter().find(|m| m.name() == "cube").unwrap() {
            Member::Function(f) => f.clone(),
            m => panic!("{m:?}"),
        };
        assert_eq!(cube.signature("Mesh."), "Mesh.cube(size: number?) -> Mesh");
        assert!(a.namespace("Color").unwrap().members.iter().any(|m| m.name() == "WHITE" && matches!(m, Member::Field(_))));
        let obj = a.class("Object").unwrap();
        assert!(obj.methods.iter().any(|m| m.name == "rotate_x"));
        assert!(obj.fields.iter().any(|f| f.name == "position"));
        assert!(a.values.iter().any(|v| v.name == "camera" && v.ty == "Camera"));
        assert!(a.aliases.iter().any(|t| t.name == "Ease"));
    }

    #[test]
    fn doc_comments_attach_to_the_next_item() {
        let a = parse("-- Adds.\n-- Two lines.\ndeclare function add(a: number, b: number): number\n\ndeclare x: {\n    -- the y\n    y: number, -- trailing\n}\n").unwrap();
        assert_eq!(a.function("add").unwrap().doc, "Adds.\nTwo lines.");
        let y = &a.namespace("x").unwrap().members[0];
        assert_eq!(y.doc(), "the y\ntrailing");
    }

    #[test]
    fn broken_input_is_an_error_not_a_panic() {
        assert!(parse("declare function broken(a: number").is_err());
        assert!(parse("declare ns: {\n a: number,\n").is_err());
        assert!(parse("declare extern type X with\n field: number\n").is_err());
    }
}
