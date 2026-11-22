//! Luau field accessors of `Object` generated from the component registry (ADR 0004, steps C-7 and C-8).
//!
//! The list of fields (which names exist, on which component) comes from the registry: every field with a `lua` name of
//! the node (`name`, `visible`) and of the components whose names are flat on the object (`transform`, `material`:
//! `position`, `color`, `metallic`, ...) gets a getter and a setter on `Obj`. The accessors themselves are small typed
//! functions looked up by the field's registry path (`"material.metallic"`): scripts read and write these on thousands
//! of objects per frame, so they touch the typed `Object` fields directly instead of going through
//! `spark_reflect::Value`.
//!
//! The argument forms and error messages are the ones the hand-written closures had (`to_vec3`, `to_color`, Luau
//! truthiness for flags, `Num` arguments reporting as `bad argument #2 to Obj.metallic`, ...); `tests/bind_errors.rs`
//! is the snapshot. Side effects stay in the accessors: `metallic` / `roughness` turn `pbr` on (`with_metallic`),
//! `blend` accepts the `"transparent"` / `"add"` aliases (`BlendMode::parse`), rotation stays radians in Luau.
//!
//! Spawn tables (`spawn(mesh, mat, { color = ... })`) use the same accessors through [`prop_accessor`]: only the
//! argument checks differ (`property 'color'` in messages, flags must be booleans).
//!
//! Hand-written adapters (handles and values that are not plain fields): `material`, `shader`, `normal_map`, `orm_map`,
//! `mesh`, `lod`, and the `x` / `y` / `z` shortcuts.
use std::sync::{Arc, OnceLock};

use mlua::{IntoLua, Lua, UserDataFields, Value};
use spark_core::material::BlendMode;
use spark_core::registry::{LuaScope, registry};
use spark_core::{Color, Object, Quat, Vec3};

use crate::convert::*;
use crate::types::{LuaColor, Obj, data_table, read, to_blend, to_data};

/// How an argument is checked: the Luau name of the field, the word that starts its error messages, and whether the
/// value comes from a spawn table (`property 'x'` messages, flags must be booleans) or from a field assignment.
pub(crate) struct Ctx<'a> {
    pub what: &'a str,
    pub name: &'a str,
    pub spawn: bool,
}

/// A converted argument, ready to be stored (conversion happens before the world is touched, as it always did).
pub(crate) enum Staged {
    Vec3(Vec3),
    Quat(Quat),
    Str(String),
    Bool(bool),
    Color(Color),
    Num(f32),
    Blend(BlendMode),
    Data([f32; 4]),
}

/// How one registry field is read and written from Luau.
pub(crate) struct Accessor {
    /// Registry path of the field: `"name"`, `"visible"`, `"transform.position"`, `"material.metallic"`.
    path: &'static str,
    get: fn(&Lua, &Object) -> LuaResult<Value>,
    /// Converts the Luau value.
    stage: fn(&Value, &Ctx) -> LuaResult<Staged>,
    /// Stores it; `what` is the field's name in messages (for the error of a missing body).
    set: fn(&mut Object, Staged, &str) -> LuaResult<()>,
}

impl Accessor {
    /// Converts and stores a value of a spawn table (`what` = `property 'color'`).
    pub(crate) fn apply_prop(&self, o: &mut Object, v: &Value, name: &str, what: &str) -> LuaResult<()> {
        let staged = (self.stage)(v, &Ctx { what, name, spawn: true })?;
        (self.set)(o, staged, what)
    }
}

/// `Num` arguments (`Num(v): Num` in the old closures) failed inside mlua's argument conversion: `bad argument #2 to
/// Obj.metallic: ...`. Spawn tables report the plain message.
// mlua::Error::BadArgument holds its cause in an Arc (mlua is built without the send feature).
#[allow(clippy::arc_with_non_send_sync)]
fn number(v: &Value, c: &Ctx) -> LuaResult<f32> {
    if c.spawn {
        return to_num(v, c.what);
    }
    to_num(v, "argument").map_err(|e| mlua::Error::BadArgument {
        to: Some(format!("Obj.{}", c.name)),
        pos: 2,
        name: None,
        cause: Arc::new(e),
    })
}

