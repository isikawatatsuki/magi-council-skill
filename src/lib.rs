pub mod adversarial;
pub mod commands;
pub mod core;
pub mod hooks;
pub mod lifecycle;

use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::cmp::Ordering;

fn compare_javascript_keys(left: &str, right: &str) -> Ordering {
    left.encode_utf16().cmp(right.encode_utf16())
}

pub fn canonicalize(value: &Value) -> Value {
    match value {
        Value::Array(items) => Value::Array(items.iter().map(canonicalize).collect()),
        Value::Object(object) => {
            let mut keys: Vec<&String> = object.keys().collect();
            keys.sort_by(|left, right| compare_javascript_keys(left, right));
            let mut sorted = Map::with_capacity(object.len());
            for key in keys {
                sorted.insert(key.clone(), canonicalize(&object[key]));
            }
            Value::Object(sorted)
        }
        _ => value.clone(),
    }
}

pub fn canonical_json(value: &Value) -> serde_json::Result<String> {
    serde_json_canonicalizer::to_string(value)
}

pub fn sha256_text(value: &str) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))
}

pub fn sha256_value(value: &Value) -> serde_json::Result<String> {
    canonical_json(value).map(|json| sha256_text(&json))
}

pub fn hash_request(request: &Value) -> serde_json::Result<String> {
    let mut immutable = request.clone();
    if let Value::Object(object) = &mut immutable {
        object.shift_remove("status");
    }
    sha256_value(&immutable)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn canonical_json_sorts_keys_recursively() {
        let value = json!({"z": {"b": 2, "a": 1}, "a": [2, {"d": 4, "c": 3}]});
        assert_eq!(
            canonical_json(&value).unwrap(),
            r#"{"a":[2,{"c":3,"d":4}],"z":{"a":1,"b":2}}"#
        );
    }

    #[test]
    fn canonical_hash_matches_node_reference() {
        let value = json!({"b": 2, "a": 1});
        assert_eq!(
            sha256_value(&value).unwrap(),
            "43258cff783fe7036d8a43033f830adfc60ec037382473548ac742b888292777"
        );
    }

    #[test]
    fn canonical_numbers_match_javascript_reference() {
        let value: Value =
            serde_json::from_str(r#"{"values":[1.0,-0.0,1e20,1e21,1e-7,333333333.33333329]}"#)
                .unwrap();
        assert_eq!(
            canonical_json(&value).unwrap(),
            r#"{"values":[1,0,100000000000000000000,1e+21,1e-7,333333333.3333333]}"#
        );
        assert_eq!(
            sha256_value(&value).unwrap(),
            "ce89416c66b33c1a2433e84cd928bf73f2493a9bbee6df9e3ba2a6968b80c426"
        );
    }

    #[test]
    fn request_hash_ignores_mutable_status() {
        let collecting = json!({"runId": "magi-example", "status": "collecting"});
        let finalized = json!({"runId": "magi-example", "status": "finalized"});
        assert_eq!(
            hash_request(&collecting).unwrap(),
            hash_request(&finalized).unwrap()
        );
    }
}
