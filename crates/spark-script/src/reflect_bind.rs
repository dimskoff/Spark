//! Luau field accessors of `Object` generated from the component registry (ADR 0004, step C-7).
//!
//! The list of fields (which names exist, on which component) comes from the registry: every field with a `lua` name of
//! the node (`name`, `visible`) and of the `transform` component (`position`, `rotation`, `scale`) gets a getter and a
//! setter. The accessors themselves are small typed functions looked up by the field's registry path (`"transform.position"`):
//! scripts read and write these on thousands of objects per frame, so they touch the typed `Object` fields directly
//! instead of going through `spark_reflect::Value`. The argument forms and error messages are the ones the hand-written
//! closures had (`to_vec3`, `to_scale`, `to_str`, Luau truthiness for `visible`); rotation stays radians in Luau.

use mlua::{Lua, UserDataFields, Value};
use spark_core::registry::registry;
use spark_core::{Object, Quat, Vec3};

use crate::convert::*;
use crate::types::{Obj, read, write};

/// A converted argument, ready to be stored (conversion happens before the world is touched, as it always did).
enum Staged {
    Vec3(Vec3),
    Quat(Quat),
    Str(String),
    Bool(bool),
}

/// How one registry field is read and written from Luau.
struct Accessor {
    /// Registry path of the field: `"name"`, `"visible"`, `"transform.position"`.
    path: &'static str,
    get: fn(&Lua, &Object) -> LuaResult<Value>,
    /// Converts the Luau value; `what` is the field's Luau name (error messages).
    stage: fn(&Value, &str) -> LuaResult<Staged>,
    set: fn(&mut Object, Staged),
}

const ACCESSORS: &[Accessor] = &[
    Accessor {
        path: "name",
        get: |lua, o| Ok(Value::String(lua.create_string(&o.name)?)),
        stage: |v, what| to_str(v, what).map(Staged::Str),
        set: |o, s| {
            if let Staged::Str(n) = s {
                o.name = n;
            }
        },
    },
    Accessor {
        path: "visible",
        get: |_, o| Ok(Value::Boolean(o.visible)),
        // `v: bool` of the old closure: nil and false are false, everything else is true.
        stage: |v, _| Ok(Staged::Bool(!matches!(v, Value::Nil | Value::Boolean(false)))),
        set: |o, s| {
            if let Staged::Bool(b) = s {
                o.visible = b;
            }
        },
    },
    Accessor {
        path: "transform.position",
        get: |_, o| Ok(Value::Vector(vv(o.transform.position))),
        stage: |v, what| to_vec3(v, what).map(Staged::Vec3),
        set: |o, s| {
            if let Staged::Vec3(p) = s {
                o.transform.position = p;
            }
        },
    },
    Accessor {
        path: "transform.rotation",
        get: |_, o| Ok(Value::Vector(vv(euler(o.transform.rotation)))),
        stage: |v, what| to_vec3(v, what).map(|e| Staged::Quat(from_euler(e))),
        set: |o, s| {
            if let Staged::Quat(q) = s {
                o.transform.rotation = q;
            }
        },
    },
    Accessor {
        path: "transform.scale",
        get: |_, o| Ok(Value::Vector(vv(o.transform.scale))),
        stage: |v, what| to_scale(v, what).map(Staged::Vec3),
        set: |o, s| {
            if let Staged::Vec3(p) = s {
                o.transform.scale = p;
            }
        },
    },
];

/// `(luau name, registry path)` of the fields that are bound from the registry, in registry order.
pub(crate) fn bound_fields() -> Vec<(&'static str, String)> {
    let reg = registry();
    let mut out: Vec<(&'static str, String)> = Vec::new();
    for f in reg.node_info().fields {
        if let Some(lua) = f.lua {
            out.push((lua, f.name.to_string()));
        }
    }
    if let Some(t) = reg.component("transform") {
        for f in t.info().fields {
            if let Some(lua) = f.lua {
                out.push((lua, format!("transform.{}", f.name)));
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
            let staged = stage(&v, lua_name)?;
            write(lua, this.0, |o| set(o, staged))
        });
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_fields_are_bound() {
        let names: Vec<&str> = bound_fields().iter().map(|(n, _)| *n).collect();
        assert_eq!(names, ["name", "visible", "position", "rotation", "scale"]);
    }

    #[test]
    fn every_accessor_is_a_registry_field() {
        let bound = bound_fields();
        for a in ACCESSORS {
            assert!(bound.iter().any(|(_, p)| p == a.path), "accessor '{}' has no registry field with a lua name", a.path);
        }
    }
}