/// Flags: Luau truthiness for assignments (`v: bool` of the old closures), booleans only in spawn tables.
fn flag(v: &Value, c: &Ctx) -> LuaResult<bool> {
    if c.spawn { to_bool(v, c.what) } else { Ok(!matches!(v, Value::Nil | Value::Boolean(false))) }
}

/// Stores `$staged` when it is the expected variant (the conversion of the same accessor made it).
macro_rules! store {
    ($o:ident, $pat:pat => $body:expr) => {
        |$o: &mut Object, s: Staged, _: &str| -> LuaResult<()> {
            if let $pat = s {
                $body;
            }
            Ok(())
        }
    };
}

fn color_value(lua: &Lua, c: Color) -> LuaResult<Value> {
    LuaColor(c).into_lua(lua)
}

const ACCESSORS: &[Accessor] = &[
    Accessor {
        path: "name",
        get: |lua, o| Ok(Value::String(lua.create_string(&o.name)?)),
        stage: |v, c| to_str(v, c.what).map(Staged::Str),
        set: store!(o, Staged::Str(n) => o.name = n),
    },
    Accessor {
        path: "visible",
        get: |_, o| Ok(Value::Boolean(o.visible)),
        stage: |v, c| flag(v, c).map(Staged::Bool),
        set: store!(o, Staged::Bool(b) => o.visible = b),
    },
    Accessor {
        path: "transform.position",
        get: |_, o| Ok(Value::Vector(vv(o.transform.position))),
        stage: |v, c| to_vec3(v, c.what).map(Staged::Vec3),
        set: store!(o, Staged::Vec3(p) => o.transform.position = p),
    },
    Accessor {
        path: "transform.rotation",
        get: |_, o| Ok(Value::Vector(vv(euler(o.transform.rotation)))),
        stage: |v, c| to_vec3(v, c.what).map(|e| Staged::Quat(from_euler(e))),
        set: store!(o, Staged::Quat(q) => o.transform.rotation = q),
    },
    Accessor {
        path: "transform.scale",
        get: |_, o| Ok(Value::Vector(vv(o.transform.scale))),
        stage: |v, c| to_scale(v, c.what).map(Staged::Vec3),
        set: store!(o, Staged::Vec3(p) => o.transform.scale = p),
    },
    Accessor {
        path: "material.color",
        get: |lua, o| color_value(lua, o.material.color),
        stage: |v, c| to_color(v, c.what).map(Staged::Color),
        set: store!(o, Staged::Color(x) => o.material.color = x),
    },
    Accessor {
        path: "material.data",
        get: |lua, o| data_table(lua, o.material.data).map(Value::Table),
        stage: |v, c| to_data(std::slice::from_ref(v), c.what).map(Staged::Data),
        set: store!(o, Staged::Data(d) => o.material.data = d),
    },
    Accessor {
        path: "material.blend",
        get: |lua, o| Ok(Value::String(lua.create_string(o.material.blend.name())?)),
        stage: |v, c| to_blend(v, c.what).map(Staged::Blend),
        set: store!(o, Staged::Blend(b) => o.material.blend = b),
    },
    Accessor {
        path: "material.double_sided",
        get: |_, o| Ok(Value::Boolean(o.material.double_sided)),
        stage: |v, c| flag(v, c).map(Staged::Bool),
        set: store!(o, Staged::Bool(b) => o.material.double_sided = b),
    },
    Accessor {
        path: "material.cast_shadow",
        get: |_, o| Ok(Value::Boolean(o.material.cast_shadow)),
        stage: |v, c| flag(v, c).map(Staged::Bool),
        set: store!(o, Staged::Bool(b) => o.material.cast_shadow = b),
    },
    Accessor {
        path: "material.pbr",
        get: |_, o| Ok(Value::Boolean(o.material.pbr)),
        stage: |v, c| flag(v, c).map(Staged::Bool),
        set: store!(o, Staged::Bool(b) => o.material.pbr = b),
    },
    Accessor {
        path: "material.metallic",
        get: |_, o| Ok(Value::Number(o.material.metallic as f64)),
        stage: |v, c| number(v, c).map(Staged::Num),
        // Writing `metallic` / `roughness` from a script also turns `pbr` on and clamps to 0..1 (`with_metallic`).
        set: store!(o, Staged::Num(x) => o.material = o.material.with_metallic(x)),
    },
    Accessor {
        path: "material.roughness",
        get: |_, o| Ok(Value::Number(o.material.roughness as f64)),
        stage: |v, c| number(v, c).map(Staged::Num),
        set: store!(o, Staged::Num(x) => o.material = o.material.with_roughness(x)),
    },
    Accessor {
        path: "material.emissive",
        get: |lua, o| color_value(lua, o.material.emissive),
        stage: |v, c| to_color(v, c.what).map(Staged::Color),
        set: store!(o, Staged::Color(x) => o.material.emissive = x),
    },
];

