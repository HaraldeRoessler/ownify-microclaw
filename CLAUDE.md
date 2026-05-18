# ownify MicroClaw

ownify MicroClaw is a Rust multi-platform chat bot with a channel-agnostic core and platform adapters. It supports Telegram, Discord, Slack, Feishu/Lark, and Web, with extensibility for more platforms. It provides agentic tool execution, web search, scheduled tasks, and persistent memory.

This is a production fork of [MicroClaw](https://microclaw.ai) — the upstream open-source multi-channel agent runtime (MIT license).

## Tech stack

Rust 2021, Tokio, teloxide 0.17, serenity 0.12, provider-agnostic LLM runtime (Anthropic + OpenAI-compatible), SQLite (rusqlite bundled), cron crate for scheduling.

## Directory overview

- `src/` -- Rust source for the bot binary
- `web/` -- Built-in Web UI (React + Vite)
- `crates/` -- Modularized crates (core, storage, tools, channels, app)

## Project layout

- `src/main.rs` -- entry point, CLI
- `src/runtime.rs` -- app wiring (`AppState`), provider/tool initialization, channel boot
- `src/agent_engine.rs` -- shared agent loop (`process_with_agent`)
- `src/llm.rs` -- provider implementations + format translation
- `src/web.rs` -- web API routes and streaming
- `src/scheduler.rs` -- background scheduler + memory reflector loops
- `src/channels/*.rs` -- Telegram/Discord/Slack/Feishu adapters
- `src/tools/*.rs` -- concrete built-in tools; registry assembly in `src/tools/mod.rs`
- `crates/microclaw-core/` -- shared error/types/text modules
- `crates/microclaw-storage/` -- SQLite DB schema/query layer + memory/usage domain
- `crates/microclaw-tools/` -- tool runtime primitives (trait/auth/risk/schema/path) + sandbox
- `crates/microclaw-channels/` -- channel abstraction and delivery boundary
- `crates/microclaw-app/` -- app-level support modules (logging, builtin skills, transcribe)

## Key patterns

- **Agentic loop** in `agent_engine.rs:process_with_agent`: call provider -> if tool_use -> execute -> loop (up to `max_tool_iterations`)
- **Session resume**: full `Vec<Message>` (including tool_use/tool_result blocks) persisted in `sessions` table
- **Context compaction**: older messages summarized when session exceeds `max_session_messages`
- **Sub-agent**: `sub_agent` tool spawns a fresh agentic loop with restricted tools
- **Tool trait**: `name()`, `definition()` (JSON Schema), `execute(serde_json::Value) -> ToolResult`
- **Shared state**: `AppState` in `Arc`, tools hold `Bot` / `Arc<Database>` as needed
- **Path guard**: sensitive paths (.ssh, .aws, .env, credentials, etc.) are blocked

## Build & run

```sh
cargo build
cargo run -- start
cargo run -- setup
cargo run -- help
```

## Configuration

MicroClaw uses `microclaw.config.yaml` (or `.yml`). Override with `MICROCLAW_CONFIG` env var.

## Soul (personality)

Supports `SOUL.md` for defining agent personality. Loading priority: config `soul_path` > `<data_dir>/SOUL.md` > `./SOUL.md`. Per-chat overrides at `<data_dir>/runtime/groups/<chat_id>/SOUL.md`.

## Database

SQLite via `microclaw-storage` (`Database` wrapper). Runtime state managed through versioned migrations.

## Conventions

- All timestamps are ISO 8601 / RFC 3339 strings
- Cron expressions use 6-field format (sec min hour dom month dow)
- Messages are stored for all chats regardless of whether bot responds
- In groups, bot only responds to @mentions
- Consecutive same-role messages are merged before sending to LLM provider
- Responses split at channel limits: Telegram 4096 / Discord 2000 / Slack 4000 / Feishu 4000
