//! Image generation tool for the ownify agent.
//!
//! Lets the agent generate images via an OpenAI-compatible image generation
//! endpoint (OpenRouter, OpenAI, Together, Replicate, Stability, custom).
//!
//! The agent uses the image URL returned by this tool, then sends it as a
//! Matrix attachment (m.image) via send_message, or saves it locally and
//! uploads to Nextcloud via the existing Nextcloud tool.
//!
//! Configuration (read at agent startup from env vars, set by the
//! ownify-control-plane dashboard):
//!   IMAGE_PROVIDER    - provider name (e.g. "openrouter", "openai")
//!   IMAGE_API_KEY     - API key for the provider
//!   IMAGE_API_URL     - base URL (e.g. "https://openrouter.ai/api/v1")
//!   IMAGE_MODEL       - default model (e.g. "black-forest-labs/FLUX-1.1-pro")
//!   IMAGE_DEFAULT_SIZE - default size, e.g. "1024x1024"
//!
//! Provider-specific behaviour:
//! - `openrouter` (case-insensitive substring match): uses the chat-completions
//!   endpoint at `<IMAGE_API_URL>/chat/completions` with `modalities: ["image"]`.
//!   This is what OpenRouter actually implements — it has no
//!   `/v1/images/generations` route. Image-only models (e.g. `black-forest-labs/flux.2-pro`)
//!   and text+image models (e.g. `google/gemini-2.5-flash-image`) both work with
//!   the `["image"]` shape; text+image models simply omit the text in the response.
//! - everything else (openai, together, replicate, stability, custom, empty):
//!   uses the OpenAI-style `/v1/images/generations` endpoint, unchanged.

use std::path::{Path, PathBuf};
use std::time::Duration;

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use tracing::{info, warn};

use microclaw_core::llm_types::ToolDefinition;
use microclaw_tools::runtime::{
    resolve_tool_working_dir, Tool, ToolResult,
};

use crate::config::{Config, WorkingDirIsolation};

// ── image_gen ─────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct ImageGenInput {
    /// The text description of the image to generate.
    pub prompt: String,
    /// Optional: image size (e.g. "1024x1024", "1024x1792"). Defaults to
    /// IMAGE_DEFAULT_SIZE env var, or "1024x1024".
    #[serde(default)]
    pub size: Option<String>,
    /// Optional: number of images to generate (most providers return 1
    /// by default; some accept n=2..4).
    #[serde(default)]
    pub n: Option<u32>,
    /// Optional: override the default model for this call.
    #[serde(default)]
    pub model: Option<String>,
    /// Optional: negative prompt (some providers like Stability support this).
    #[serde(default)]
    pub negative_prompt: Option<String>,
    /// Optional: where to save the image bytes locally. If set, the image
    /// is downloaded from the provider URL and saved to this path. The
    /// returned tool result includes both the original provider URL and
    /// the local file path. Useful for the agent to use the image for
    /// webpages (Nextcloud upload, embedded in generated HTML, etc.).
    #[serde(default)]
    pub save_path: Option<String>,
}

pub struct ImageGenTool {
    image_api_url: String,
    image_api_key: String,
    image_model: String,
    image_default_size: String,
    image_provider: String,
    /// Base working directory. When `working_dir_isolation: "chat"` is set,
    /// the actual save dir is `<working_dir>/<channel>/<chat_id>/image_gen/`
    /// — that way the LLM doesn't have to think about chat-specific paths
    /// and the file ends up somewhere the chat-isolated send_message tool
    /// can find it without absolute paths.
    working_dir: PathBuf,
    working_dir_isolation: WorkingDirIsolation,
    http: reqwest::Client,
}

impl ImageGenTool {
    pub fn new(config: &Config) -> Self {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(120))
            .build()
            .expect("reqwest client builder should not fail");
        Self {
            image_api_url: config.image_api_url.clone(),
            image_api_key: config.image_api_key.clone(),
            image_model: config.image_model.clone(),
            image_default_size: config.image_default_size.clone(),
            image_provider: config.image_provider.clone(),
            working_dir: PathBuf::from(&config.working_dir),
            working_dir_isolation: config.working_dir_isolation,
            http,
        }
    }
}

