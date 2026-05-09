//! `pptx_edit` — structured PowerPoint inspection + editing tool.
//!
//! Wraps a small Python helper (`pptx_edit.py`, embedded via `include_str!`)
//! that uses python-pptx to do real edits server-side. The LLM emits a
//! structured operation list as JSON; we shell out to python3 and pipe
//! the spec via stdin. This replaces the old workflow where the agent
//! had to write a multi-line python script via the `bash` tool — that
//! pattern was unreliable on some upstream providers (the model would
//! return an empty completion mid-script-generation).
//!
//! Operations supported by the Python helper (must match `OP_REGISTRY`):
//!   - `replace_text`        — global or per-slide find/replace
//!   - `set_slide_title`     — set the title placeholder text
//!   - `set_paragraph_text`  — set one paragraph in a shape
//!   - `add_bullet`          — append a bullet to a text frame
//!   - `delete_slide`        — remove a slide by index
//!   - `add_slide`           — add a new slide (layout, title, body bullets)
//!   - `reorder_slides`      — reorder by index permutation
//!   - `set_font_size`       — set Pt size on runs
//!   - `set_font_color`      — set RGB hex color on runs
//!
//! Inspect-only mode (no `operations`, no `output_path`) returns the
//! full slide structure so the agent can plan edits without running
//! its own Python.

use async_trait::async_trait;
use serde_json::json;
use std::process::Stdio;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;
use tokio::time::{timeout, Duration};
use tracing::warn;

use microclaw_core::llm_types::ToolDefinition;

use super::{schema_object, Tool, ToolResult};

const PPTX_HELPER_PY: &str = include_str!("pptx_edit.py");
const EXEC_TIMEOUT_SECS: u64 = 60;

pub struct PptxEditTool;

impl PptxEditTool {
    pub fn new() -> Self {
        Self
    }
}