/// `(luau name, registry path)` of the fields of the object that are bound from the registry, in registry order:
/// the node, then the components whose names are flat on the object. Fields without an accessor (`mesh`, `shader`,
/// `normal_map`, ...) are hand-written.
pub(crate) fn bound_fields() -> Vec<(&'static str, String)> {
    let reg = registry();
    let mut out: Vec<(&'static str, String)> = Vec::new();
    for f in reg.node_info().fields {
        if let Some(lua) = f.lua {
            out.push((lua, f.name.to_string()));
        }
    }
    for c in reg.components.iter().filter(|c| c.lua == LuaScope::Object) {
        for f in c.info().fields {
            if let Some(lua) = f.lua {
                out.push((lua, format!("{}.{}", c.name, f.name)));
            }
        }
    }
    out.retain(|(_, path)| ACCESSORS.iter().any(|a| a.path == path));
    out
}

/// Registers the getter and setter of every bound field on `Obj`.
pub(crate) fn add_fields<F: UserDataFields<Obj>>(f: &mut F) {
    for (lua_name, path) in bound_fields() {
        let acc = ACCESSORS.iter().find(|a| a.path == path).expect("filtered by bound_fields");
        let (get, stage, set) = (acc.get, acc.stage, acc.set);
        f.add_field_method_get(lua_name, move |lua, this| read(lua, this.0, |_, o| get(lua, o))?);
        f.add_field_method_set(lua_name, move |lua, this, v: Value| {
            let staged = stage(&v, &Ctx { what: lua_name, name: lua_name, spawn: false })?;
            with(lua, |w| {
                let o = w.scene.get_mut(this.0).ok_or_else(dead)?;
                set(o, staged, lua_name)
            })
        });
    }
}

/// The accessor of a spawn-table key (`color`, `metallic`, `position`, ...). The hand-written fields (`shader`, `normal_map`, ...) are not here.
pub(crate) fn prop_accessor(key: &str) -> Option<&'static Accessor> {
    static PROPS: OnceLock<Vec<(&'static str, &'static Accessor)>> = OnceLock::new();
    let props = PROPS.get_or_init(|| {
        bound_fields().into_iter().filter_map(|(lua, path)| ACCESSORS.iter().find(|a| a.path == path).map(|a| (lua, a))).collect()
    });
    props.iter().find(|(lua, _)| *lua == key).map(|(_, a)| *a)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_fields_are_bound() {
        let names: Vec<&str> = bound_fields().iter().map(|(n, _)| *n).collect();
        assert_eq!(
            names,
            [
                "name",
                "visible",
                "position",
                "rotation",
                "scale",
                "color",
                "data",
                "blend",
                "double_sided",
                "cast_shadow",
                "pbr",
                "metallic",
                "roughness",
                "emissive"
            ]
        );
    }

    #[test]
    fn every_accessor_is_a_registry_field() {
        let bound = bound_fields();
        for a in ACCESSORS {
            assert!(bound.iter().any(|(_, p)| p == a.path), "accessor '{}' has no registry field with a lua name", a.path);
        }
    }
}