#[async_trait]
impl Tool for ImageGenTool {
    fn name(&self) -> &str { "image_gen" }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "image_gen".into(),
            description: "Generate an image from a text prompt using the configured image generation provider (OpenRouter, OpenAI, Together, Replicate, Stability, etc.). The image is ALWAYS auto-saved to disk; the tool never returns the raw base64 to keep LLM context small. Just call with `prompt` (and optional `size`/`model`/`n`); the tool result includes the absolute `saved_path`. To send the image as a Matrix attachment, call `send_message` with `attachment_path=<saved_path>`. Only set `save_path` if you need the file at a specific location; otherwise omit it and use the returned `saved_path` directly. If you must use `save_path`, give an absolute path (relative paths may fail with permission errors depending on the chat's working-dir isolation).".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "prompt": {
                        "type": "string",
                        "description": "Detailed text description of the image to generate. Be specific: subject, style, lighting, composition, colors."
                    },
                    "size": {
                        "type": "string",
                        "description": "Image dimensions: '1024x1024' (square), '1024x1792' (portrait), '1792x1024' (landscape), '512x512', or '256x256'. Defaults to IMAGE_DEFAULT_SIZE env var or 1024x1024."
                    },
                    "n": {
                        "type": "integer",
                        "description": "Number of images to generate (1-4). Defaults to 1."
                    },
                    "model": {
                        "type": "string",
                        "description": "Override the default image model for this call. Useful when the user requests a specific model."
                    },
                    "negative_prompt": {
                        "type": "string",
                        "description": "Optional negative prompt (Stability, some FLUX models): things to AVOID in the image."
                    },
                    "save_path": {
                        "type": "string",
                        "description": "OPTIONAL. Absolute path to write the image to. If relative, it's resolved against the chat-isolated working dir. If omitted, the image is auto-saved to the chat-isolated working dir under 'image_gen/' and the path is returned in the result. Prefer omitting this and using the returned saved_path — relative paths have caused real permission errors in the past."
                    }
                },
                "required": ["prompt"]
            }),
        }
    }

    async fn execute(&self, input: Value) -> ToolResult {
        // We need the raw `&Value` for chat-isolated working-dir resolution
        // (it reads the `__microclaw_auth` injection from the input). So
        // parse into a clone, leaving the original `input` available.
        let parsed: ImageGenInput = match serde_json::from_value(input.clone()) {
            Ok(v) => v,
            Err(e) => return ToolResult::error(format!("Invalid input: {e}")),
        };

        if self.image_api_url.is_empty() {
            return ToolResult::error(
                "Image generation is not configured. Ask the user to set IMAGE_PROVIDER, IMAGE_API_URL, IMAGE_API_KEY, and IMAGE_MODEL in the dashboard under 'Image generation'.".to_string()
            ).with_error_type("not_configured");
        }
        if self.image_api_key.is_empty() && self.image_provider != "ollama" {
            return ToolResult::error(
                "IMAGE_API_KEY is not set. Ask the user to add their API key in the dashboard under 'Image generation'.".to_string()
            ).with_error_type("not_configured");
        }

        let model = parsed.model.unwrap_or_else(|| self.image_model.clone());
        if model.is_empty() {
            return ToolResult::error("No image model configured. Set IMAGE_MODEL or pass model=...".to_string()).with_error_type("not_configured");
        }
        let size = parsed.size.unwrap_or_else(|| self.image_default_size.clone());
        let n = parsed.n.unwrap_or(1);

        info!(
            provider = %self.image_provider,
            model = %model,
            size = %size,
            n = n,
            prompt_chars = parsed.prompt.len(),
            "image_gen: dispatching request"
        );

        // Dispatch by provider. OpenRouter uses a different endpoint + body
        // shape than the OpenAI-style /v1/images/generations. Detection is a
        // case-insensitive substring match on the provider name so that
        // "openrouter", "OpenRouter", "openrouter-prod" all work.
        let is_openrouter = self.image_provider.eq_ignore_ascii_case("openrouter")
            || self.image_provider.to_lowercase().contains("openrouter");

        let (url, body) = if is_openrouter {
            // OpenRouter image generation: chat completions with image-only
            // output modality. Per OpenRouter's per-model API docs (e.g.
            // https://openrouter.ai/black-forest-labs/flux.2-pro/api),
            // image-only models require `modalities: ["image"]` (no text).
            // This shape also works for text+image models like
            // google/gemini-2.5-flash-image — they simply omit the text
            // response, which is what callers of an image-generation tool
            // want anyway. The response image comes back in
            // `choices[0].message.images[].image_url.url` (handled below).
            //
            // Earlier v0.1.66 used `modalities: ["text", "image"]` which
            // works for gemini-2.5-flash-image but fails 404 on image-only
            // models like flux.2-pro because they have no endpoint that
            // produces both text and image output.
            let url = format!("{}/chat/completions", self.image_api_url.trim_end_matches('/'));
            let body = json!({
                "model": model,
                "modalities": ["image"],
                "messages": [
                    {
                        "role": "user",
                        "content": parsed.prompt
                    }
                ]
            });
            (url, body)
        } else {
            // OpenAI-style /v1/images/generations (works for OpenAI direct,
            // Together, Replicate, Stability, and other OpenAI-compatible
            // providers).
            let url = format!("{}/images/generations", self.image_api_url.trim_end_matches('/'));
            let mut body = json!({
                "model": model,
                "prompt": parsed.prompt,
                "size": size,
                "n": n,
                "response_format": "url",
            });
            if let Some(np) = &parsed.negative_prompt {
                body["negative_prompt"] = json!(np);
            }
            (url, body)
        };

        let mut req = self.http.post(&url)
            .header("Content-Type", "application/json");
        if !self.image_api_key.is_empty() {
            req = req.bearer_auth(&self.image_api_key);
        }

        let resp = match req.json(&body).send().await {
            Ok(r) => r,
            Err(e) => return ToolResult::error(format!("image_gen request failed: {e}")).with_error_type("network_error"),
        };

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            warn!(provider = %self.image_provider, status = %status, body = %body, "image_gen non-2xx");
            return ToolResult::error(format!("image_gen returned {status}: {body}")).with_error_type("provider_error");
        }

        let payload: Value = match resp.json().await {
            Ok(v) => v,
            Err(e) => return ToolResult::error(format!("image_gen bad response: {e}")).with_error_type("parse_error"),
        };

        // Provider-specific response parsing.
        let urls: Vec<String> = if is_openrouter {
            // OpenRouter returns a chat-completions shape. The image can be in
            // several places depending on the model. Walk the response looking
            // for any URL or data: URI.
            //
            // Known shapes seen in the wild:
            //   1. { choices: [ { message: { images: [ { image_url: { url: "data:image/png;base64,..." } } ] } } ] }
            //   2. { choices: [ { message: { content: [ { type: "image_url", image_url: { url: "data:..." } } ] } } ] }
            //   3. { choices: [ { message: { content: "data:image/png;base64,..." } } ] }   (plain string)
            //   4. { choices: [ { message: { content: "https://..." } } ] }                 (URL string)
            //
            // Extract from "images" array first (variants 1 and 2 share the
            // "image_url.url" path), then fall back to scanning "content" for
            // any string that looks like a URL or data URI.
            let mut found: Vec<String> = Vec::new();

            if let Some(choices) = payload.get("choices").and_then(|c| c.as_array()) {
                for choice in choices {
                    let msg = match choice.get("message") {
                        Some(m) => m,
                        None => continue,
                    };

                    // 1. message.images[].image_url.url
                    if let Some(images) = msg.get("images").and_then(|i| i.as_array()) {
                        for img in images {
                            if let Some(url) = img
                                .get("image_url")
                                .and_then(|u| u.get("url"))
                                .and_then(|u| u.as_str())
                            {
                                found.push(url.to_string());
                            }
                        }
                    }

                    // 2. message.content as array of parts
                    if let Some(parts) = msg.get("content").and_then(|c| c.as_array()) {
                        for part in parts {
                            if let Some(url) = part
                                .get("image_url")
                                .and_then(|u| u.get("url"))
                                .and_then(|u| u.as_str())
                            {
                                found.push(url.to_string());
                            }
                        }
                    }

                    // 3. message.content as plain string (URL or data URI)
                    if let Some(s) = msg.get("content").and_then(|c| c.as_str()) {
                        if s.starts_with("data:image/") || s.starts_with("http://") || s.starts_with("https://") {
                            found.push(s.to_string());
                        }
                    }
                }
            }

            // De-dupe while preserving order.
            let mut seen = std::collections::HashSet::new();
            found.retain(|u| seen.insert(u.clone()));

            if found.is_empty() {
                return ToolResult::error(format!("image_gen (openrouter) returned no images in response: {payload}"))
                    .with_error_type("empty_response");
            }
            found
        } else {
            // OpenAI format: { "data": [ { "url": "...", "b64_json": "..." }, ... ] }
            let images: Vec<&Value> = payload.get("data")
                .and_then(|d| d.as_array())
                .map(|a| a.iter().collect())
                .unwrap_or_default();

            if images.is_empty() {
                return ToolResult::error(format!("image_gen returned no images: {payload}"))
                    .with_error_type("empty_response");
            }

            images.iter()
                .filter_map(|img| img.get("url").and_then(|u| u.as_str()).map(String::from))
                .collect()
        };

        // Decide where to write the first image. We always write to disk so
        // the LLM tool result can be small (path only) — returning the raw
        // data: URI back to the LLM would bloat its conversation history
        // with hundreds of KB per image, and after a few iterations the
        // context would exceed the model's max length.
        //
        // Chat-isolated working dir: when `working_dir_isolation: "chat"`
        // is set, the auto-save dir is `<working_dir>/<channel>/<chat_id>/image_gen/`.
        // The LLM can then use the returned absolute path directly in
        // `send_message attachment_path=...` without having to know the
        // chat-specific dir.
        //
        // Note: the chat-isolated working dir is something like
        // `/home/microclaw/.microclaw/workspace/chat/matrix/1/tmp`. The
        // bot's CWD when calling bash/send_message is exactly this dir.
        // The LLM naturally thinks in terms of "current dir" and writes
        // relative paths like `tmp/red_apple.png` meaning "the tmp/ I'm
        // already in" — but resolve_tool_path() would join them and get
        // `.../tmp/tmp/red_apple.png` (one tmp/ too many). We work around
        // this by stripping a single leading `tmp/` segment if the chat
        // dir already ends in `tmp`.
        //
        // Priority:
        //  1. If the caller provided an explicit save_path (absolute or
        //     relative-to-chat-dir), use that.
        //  2. Otherwise, if the URL is a data: URI, decode and write it to
        //     <chat-dir>/image_gen/image_<unix_ts>.<ext>.
        //  3. Otherwise (http/https URL), don't auto-download.
        let saved_path: Option<Result<String, String>> = if let Some(save_path) = &parsed.save_path {
            if let Some(first_url) = urls.first() {
                let chat_dir = resolve_tool_working_dir(
                    &self.working_dir,
                    self.working_dir_isolation,
                    &input,
                );
                // Absolute path → use as-is. Relative → resolve against
                // chat-isolated working dir, with a workaround for the
                // common "tmp/foo.png" pattern that LLM uses (strip a
                // leading tmp/ segment if the chat dir already ends in tmp).
                let resolved = if Path::new(save_path).is_absolute() {
                    PathBuf::from(save_path)
                } else {
                    let path = Path::new(save_path);
                    let strip_leading_tmp = chat_dir
                        .file_name()
                        .map(|n| n == "tmp")
                        .unwrap_or(false)
                        && path
                            .components()
                            .next()
                            .and_then(|c| c.as_os_str().to_str())
                            .map(|s| s == "tmp")
                            .unwrap_or(false);
                    if strip_leading_tmp {
                        // Skip the leading "tmp/" component.
                        let stripped = path
                            .components()
                            .skip(1)
                            .collect::<std::path::PathBuf>();
                        chat_dir.join(stripped)
                    } else {
                        chat_dir.join(path)
                    }
                };
                let resolved_str = resolved.to_string_lossy().to_string();
                Some(self.persist_first_image(first_url, &resolved_str, &model).await)
            } else {
                None
            }
        } else if let Some(first_url) = urls.first() {
            if let Some(stripped) = first_url.strip_prefix("data:") {
                // Auto-save data: URIs so the LLM never sees the raw bytes.
                let ext = detect_image_extension_from_data_uri(stripped);
                let ts = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                let safe_model: String = model
                    .chars()
                    .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
                    .collect();
                let chat_dir = resolve_tool_working_dir(
                    &self.working_dir,
                    self.working_dir_isolation,
                    &input,
                );
                let image_gen_subdir = chat_dir.join("image_gen");
                // Best-effort create the dir; we surface write failure to
                // the LLM, but we want to be sure the dir exists first.
                if let Err(e) = std::fs::create_dir_all(&image_gen_subdir) {
                    warn!(
                        error = %e,
                        dir = %image_gen_subdir.display(),
                        "image_gen: failed to create image_gen subdir"
                    );
                }
                let default_path = image_gen_subdir.join(
                    format!("image_{}_{}.{}", safe_model, ts, ext)
                );
                let default_path_str = default_path.to_string_lossy().to_string();
                Some(self.write_data_uri_to_path(stripped, &default_path_str, &model))
            } else {
                // Real http/https URL — don't auto-download; just report it.
                None
            }
        } else {
            None
        };

        // Propagate any save error to the LLM. The previous implementation
        // returned the dest_path even on write failure, which made the LLM
        // (and the user) believe the file existed when it didn't. Now the
        // LLM sees the actual error and can retry with a different path.
        let saved_path: Option<String> = match saved_path {
            Some(Ok(p)) => Some(p),
            Some(Err(e)) => return ToolResult::error(format!(
                "image_gen: failed to save image to disk: {e}. \
                 Try again with an absolute save_path in /tmp/ or /home/microclaw/.microclaw/workspace/."
            )),
            None => None,
        };

        // Build tool result. CRITICAL: do NOT include the raw data: URI in
        // image_urls when it's a multi-KB data URI — that bloats the LLM
        // context. Always prefer to return the saved_path. For small HTTP
        // URLs we still include them (they're <200 chars typically).
        let first_url_is_data = urls
            .first()
            .map(|u| u.starts_with("data:"))
            .unwrap_or(false);
        let urls_for_result: Vec<String> = if first_url_is_data {
            // Don't echo the data URI back to the LLM. If we couldn't
            // auto-save, return a short placeholder so the LLM knows the
            // image exists but the bytes are not in this response.
            if saved_path.is_some() {
                vec![format!("[saved to {}. Image bytes are on disk, see saved_path field.]",
                    saved_path.as_deref().unwrap_or("<unknown>"))]
            } else {
                vec!["[data:image/...; image bytes omitted from tool result to keep LLM context small; see image_data_chars below for the size.]".to_string()]
            }
        } else {
            urls.clone()
        };

        let data_uri_char_count: Option<usize> = if first_url_is_data {
            urls.first().map(|u| u.len())
        } else {
            None
        };

        let result_json = json!({
            "ok": true,
            "provider": self.image_provider,
            "model": model,
            "size": size,
            "image_urls": urls_for_result,
            "saved_path": saved_path,
            "image_data_chars": data_uri_char_count,
            "next_steps": if saved_path.is_some() {
                "Image saved locally. To send as a Matrix attachment, use send_message with attachment_path=<saved_path>."
            } else if first_url_is_data {
                "Image bytes were large and have been kept off the LLM context. If you need the image delivered, retry with save_path=/some/path/img.png so it is written to disk."
            } else {
                "To send as a Matrix attachment, use send_message with image_url=<url>."
            }
        });

        ToolResult::success(result_json.to_string())
    }
}