impl Default for PptxEditTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for PptxEditTool {
    fn name(&self) -> &str {
        "pptx_edit"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().into(),
            description: r#"Inspect or edit a PowerPoint (.pptx) file using structured operations — preferred over writing python-pptx scripts in bash.

Two modes:

INSPECT (read-only): pass only `source_path`. Returns the deck structure: slide count, per-slide title + layout + shape summary + paragraph text. Use this first to plan edits.

APPLY: pass `source_path`, `output_path`, and a non-empty `operations` array. Each operation is applied in order; the modified deck is saved to `output_path`. Returns per-op success/failure plus a final ok flag.

Operations (give one as `op` in each operation object):
- `replace_text`        : { find, replace, slide?: int, case_sensitive?: bool } — find/replace across all (or one) slide
- `set_slide_title`     : { slide: int, title: str } — set the title placeholder
- `set_paragraph_text`  : { slide: int, shape: int, paragraph: int, text: str } — overwrite one paragraph
- `add_bullet`          : { slide: int, shape: int, text: str, level?: int } — append bullet to text frame
- `delete_slide`        : { slide: int } — remove a slide by index
- `add_slide`           : { layout?: int, after_slide?: int, title?: str, body?: [str] } — append new slide
- `reorder_slides`      : { order: [int] } — full permutation of current indices
- `set_font_size`       : { slide: int, shape: int, paragraph?: int, size: int } — Pt size on runs
- `set_font_color`      : { slide: int, shape: int, paragraph?: int, rgb: "RRGGBB" } — hex color on runs

All slide/shape/paragraph indices are 0-based. If `paragraph` is omitted on font ops, applies to every paragraph in the shape.

Paths must be absolute or relative to the chat workspace (e.g. `attachments/in.pptx` works). Always inspect first to find the right slide/shape/paragraph indices before editing."#.into(),
            input_schema: schema_object(
                json!({
                    "source_path": {
                        "type": "string",
                        "description": "Path to the input .pptx file. Required."
                    },
                    "output_path": {
                        "type": "string",
                        "description": "Where to write the edited .pptx. Required when `operations` is non-empty. Omit for inspect-only."
                    },
                    "operations": {
                        "type": "array",
                        "description": "Ordered list of operations to apply. Empty/absent = inspect only.",
                        "items": {
                            "type": "object",
                            "properties": {
                                "op": {
                                    "type": "string",
                                    "enum": [
                                        "replace_text",
                                        "set_slide_title",
                                        "set_paragraph_text",
                                        "add_bullet",
                                        "delete_slide",
                                        "add_slide",
                                        "reorder_slides",
                                        "set_font_size",
                                        "set_font_color"
                                    ]
                                }
                            },
                            "required": ["op"]
                        }
                    }
                }),
                &["source_path"],
            ),
        }
    }

    async fn execute(&self, input: serde_json::Value) -> ToolResult {
        let source_path = match input.get("source_path").and_then(|v| v.as_str()) {
            Some(s) => s.to_string(),
            None => return ToolResult::error("Missing required parameter: source_path".into()),
        };
        let output_path = input
            .get("output_path")
            .and_then(|v| v.as_str())
            .map(str::to_string);
        let operations = input
            .get("operations")
            .cloned()
            .unwrap_or_else(|| serde_json::Value::Array(vec![]));

        // Build the spec we'll pipe to python via stdin.
        let mut spec = json!({
            "source_path": source_path,
            "operations": operations,
        });
        if let Some(out) = output_path {
            spec["output_path"] = serde_json::Value::String(out);
        }
        let spec_bytes = match serde_json::to_vec(&spec) {
            Ok(b) => b,
            Err(e) => {
                return ToolResult::error(format!("failed to serialise spec: {e}"))
                    .with_error_type("serialize_error");
            }
        };

        // Spawn python3 with the embedded helper passed via `-c` would be
        // unwieldy at this size; pipe the script via stdin alongside a
        // sentinel won't work either. Instead: pass the script via a
        // python3 invocation that reads it from a heredoc-style stdin
        // — but we need stdin for the spec. So we drop the script into
        // a stable cache path on first call (idempotent) and exec it.
        let script_path = match ensure_helper_on_disk() {
            Ok(p) => p,
            Err(e) => {
                return ToolResult::error(format!("failed to stage pptx helper: {e}"))
                    .with_error_type("stage_error");
            }
        };

        let mut child = match Command::new("python3")
            .arg(&script_path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
        {
            Ok(c) => c,
            Err(e) => {
                return ToolResult::error(format!("failed to spawn python3: {e}"))
                    .with_error_type("spawn_error");
            }
        };

        if let Some(mut stdin) = child.stdin.take() {
            if let Err(e) = stdin.write_all(&spec_bytes).await {
                warn!("pptx_edit: failed to write spec to python stdin: {e}");
            }
            // dropping stdin here closes it so python's sys.stdin.read() returns
        }

        let output_fut = child.wait_with_output();
        let output = match timeout(Duration::from_secs(EXEC_TIMEOUT_SECS), output_fut).await {
            Ok(Ok(o)) => o,
            Ok(Err(e)) => {
                return ToolResult::error(format!("python3 wait failed: {e}"))
                    .with_error_type("wait_error");
            }
            Err(_) => {
                return ToolResult::error(format!(
                    "pptx_edit timed out after {EXEC_TIMEOUT_SECS}s"
                ))
                .with_error_type("timeout");
            }
        };

        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();

        if !output.status.success() {
            // Helper wrote JSON to stdout even on most errors; surface it
            // alongside stderr so the caller sees both.
            let mut msg = stdout;
            if !stderr.trim().is_empty() {
                msg.push_str("\n[stderr]\n");
                msg.push_str(stderr.trim());
            }
            return ToolResult::error(msg)
                .with_error_type("pptx_helper_error")
                .with_status_code(output.status.code().unwrap_or(1));
        }

        // Truncate very long inspect output (large decks dump full paragraph text).
        let mut text = stdout;
        if text.len() > 40_000 {
            text.truncate(40_000);
            text.push_str("\n... (truncated)");
        }
        ToolResult::success(text)
    }
}

/// Stage the embedded Python helper to a stable cache path so successive
/// invocations don't reparse it. We rewrite if missing or stale.
fn ensure_helper_on_disk() -> std::io::Result<std::path::PathBuf> {
    use std::fs;
    use std::io::Write;
    let dir = std::env::temp_dir().join("microclaw-helpers");
    fs::create_dir_all(&dir)?;
    let path = dir.join("pptx_edit.py");
    let needs_write = match fs::read_to_string(&path) {
        Ok(existing) => existing != PPTX_HELPER_PY,
        Err(_) => true,
    };
    if needs_write {
        let mut f = fs::File::create(&path)?;
        f.write_all(PPTX_HELPER_PY.as_bytes())?;
    }
    Ok(path)
}
