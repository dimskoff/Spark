//! The dynamic value exchanged between the inspector, scene files, devtools and Luau.

/// Dynamic value: what the inspector, files, devtools and Luau exchange.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
    Vec2([f32; 2]),
    Vec3([f32; 3]),
    Vec4([f32; 4]),
    /// Linear RGBA.
    Color([f32; 4]),
    /// Enum value. Unit variants have no fields.
    Enum {
        variant: String,
        fields: Vec<(String, Value)>,
    },
    List(Vec<Value>),
    /// Ordered map; also the representation of a reflected struct and of `script.props`.
    Map(Vec<(String, Value)>),
    /// Stable node id.
    NodeRef(String),
    /// Project-relative path, `#fragment` allowed.
    Asset(String),
    /// `Option::None`.
    None,
}

impl Value {
    /// Short lowercase name of the variant, used in error messages ("float", "vec3", ...).
    pub fn type_name(&self) -> &'static str {
        match self {
            Value::Bool(_) => "bool",
            Value::Int(_) => "int",
            Value::Float(_) => "float",
            Value::Str(_) => "string",
            Value::Vec2(_) => "vec2",
            Value::Vec3(_) => "vec3",
            Value::Vec4(_) => "vec4",
            Value::Color(_) => "color",
            Value::Enum { .. } => "enum",
            Value::List(_) => "list",
            Value::Map(_) => "map",
            Value::NodeRef(_) => "node_ref",
            Value::Asset(_) => "asset",
            Value::None => "none",
        }
    }

    /// Unit variant helper: `Value::unit("Dynamic")`.
    pub fn unit(variant: &str) -> Value {
        Value::Enum { variant: variant.to_string(), fields: Vec::new() }
    }

    /// Looks a key up in a `Map` (or the fields of an `Enum`).
    pub fn field(&self, name: &str) -> Option<&Value> {
        let pairs = match self {
            Value::Map(p) => p,
            Value::Enum { fields, .. } => fields,
            _ => return None,
        };
        pairs.iter().find(|(k, _)| k == name).map(|(_, v)| v)
    }
}

impl From<bool> for Value {
    fn from(v: bool) -> Self {
        Value::Bool(v)
    }
}
impl From<i64> for Value {
    fn from(v: i64) -> Self {
        Value::Int(v)
    }
}
impl From<i32> for Value {
    fn from(v: i32) -> Self {
        Value::Int(i64::from(v))
    }
}
impl From<f64> for Value {
    fn from(v: f64) -> Self {
        Value::Float(v)
    }
}
impl From<f32> for Value {
    fn from(v: f32) -> Self {
        Value::Float(f64::from(v))
    }
}
impl From<&str> for Value {
    fn from(v: &str) -> Self {
        Value::Str(v.to_string())
    }
}
impl From<String> for Value {
    fn from(v: String) -> Self {
        Value::Str(v)
    }
}
impl From<[f32; 2]> for Value {
    fn from(v: [f32; 2]) -> Self {
        Value::Vec2(v)
    }
}
impl From<[f32; 3]> for Value {
    fn from(v: [f32; 3]) -> Self {
        Value::Vec3(v)
    }
}
impl From<[f32; 4]> for Value {
    fn from(v: [f32; 4]) -> Self {
        Value::Vec4(v)
    }
}
