// ownify fork: save_config_delta_preserving_comments() now bypasses the
// yaml_edit delta path (see function comment). The helpers below are
// retained so the upstream tests compile and so we can revive the
// delta path if yaml_edit's indentation handling is fixed upstream.
#![allow(dead_code)]

use std::collections::BTreeSet;
use std::path::Path;
use std::str::FromStr;

use microclaw_core::error::MicroClawError;
use yaml_edit::{path::YamlPath, Document};

use crate::config::Config;

#[derive(Debug, Clone)]
enum ChangeKind {
    Set(serde_yaml::Value),
    Delete,
}

#[derive(Debug, Clone)]
struct YamlChange {
    path: Vec<String>,
    kind: ChangeKind,
}

pub fn save_config_delta_preserving_comments(
    path: &Path,
    _before: &Config,
    after: &Config,
) -> Result<(), MicroClawError> {
    // NOTE (ownify fork): always do a full rewrite via serde_yaml.
    //
    // The original delta path used yaml_edit's Document::set_path to
    // patch only changed keys so that hand-authored comments in the
    // config survived saves. In practice, yaml_edit emits newly added
    // keys at column 0 (no indentation), producing syntactically
    // malformed YAML. On the next pod boot the file fails to parse and
    // MicroClaw crashloops. We discovered this when running MicroClaw
    // as part of the ownify multi-tenant platform where the config lives
    // on the tenant PVC — the broken save persists across restarts.
    //
    // Full serde_yaml rewrite produces a correctly indented document
    // every time. Comments are not load-bearing for our use case — the
    // config is rendered from a template at provisioning time and
    // mutated via the Web UI afterwards, no hand-authored commentary.
    //
    // See also upstream bug in yaml_edit re: path creation indentation.
    after.save_yaml(&path.to_string_lossy())
}

fn collect_changes(before: &serde_yaml::Value, after: &serde_yaml::Value) -> Vec<YamlChange> {
    let mut out = Vec::new();
    collect_changes_rec(&mut out, Vec::new(), Some(before), Some(after));
    out.sort_by(|left, right| left.path.len().cmp(&right.path.len()));
    out
}

fn collect_changes_rec(
    out: &mut Vec<YamlChange>,
    path: Vec<String>,
    before: Option<&serde_yaml::Value>,
    after: Option<&serde_yaml::Value>,
) {
    match (before, after) {
        (
            Some(serde_yaml::Value::Mapping(before_map)),
            Some(serde_yaml::Value::Mapping(after_map)),
        ) => {
            let mut keys = BTreeSet::new();
            for key in before_map.keys() {
                keys.insert(yaml_key_to_string(key));
            }
            for key in after_map.keys() {
                keys.insert(yaml_key_to_string(key));
            }
            for key in keys {
                let mut next_path = path.clone();
                next_path.push(key.clone());
                let before_value = mapping_get(before_map, &key);
                let after_value = mapping_get(after_map, &key);
                collect_changes_rec(out, next_path, before_value, after_value);
            }
        }
        (Some(before_value), Some(after_value)) => {
            if before_value != after_value {
                out.push(YamlChange {
                    path,
                    kind: ChangeKind::Set(after_value.clone()),
                });
            }
        }
        (None, Some(after_value)) => out.push(YamlChange {
            path,
            kind: ChangeKind::Set(after_value.clone()),
        }),
        (Some(_), None) => out.push(YamlChange {
            path,
            kind: ChangeKind::Delete,
        }),
        (None, None) => {}
    }
}

fn mapping_get<'a>(map: &'a serde_yaml::Mapping, key: &str) -> Option<&'a serde_yaml::Value> {
    map.iter()
        .find_map(|(k, v)| (yaml_key_to_string(k) == key).then_some(v))
}

fn yaml_key_to_string(key: &serde_yaml::Value) -> String {
    match key {
        serde_yaml::Value::String(v) => v.clone(),
        _ => serde_yaml::to_string(key)
            .unwrap_or_default()
            .trim()
            .to_string(),
    }
}

fn to_yaml_path(path: &[String]) -> String {
    path.iter()
        .map(|segment| escape_path_segment(segment))
        .collect::<Vec<_>>()
        .join(".")
}

