use clap::{ArgAction, CommandFactory};
use serde_json::{json, Value};

use super::commands::ManagerCli;
use crate::error::{SignerError, SignerResult};

#[derive(Debug, Clone, PartialEq, Eq)]
struct FlagGroup {
    boolean: Vec<String>,
    with_value: Vec<String>,
}

impl FlagGroup {
    fn to_json(&self) -> Value {
        json!({
            "boolean": self.boolean,
            "with_value": self.with_value,
        })
    }
}

fn classify_long_flags(cmd: &clap::Command) -> FlagGroup {
    let mut boolean = Vec::new();
    let mut with_value = Vec::new();
    for arg in cmd.get_arguments() {
        if arg.is_hide_set() {
            continue;
        }
        let Some(long) = arg.get_long() else {
            continue;
        };
        if matches!(long, "help" | "version") {
            continue;
        }
        let flag = format!("--{long}");
        match arg.get_action() {
            ArgAction::SetTrue | ArgAction::SetFalse => boolean.push(flag),
            _ => with_value.push(flag),
        }
    }
    boolean.sort();
    with_value.sort();
    FlagGroup {
        boolean,
        with_value,
    }
}

pub fn flag_groups_json(subcommand: &str) -> SignerResult<Value> {
    let root = ManagerCli::command();
    let global = classify_long_flags(&root);
    let Some(sub) = root.find_subcommand(subcommand) else {
        return Err(SignerError::Other(format!(
            "unknown manager subcommand: {subcommand}"
        )));
    };
    let subcommand_flags = classify_long_flags(sub);
    Ok(json!({
        "subcommand": subcommand,
        "global": global.to_json(),
        "subcommand_flags": subcommand_flags.to_json(),
    }))
}

pub fn emit_flag_groups(subcommand: &str) -> SignerResult<Value> {
    flag_groups_json(subcommand)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flag_groups_include_known_manager_globals_and_combine_flags() {
        let payload = flag_groups_json("combine-market-cat-dust").expect("flag groups");
        let global = payload
            .get("global")
            .and_then(Value::as_object)
            .expect("global object");
        let global_flags: Vec<&str> = global
            .get("with_value")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .chain(
                global
                    .get("boolean")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten(),
            )
            .map(|value| value.as_str().expect("flag name"))
            .collect();
        assert!(global_flags.contains(&"--program-config"));
        assert!(global_flags.contains(&"--json"));

        let subcommand = payload
            .get("subcommand_flags")
            .and_then(Value::as_object)
            .expect("subcommand object");
        let sub_flags: Vec<&str> = subcommand
            .get("with_value")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .chain(
                subcommand
                    .get("boolean")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten(),
            )
            .map(|value| value.as_str().expect("flag name"))
            .collect();
        assert!(sub_flags.contains(&"--dry-run"));
        assert!(sub_flags.contains(&"--verify-timeout-seconds"));
    }
}