// ── helpers ──────────────────────────────────────────────────

/// Detect a sensible file extension from a `data:` URI prefix of the form
/// `image/png;base64,...`. Returns "png" for the common cases we see from
/// OpenRouter (flux.2-pro, gemini-2.5-flash-image), or "bin" if unknown.
fn detect_image_extension_from_data_uri(stripped: &str) -> &'static str {
    // stripped looks like "image/png;base64,...." (the leading "data:" was
    // stripped by the caller). Pull out the substring up to the first ';'.
    let end = stripped.find(';').unwrap_or(stripped.len());
    let mime = &stripped[..end];
    match mime {
        "image/png" => "png",
        "image/jpeg" | "image/jpg" => "jpg",
        "image/webp" => "webp",
        "image/gif" => "gif",
        _ => "bin",
    }
}

impl ImageGenTool {
    /// Persist the first image (from `first_url`) to `dest_path`. Used when
    /// the caller passed an explicit `save_path` and the URL is either an
    /// http(s) URL or a `data:` URI. Returns the path on success, or an
    /// error string on failure (the previous version silently returned
    /// `dest_path` even when the write failed, which misled the LLM into
    /// thinking the file existed when it didn't).
    async fn persist_first_image(
        &self,
        first_url: &str,
        dest_path: &str,
        model: &str,
    ) -> Result<String, String> {
        if let Some(stripped) = first_url.strip_prefix("data:") {
            return self.write_data_uri_to_path(stripped, dest_path, model);
        }
        // HTTP(S) URL: try to GET the bytes and write to disk.
        let resp = match self.http.get(first_url).send().await {
            Ok(r) => r,
            Err(e) => {
                let prefix = first_url.chars().take(40).collect::<String>();
                warn!(error = %e, model = %model, url_prefix = %prefix, "image_gen: download request failed");
                return Err(format!(
                    "download request failed for {prefix}...: {e}"
                ));
            }
        };
        if !resp.status().is_success() {
            warn!(status = %resp.status(), model = %model, "image_gen: download returned non-2xx");
            return Err(format!(
                "download returned HTTP {}",
                resp.status()
            ));
        }
        let bytes = match resp.bytes().await {
            Ok(b) => b,
            Err(e) => {
                warn!(error = %e, model = %model, "image_gen: failed to download bytes");
                return Err(format!("failed to download bytes: {e}"));
            }
        };
        if let Some(parent) = std::path::Path::new(dest_path).parent() {
            // Surface create_dir failures — silent ignore was a real bug
            // in the old code path.
            std::fs::create_dir_all(parent).map_err(|e| {
                format!("failed to create parent dir {}: {e}", parent.display())
            })?;
        }
        std::fs::write(dest_path, &bytes).map_err(|e| {
            warn!(error = %e, save_path = %dest_path, model = %model, "image_gen: failed to write file");
            format!("failed to write {}: {e}", dest_path)
        })?;
        info!(
            model = %model,
            dest_path = %dest_path,
            bytes = bytes.len(),
            "image_gen: saved http(s) URL bytes to disk"
        );
        Ok(dest_path.to_string())
    }

