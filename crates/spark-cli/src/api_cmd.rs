//! `spark api search <words> | show <name> | list` - look the Luau API up from the terminal.

use serde_json::json;

use crate::util::*;

const USAGE: &str = "usage: spark api search <words...> [--limit N] [--json]\n       spark api show <name> [--json]     (name: spawn, Mesh.cube, Object.rotate_x, ...)\n       spark api list [--json]            (every global function, table and class)";

pub fn run(mut args: Vec<String>) -> i32 {
    let json = take_flag(&mut args, "--json");
    let limit = match take_value(&mut args, "--limit") {
        Ok(v) => v.and_then(|n| n.parse::<usize>().ok()).unwrap_or(8).clamp(1, 100),
        Err(e) => return usage(json, &e),
    };
    let api = spark_api::api();
    let (cmd, rest) = match args.split_first() {
        Some((c, r)) => (c.as_str(), r),
        None => return usage(json, "missing subcommand"),
    };
    match cmd {
        "search" => {
            if rest.is_empty() {
                return usage(json, "search needs a word");
            }
            let hits = spark_api::search(&api, &rest.join(" "), limit);
            if json {
                print_json(&json!({ "command": "api search", "ok": true, "query": rest.join(" "), "hits": hits }));
            } else if hits.is_empty() {
                println!("no match for '{}' (try one word, e.g. `spark api search gravity`)", rest.join(" "));
            } else {
                for h in hits {
                    println!("{}\n    {}", h.signature, if h.doc.is_empty() { "-" } else { &h.doc });
                }
            }
            EXIT_OK
        }
        "show" => {
            let Some(name) = rest.first() else { return usage(json, "show needs a name") };
            let hits: Vec<_> = spark_api::search(&api, name, 50)
                .into_iter()
                .filter(|h| h.name.eq_ignore_ascii_case(name) || h.name.ends_with(&format!(".{name}")))
                .collect();
            if hits.is_empty() {
                if json {
                    print_json(
                        &json!({ "command": "api show", "ok": false, "exit_code": EXIT_FAILED, "error": format!("'{name}' is not in the API"), "suggestions": spark_api::search(&api, name, 5) }),
                    );
                } else {
                    eprintln!(
                        "spark api: '{name}' is not in the API; closest: {}",
                        spark_api::search(&api, name, 5).iter().map(|h| h.name.as_str()).collect::<Vec<_>>().join(", ")
                    );
                }
                return EXIT_FAILED;
            }
            if json {
                print_json(&json!({ "command": "api show", "ok": true, "items": hits }));
            } else {
                for h in hits {
                    println!("{}\n    {}", h.signature, h.doc);
                }
            }
            EXIT_OK
        }
        "list" => {
            if json {
                print_json(&json!({ "command": "api list", "ok": true, "api": api }));
            } else {
                println!("functions: {}", api.functions.iter().map(|f| f.name.as_str()).collect::<Vec<_>>().join(", "));
                println!("tables:    {}", api.namespaces.iter().map(|n| n.name.as_str()).collect::<Vec<_>>().join(", "));
                println!("classes:   {}", api.classes.iter().map(|c| c.name.as_str()).collect::<Vec<_>>().join(", "));
            }
            EXIT_OK
        }
        other => usage(json, &format!("unknown subcommand '{other}'")),
    }
}

fn usage(json: bool, msg: &str) -> i32 {
    if json {
        print_json(&json_error("api", msg));
    } else {
        eprintln!("spark api: {msg}\n{USAGE}");
    }
    EXIT_USAGE
}
