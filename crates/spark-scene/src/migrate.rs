//! Format migrations (ADR 0003, rule 8).
//!
//! A file declares `format_version = N` in its header. The loader parses to a raw TOML table, runs the step that
//! leaves version `N`, then the next, ... until [`FORMAT_VERSION`], and only then decodes through the registry. A step is
//! a pure function on the table. Adding a version means: raise `FORMAT_VERSION`, append a [`Step`] to [`STEPS`], add a
//! golden fixture for the old version to the tests (the test `steps_are_a_chain` checks the list is complete).

use toml::{Table, Value};

/// The version this Spark reads natively and always writes.
pub const FORMAT_VERSION: u32 = 1;

/// One migration: turns a file of version `from` into version `from + 1`.
pub struct Step {
    pub from: u32,
    pub name: &'static str,
    pub run: fn(&mut Table) -> Result<(), String>,
}

/// Every step, in order: `STEPS[i].from == i`.
pub const STEPS: &[Step] = &[Step { from: 0, name: "enum tables name their variant with `type` (was `kind`)", run: v0_to_v1 }];

/// The header table (`[scene]` or `[prefab]`) of a raw file.
pub(crate) fn header_mut(t: &mut Table) -> Option<&mut Table> {
    let key = if t.contains_key("scene") { "scene" } else { "prefab" };
    t.get_mut(key)?.as_table_mut()
}

/// Runs the steps from version `from` up to `target` on `table` and updates `format_version` after each one.
pub fn migrate_table(table: &mut Table, from: u32, steps: &[Step], target: u32) -> Result<(), String> {
    let mut version = from;
    while version < target {
        let step = steps.iter().find(|s| s.from == version).ok_or_else(|| format!("no migration from format_version {version}"))?;
        (step.run)(table).map_err(|e| format!("migration {} -> {}: {e}", version, version + 1))?;
        version += 1;
        if let Some(h) = header_mut(table) {
            h.insert("format_version".into(), Value::Integer(i64::from(version)));
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------------------------
// v0 -> v1

/// v0 was the draft in the first version of ADR 0003: the inline table of a data-bearing enum named its variant with
/// `kind` (`shape = { kind = "box", size = [1, 1, 1] }`), which collided with fields called `kind`
/// (`[node.light] kind = { kind = "spot" }`). v1 uses `type`.
fn v0_to_v1(t: &mut Table) -> Result<(), String> {
    const ENUM_FIELDS: &[(&str, &str)] = &[("body", "shape"), ("light", "kind")];
    let Some(Value::Array(nodes)) = t.get_mut("node") else { return Ok(()) };
    for node in nodes {
        let Some(node) = node.as_table_mut() else { continue };
        for (component, field) in ENUM_FIELDS {
            let Some(Value::Table(comp)) = node.get_mut(*component) else { continue };
            if let Some(Value::Table(e)) = comp.get_mut(*field) {
                rename_key(e, "kind", "type");
            }
        }
    }
    Ok(())
}

/// Renames a key keeping its position (unless the new name is already taken).
fn rename_key(t: &mut Table, from: &str, to: &str) {
    if !t.contains_key(from) || t.contains_key(to) {
        return;
    }
    let old = std::mem::take(t);
    for (k, v) in old {
        t.insert(if k == from { to.to_string() } else { k }, v);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table(s: &str) -> Table {
        s.parse().unwrap()
    }

    #[test]
    fn steps_are_a_chain() {
        assert_eq!(STEPS.len(), FORMAT_VERSION as usize, "one step per version");
        for (i, s) in STEPS.iter().enumerate() {
            assert_eq!(s.from as usize, i);
        }
    }

    #[test]
    fn v0_renames_the_variant_key_in_place() {
        let mut t = table(
            "[scene]\nformat_version = 0\n[[node]]\nid = 'a'\n[node.light]\nkind = { kind = 'spot', angle = 50 }\ncolor = '#ffffff'\n\
             [node.body]\nkind = 'static'\nshape = { kind = 'box', size = [1, 1, 1] }\n",
        );
        migrate_table(&mut t, 0, STEPS, 1).unwrap();
        let light = &t["node"][0]["light"]["kind"];
        assert_eq!(light["type"].as_str(), Some("spot"));
        assert!(light.get("kind").is_none());
        assert_eq!(light.as_table().unwrap().keys().next().map(String::as_str), Some("type"), "position is kept");
        assert_eq!(t["node"][0]["body"]["kind"].as_str(), Some("static"), "a field called kind is not touched");
        assert_eq!(t["node"][0]["body"]["shape"]["type"].as_str(), Some("box"));
        assert_eq!(t["scene"]["format_version"].as_integer(), Some(1));
    }

    #[test]
    fn a_chain_of_steps_runs_in_order() {
        fn a(t: &mut Table) -> Result<(), String> {
            t.insert("a".into(), Value::Integer(1));
            Ok(())
        }
        fn b(t: &mut Table) -> Result<(), String> {
            let seen = t.get("a").and_then(Value::as_integer).ok_or("a missing")?;
            t.insert("b".into(), Value::Integer(seen + 1));
            Ok(())
        }
        let steps = [Step { from: 3, name: "a", run: a }, Step { from: 4, name: "b", run: b }];
        let mut t = table("[prefab]\nformat_version = 3\n");
        migrate_table(&mut t, 3, &steps, 5).unwrap();
        assert_eq!(t["b"].as_integer(), Some(2));
        assert_eq!(t["prefab"]["format_version"].as_integer(), Some(5));
        let err = migrate_table(&mut table("[scene]\n"), 9, &steps, 10).unwrap_err();
        assert!(err.contains("no migration from format_version 9"), "{err}");
    }
}
