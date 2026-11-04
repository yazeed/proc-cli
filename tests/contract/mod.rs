//! The `--json` contract every document keeps, checked wherever a test reads
//! one (skills/proc-cli/reference.md, "Exit codes"; PHILOSOPHY.md, "One
//! Contract"). The document being one JSON value is checked by the parse
//! itself: serde refuses anything after the first value.

// Each test binary uses one of the two
#![allow(dead_code)]

use serde_json::{Map, Value};
use std::collections::BTreeMap;

/// What is wrong with a document `proc` printed before exiting with `code`,
/// if anything:
/// - the exit code is one of proc's, 0 to 4;
/// - `action` names the command;
/// - `success` is true exactly when the exit code is 0;
/// - `exit_code`, which every error has, is the exit code;
/// - every error has a `reason`, one of its exit code's words, and nothing
///   else has one;
/// - `ruled_out_by` comes with `reason: ruled_out`, and only then;
/// - the names rules of [`names_violation`].
pub fn violation(json: &Value, code: i32) -> Option<String> {
    if !(0..=4).contains(&code) {
        return Some(format!("exit {code} is not one of proc's codes, 0 to 4"));
    }
    if !json["action"].is_string() {
        return Some("no `action`".to_string());
    }
    if json["success"] != (code == 0) {
        return Some(format!("exit {code} but success = {}", json["success"]));
    }
    if json.get("error").is_some() && json.get("exit_code").is_none() {
        return Some("an `error` without `exit_code`".to_string());
    }
    if json.get("exit_code").is_some_and(|field| *field != code) {
        return Some(format!("exit {code} but exit_code = {}", json["exit_code"]));
    }
    if json.get("reason").is_some() != json.get("error").is_some() {
        return Some(format!(
            "error = {}, reason = {}: every error has a reason, and only errors do",
            json["error"], json["reason"]
        ));
    }
    if let Some(reason) = json.get("reason") {
        let words: &[&str] = match code {
            1 => &["system_error"],
            2 => &["not_found", "command_line_only", "ruled_out"],
            3 => &["permission_denied"],
            _ => &["invalid_input", "needs_confirmation", "not_supported"],
        };
        if !words.iter().any(|word| reason == word) {
            return Some(format!(
                "exit {code} with reason = {reason}: not one of {words:?}"
            ));
        }
    }
    if json.get("ruled_out_by").is_some() != (json["reason"] == "ruled_out") {
        return Some(format!(
            "reason = {}, ruled_out_by = {}: they come together",
            json["reason"], json["ruled_out_by"]
        ));
    }
    names_violation(json)
}

/// Names that 2.3.0 deprecated and 3.0 removes (skills/proc-cli/reference.md).
/// Each is exempt from the one rule it breaks, and from nothing else, so the
/// list can only shrink: a name added here is a new break, and needs a reason.
///
/// Each is accepted only in the document of the command (`action`) that carries it.
///
/// Rule 3, a duration ends in `_secs`
const DEPRECATED_DURATIONS: &[(&str, &str)] = &[
    ("wait", "elapsed_seconds"),
    ("wait", "exited_after_seconds"),
];
/// Rule 2, a `_count` sits beside a list of its stem or counts an outcome
const DEPRECATED_COUNTS: &[(&str, &str)] = &[("info", "found_count"), ("wait", "initial_count")];
/// The outcome words a count may be named for, without a list of that name
const OUTCOMES: &[&str] = &[
    "succeeded",
    "failed",
    "recovered",
    "not_stuck",
    "still_stuck",
    "terminated",
];
/// A number about one process (how many children it has), not the count of a
/// list beside it: the one `_count` that is not rule 2's
const PROCESS_COUNTS: &[&str] = &["child_count"];

/// Whether `key` holds a duration: it says seconds, elapsed, uptime or a
/// duration in one of its words. This is a word list: when a new key holds a
/// duration in other words (`age`, `ttl`), add the word here, so the rule sees it.
fn is_duration(key: &str) -> bool {
    key.split('_').any(|word| {
        [
            "seconds", "secs", "elapsed", "uptime", "duration", "timeout",
        ]
        .contains(&word)
    })
}

/// What is wrong with the names in a document, if anything:
/// - rule 3: a key that holds a duration ends in `_secs` (`elapsed_human` is
///   the same time as words, and says so);
/// - rule 2: a key ending `_count` holds a number and sits beside a list of
///   the same stem (`not_found_count` beside `not_found`), or counts an
///   outcome (`recovered_count`); and `count` is the length of the main list
///   beside it (`processes`, `results` or `ports`; `wait` has `exited` and
///   `still_running`), or 0 where there is none;
/// - rule 5: a `processes` list whose entries have an `outcome` of `succeeded`
///   or `failed` is a receipt: a failure says why in `error`, a success has
///   none, and where the older `succeeded` and `failed` lists are kept they
///   hold the same processes.
///
/// Rule 6 needs more than one document: see [`mixed_kinds`].
pub fn names_violation(json: &Value) -> Option<String> {
    let mut found = None;
    let action = json["action"].as_str().unwrap_or("");
    visit(json, &mut |object| {
        found = found.take().or_else(|| object_violation(action, object));
    });
    found
}

/// Call `each` on every object in the document, however deep
fn visit(json: &Value, each: &mut impl FnMut(&Map<String, Value>)) {
    match json {
        Value::Object(object) => {
            each(object);
            object.values().for_each(|value| visit(value, each));
        }
        Value::Array(items) => items.iter().for_each(|value| visit(value, each)),
        _ => {}
    }
}

