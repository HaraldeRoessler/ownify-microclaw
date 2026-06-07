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

use std::time::Duration;

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use tracing::{info, warn};

use microclaw_core::llm_types::ToolDefinition;
use microclaw_tools::runtime::{Tool, ToolResult};

use crate::config::Config;

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
            description: "Generate an image from a text prompt using the configured image generation provider (OpenRouter, OpenAI, Together, Replicate, Stability, etc.). Returns the image URL. The agent should then send the image to the user as a Matrix attachment via send_message, or save the image locally and use it for webpages (Nextcloud upload, embed in HTML, etc.).".into(),
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
                        "description": "If set, the image bytes are downloaded from the provider URL and saved to this local file path. The tool result then includes both the URL and the local path. Use this when the image will be embedded in a generated webpage or uploaded to Nextcloud."
                    }
                },
                "required": ["prompt"]
            }),
        }
    }

    async fn execute(&self, input: Value) -> ToolResult {
        let input: ImageGenInput = match serde_json::from_value(input) {
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

        let model = input.model.unwrap_or_else(|| self.image_model.clone());
        if model.is_empty() {
            return ToolResult::error("No image model configured. Set IMAGE_MODEL or pass model=...".to_string()).with_error_type("not_configured");
        }
        let size = input.size.unwrap_or_else(|| self.image_default_size.clone());
        let n = input.n.unwrap_or(1);

        // Build OpenAI-compatible request body.
        let mut body = json!({
            "model": model,
            "prompt": input.prompt,
            "size": size,
            "n": n,
            "response_format": "url",
        });
        if let Some(np) = &input.negative_prompt {
            body["negative_prompt"] = json!(np);
        }

        info!(
            provider = %self.image_provider,
            model = %model,
            size = %size,
            n = n,
            prompt_chars = input.prompt.len(),
            "image_gen: dispatching request"
        );

        let url = format!("{}/images/generations", self.image_api_url.trim_end_matches('/'));
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

        // OpenAI format: { "data": [ { "url": "...", "b64_json": "..." }, ... ] }
        let images: Vec<&Value> = payload.get("data")
            .and_then(|d| d.as_array())
            .map(|a| a.iter().collect())
            .unwrap_or_default();

        if images.is_empty() {
            return ToolResult::error(format!("image_gen returned no images: {payload}")).with_error_type("empty_response");
        }

        // Collect URLs.
        let urls: Vec<String> = images.iter()
            .filter_map(|img| img.get("url").and_then(|u| u.as_str()).map(String::from))
            .collect();

        // Optional: download first image to save_path.
        let saved_path = if let Some(save_path) = &input.save_path {
            if let Some(first_url) = urls.first() {
                match self.http.get(first_url).send().await {
                    Ok(r) if r.status().is_success() => {
                        match r.bytes().await {
                            Ok(bytes) => {
                                // Ensure parent dir exists.
                                if let Some(parent) = std::path::Path::new(save_path).parent() {
                                    let _ = std::fs::create_dir_all(parent);
                                }
                                match std::fs::write(save_path, &bytes) {
                                    Ok(()) => Some(save_path.clone()),
                                    Err(e) => {
                                        warn!(error = %e, save_path = %save_path, "image_gen: failed to write file");
                                        None
                                    }
                                }
                            }
                            Err(e) => {
                                warn!(error = %e, "image_gen: failed to download bytes");
                                None
                            }
                        }
                    }
                    Ok(r) => {
                        warn!(status = %r.status(), "image_gen: download returned non-2xx");
                        None
                    }
                    Err(e) => {
                        warn!(error = %e, "image_gen: download request failed");
                        None
                    }
                }
            } else {
                None
            }
        } else {
            None
        };

        // Build tool result. Always include URLs; include saved_path if set.
        let result_json = json!({
            "ok": true,
            "provider": self.image_provider,
            "model": model,
            "size": size,
            "image_urls": urls,
            "saved_path": saved_path,
            "next_steps": if saved_path.is_some() {
                "Image saved locally. Use the saved_path to embed in a webpage or upload via the nextcloud tool. To send as a Matrix attachment, use send_message with image_url=<url>."
            } else {
                "To send as a Matrix attachment, use send_message with image_url=<url>."
            }
        });

        ToolResult::success(result_json.to_string())
    }
}