    /// Decode a `data:` URI payload (the part after `data:`) and write the
    /// raw bytes to `dest_path`. Returns the path on success, or an error
    /// string on failure (the previous version returned `dest_path` on
    /// failure too, which masked real filesystem errors from the LLM).
    fn write_data_uri_to_path(
        &self,
        stripped: &str,
        dest_path: &str,
        model: &str,
    ) -> Result<String, String> {
        // stripped looks like "image/png;base64,...." — split off the base64
        // payload. Note: the spec also allows URL-encoded data, but
        // OpenRouter and friends always use base64.
        let payload = stripped.find(',')
            .map(|i| &stripped[i + 1..])
            .ok_or_else(|| {
                warn!(model = %model, dest_path = %dest_path, "image_gen: data URI missing comma separator");
                format!("data URI missing comma separator (dest_path={dest_path})")
            })?;
        let bytes = base64_decode(payload)
            .ok_or_else(|| {
                warn!(model = %model, dest_path = %dest_path, "image_gen: data URI base64 decode failed");
                format!("data URI base64 decode failed (dest_path={dest_path})")
            })?;
        if let Some(parent) = std::path::Path::new(dest_path).parent() {
            // Surface create_dir failures — silent ignore was a real bug
            // in the old code path.
            std::fs::create_dir_all(parent).map_err(|e| {
                format!("failed to create parent dir {}: {e}", parent.display())
            })?;
        }
        std::fs::write(dest_path, &bytes).map_err(|e| {
            warn!(error = %e, dest_path = %dest_path, model = %model, "image_gen: write failed");
            format!("failed to write {dest_path}: {e}")
        })?;
        info!(
            model = %model,
            dest_path = %dest_path,
            bytes = bytes.len(),
            "image_gen: auto-saved data URI to disk"
        );
        Ok(dest_path.to_string())
    }
}