fn object_violation(action: &str, object: &Map<String, Value>) -> Option<String> {
    let deprecated = |list: &[(&str, &str)], key: &str| list.contains(&(action, key));
    for (key, value) in object {
        if is_duration(key)
            && !key.ends_with("_secs")
            && !key.ends_with("_human")
            && !deprecated(DEPRECATED_DURATIONS, key)
        {
            return Some(format!(
                "`{key}` holds a duration, and does not end in _secs"
            ));
        }
        if let Some(stem) = key.strip_suffix("_count") {
            if !value.is_u64() {
                return Some(format!("`{key}` = {value}: a count is a number"));
            }
            let listed = object.get(stem).is_some_and(Value::is_array);
            let allowed = OUTCOMES.contains(&stem)
                || deprecated(DEPRECATED_COUNTS, key)
                || PROCESS_COUNTS.contains(&key.as_str());
            if !listed && !allowed {
                return Some(format!(
                    "`{key}` is beside no list `{stem}` and counts no outcome"
                ));
            }
        }
    }
    if let Some(count) = object.get("count") {
        let length = |key: &str| object.get(key).and_then(Value::as_array).map(Vec::len);
        let expected = ["processes", "results", "ports"]
            .iter()
            .find_map(|key| length(key))
            .or_else(|| Some(length("exited")? + length("still_running")?))
            .unwrap_or(0);
        if *count != expected {
            return Some(format!(
                "count = {count}, but the list beside it has {expected}"
            ));
        }
    }
    receipt_violation(object)
}

fn outcome(entry: &Value) -> Option<&str> {
    entry.get("outcome").and_then(Value::as_str)
}

/// Rule 5, for the object that has a receipt's `processes`
fn receipt_violation(object: &Map<String, Value>) -> Option<String> {
    let processes = object.get("processes").and_then(Value::as_array)?;
    if !processes
        .iter()
        .all(|entry| matches!(outcome(entry), Some("succeeded" | "failed")))
    {
        // a preview, or unstick, which has outcomes of its own
        return None;
    }
    for entry in processes {
        let failed = outcome(entry) == Some("failed");
        if entry.get("error").is_some() != failed {
            return Some(format!(
                "a receipt entry has outcome and error apart: {entry}"
            ));
        }
    }
    let pids_of = |items: Option<&Value>, process_key: Option<&str>| -> Vec<u64> {
        let mut pids: Vec<u64> = items
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| match process_key {
                        Some(key) => item[key]["pid"].as_u64(),
                        None => item["pid"].as_u64(),
                    })
                    .collect()
            })
            .unwrap_or_default();
        pids.sort_unstable();
        pids
    };
    let with = |wanted: &str| -> Vec<u64> {
        let mut pids: Vec<u64> = processes
            .iter()
            .filter(|entry| outcome(entry) == Some(wanted))
            .filter_map(|entry| entry["pid"].as_u64())
            .collect();
        pids.sort_unstable();
        pids
    };
    if object.contains_key("succeeded")
        && pids_of(object.get("succeeded"), None) != with("succeeded")
    {
        return Some("`succeeded` and the `succeeded` entries of `processes` differ".to_string());
    }
    if object.contains_key("failed")
        && pids_of(object.get("failed"), Some("process")) != with("failed")
    {
        return Some("`failed` and the `failed` entries of `processes` differ".to_string());
    }
    None
}

/// Rule 6, one word one meaning: what kind of value each key held, over
/// every document given to [`note_kinds`]
pub type Kinds = BTreeMap<String, BTreeMap<&'static str, String>>;

/// Add the kind of value of every key in `json` (a document of `action`)
pub fn note_kinds(json: &Value, kinds: &mut Kinds) {
    let action = json["action"].as_str().unwrap_or("?").to_string();
    visit(json, &mut |object| {
        for (key, value) in object {
            let kind = match value {
                Value::Array(_) => "an array",
                Value::Number(_) => "a number",
                Value::String(_) => "a string",
                Value::Bool(_) => "a bool",
                Value::Object(_) => "an object",
                Value::Null => "null",
            };
            kinds
                .entry(key.clone())
                .or_default()
                .entry(kind)
                .or_insert_with(|| action.clone());
        }
    });
}

/// What is wrong with the kinds seen: a key that has two different kinds of
/// value in different documents. `failed` is the one exception: a list of what
/// failed in the receipts of `kill`, `stop`, `freeze`, `thaw`, `stuck --kill`
/// and `orphans --kill`, and a number in `unstick`; `unstick`'s bare numbers are
/// deprecated (use `failed_count`), removed in 3.0, and the exception goes
/// with them.
pub fn mixed_kinds(kinds: &Kinds) -> Option<String> {
    kinds.iter().find_map(|(key, seen)| {
        let mut two = seen.iter();
        let (first, second) = (two.next()?, two.next()?);
        (key != "failed").then(|| {
            format!(
                "`{key}` is {} in a `{}` document and {} in a `{}` one",
                first.0, first.1, second.0, second.1
            )
        })
    })
}

/// The top-level keys of a pretty-printed document, in the order it printed
/// them: a key a release adds comes after the ones it had
pub fn key_order(text: &str) -> Vec<String> {
    keys_at(text, 2)
}

/// The keys printed `indent` spaces in (2 for the top level, 6 for the keys of
/// the objects in a list at the top level), in the order they were printed
pub fn keys_at(text: &str, indent: usize) -> Vec<String> {
    let margin = format!("{}\"", " ".repeat(indent));
    text.lines()
        .filter_map(|line| line.strip_prefix(margin.as_str()))
        .filter_map(|rest| rest.split('"').next())
        .map(str::to_string)
        .collect()
}

/// Panic unless the document keeps the contract
pub fn check(json: &Value, code: i32, args: &[&str]) {
    if let Some(wrong) = violation(json, code) {
        panic!("proc {args:?} broke the JSON contract: {wrong}\n{json:#}");
    }
}