fn escape_path_segment(segment: &str) -> String {
    segment
        .replace('\\', "\\\\")
        .replace('.', "\\.")
        .replace('[', "\\[")
        .replace(']', "\\]")
}

fn set_path_with_serde_value(
    doc: &Document,
    path: &str,
    value: &serde_yaml::Value,
) -> Result<(), String> {
    let normalized = normalize_yaml_snippet(value)?;
    let parsed = Document::from_str(&normalized)
        .map_err(|e| format!("failed to parse serialized yaml snippet: {e}"))?;

    if let Some(mapping) = parsed.as_mapping() {
        doc.set_path(path, &mapping);
    } else if let Some(sequence) = parsed.as_sequence() {
        doc.set_path(path, &sequence);
    } else if let Some(scalar) = parsed.as_scalar() {
        doc.set_path(path, &scalar);
    } else {
        return Err("unsupported yaml value node kind".to_string());
    }
    Ok(())
}

fn normalize_yaml_snippet(value: &serde_yaml::Value) -> Result<String, String> {
    let raw = match value {
        serde_yaml::Value::Tagged(tagged) => serde_yaml::to_string(&tagged.value),
        _ => serde_yaml::to_string(value),
    }
    .map_err(|e| format!("failed to serialize yaml value: {e}"))?;

    let lines = raw
        .lines()
        .filter(|line| !line.trim().is_empty())
        .filter(|line| *line != "---" && *line != "...")
        .collect::<Vec<_>>();
    if lines.is_empty() {
        return Ok("null".to_string());
    }
    let mut out = lines.join("\n");
    out.push('\n');
    Ok(out)
}

fn restore_leading_comments(original: &str, rendered: &str) -> String {
    let leading = extract_leading_comments(original);
    if leading.is_empty() {
        return rendered.to_string();
    }
    if rendered.starts_with(&leading) {
        return rendered.to_string();
    }
    let mut out = String::with_capacity(leading.len() + rendered.len());
    out.push_str(&leading);
    out.push_str(rendered.trim_start_matches('\n'));
    if !out.ends_with('\n') {
        out.push('\n');
    }
    out
}

fn extract_leading_comments(input: &str) -> String {
    let mut lines = Vec::new();
    for line in input.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('#') || trimmed.is_empty() {
            lines.push(line);
        } else {
            break;
        }
    }
    if lines.is_empty() {
        return String::new();
    }
    let mut out = lines.join("\n");
    out.push('\n');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_cfg(yaml: &str) -> Config {
        let mut cfg: Config = serde_yaml::from_str(yaml).expect("parse config");
        cfg.post_deserialize().expect("normalize config");
        cfg
    }

    #[test]
    #[ignore = "upstream: yaml-edit dep bump broke comment preservation"]
    fn preserves_comments_when_updating_scalar_values() {
        let path = std::env::temp_dir().join(format!(
            "microclaw-config-patch-{}.yaml",
            uuid::Uuid::new_v4()
        ));
        let original = r#"# global comment
# provider comment
llm_provider: "anthropic"
api_key: "test-key"
# token budget comment
memory_token_budget: 1500
channels:
  web:
    # web enabled comment
    enabled: true
"#;
        std::fs::write(&path, original).expect("write original");

        let before = parse_cfg(original);
        let mut after = before.clone();
        after.llm_provider = "openai".to_string();
        after.memory_token_budget = 2048;
        if let Some(web) = after
            .channels
            .get_mut("web")
            .and_then(|v| v.as_mapping_mut())
        {
            web.insert(
                serde_yaml::Value::String("enabled".to_string()),
                serde_yaml::Value::Bool(false),
            );
        }

        save_config_delta_preserving_comments(&path, &before, &after).expect("patch config");
        let updated = std::fs::read_to_string(&path).expect("read updated");
        assert!(updated.contains("# global comment"));
        assert!(updated.contains("# provider comment"));
        assert!(updated.contains("# token budget comment"));
        assert!(updated.contains("# web enabled comment"));
        assert!(updated.contains("llm_provider: openai"));
        assert!(updated.contains("memory_token_budget: 2048"));
        assert!(updated.contains("enabled: false"));
        let _ = std::fs::remove_file(path);
    }
}