/// Minimal base64 decoder. We avoid pulling in the `base64` crate to keep
/// the change small; base64 decoding is well-defined and the input is
/// always a single contiguous block from a trusted provider (OpenRouter).
/// Returns None on any malformed input.
fn base64_decode(input: &str) -> Option<Vec<u8>> {
    // Use the standard alphabet, ignore whitespace. Modern Rust has
    // `base64` available via Cargo, but we don't want a new dependency
    // for one call site, so use a simple lookup table.
    fn val(c: u8) -> Option<u8> {
        match c {
            b'A'..=b'Z' => Some(c - b'A'),
            b'a'..=b'z' => Some(c - b'a' + 26),
            b'0'..=b'9' => Some(c - b'0' + 52),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    }
    // Strip padding for length math.
    let mut clean: Vec<u8> = input
        .bytes()
        .filter(|b| !b.is_ascii_whitespace())
        .collect();
    let pad = clean.iter().rev().take_while(|&&b| b == b'=').count();
    clean.truncate(clean.len() - pad);
    if clean.len() % 4 == 1 {
        return None; // invalid base64 length
    }
    let mut out = Vec::with_capacity(clean.len() / 4 * 3);
    let mut buf: u32 = 0;
    let mut bits: u32 = 0;
    for &b in &clean {
        let v = val(b)?;
        buf = (buf << 6) | (v as u32);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push(((buf >> bits) & 0xFF) as u8);
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_decode_png_magic_bytes() {
        // 1x1 transparent PNG. The first 8 bytes must be 89 50 4e 47 0d 0a 1a 0a.
        let b64 = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8/5+hHgAHggJ/PchI7wAAAABJRU5ErkJggg==";
        let bytes = base64_decode(b64).expect("decode failed");
        let first8: Vec<String> = bytes.iter().take(8).map(|b| format!("{:02x}", b)).collect();
        assert_eq!(
            first8.join(" "),
            "89 50 4e 47 0d 0a 1a 0a",
            "expected PNG magic bytes"
        );
    }

    #[test]
    fn base64_decode_text() {
        let b64 = "SGVsbG8sIFdvcmxkIQ==";
        let decoded = base64_decode(b64).expect("decode failed");
        assert_eq!(String::from_utf8(decoded).unwrap(), "Hello, World!");
    }

    #[test]
    fn base64_decode_no_padding() {
        let b64 = "SGVsbG8";
        let decoded = base64_decode(b64).expect("decode failed");
        assert_eq!(String::from_utf8(decoded).unwrap(), "Hello");
    }

    #[test]
    fn base64_decode_rejects_invalid() {
        // Length 1 mod 4 is invalid base64.
        assert!(base64_decode("a").is_none());
        // Unknown character.
        assert!(base64_decode("@@@@").is_none());
    }

    #[test]
    fn detect_image_extension_works() {
        assert_eq!(detect_image_extension_from_data_uri("image/png;base64,..."), "png");
        assert_eq!(detect_image_extension_from_data_uri("image/jpeg;base64,..."), "jpg");
        assert_eq!(detect_image_extension_from_data_uri("image/webp;base64,..."), "webp");
        assert_eq!(detect_image_extension_from_data_uri("image/gif;base64,..."), "gif");
        assert_eq!(detect_image_extension_from_data_uri("image/svg+xml;base64,..."), "bin");
    }
}
