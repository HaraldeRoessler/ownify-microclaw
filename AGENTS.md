# AGENTS.md

## Project overview

ownify MicroClaw is a Rust multi-channel agent runtime for Telegram, Discord, Slack, Feishu, IRC, and Web. It shares one channel-agnostic agent loop (`src/agent_engine.rs`) and one provider-agnostic LLM layer (`src/llm.rs`).

This is a production fork of upstream [MicroClaw](https://microclaw.ai) (MIT license), extended for the ownify Kubernetes platform.

Core capabilities:
- Tool-using chat agent loop (multi-step tool calls)
- Session resume and context compaction
- Scheduled tasks + background scheduler
- File memory (`AGENTS.md`) + structured SQLite memory
- Memory reflector, quality gate, and observability metrics
- Skills + MCP tool federation
- Agent-to-agent (A2A) via ownify-gateway

## Tech stack

- Language: Rust (edition 2021)
- CLI args: clap
- Async runtime: Tokio
- Telegram: teloxide
- Discord: serenity
- Web API/UI: axum + React (in `web/`)
- Database: SQLite (rusqlite)
- LLM runtime: provider abstraction with native Anthropic and OpenAI-compatible providers

## Source index (`src/` + `crates/`)

Main orchestration files in `src/`:
- `main.rs`: CLI entry (`start`, `setup`, etc.)
- `runtime.rs`: app wiring (`AppState`), provider/tool initialization, channel boot
- `agent_engine.rs`: shared agent loop (`process_with_agent`)
- `llm.rs`: provider implementations + stream handling
- `web.rs`: Web API router, shared web state, stream APIs
- `scheduler.rs`: scheduled-task runner + memory reflector loop
- `channels/*.rs`: concrete channel adapters (Telegram/Discord/Slack/Feishu/IRC)
- `tools/*.rs`: built-in tool implementations and registry assembly

Modularized crates in `crates/`:
- `microclaw-core`: shared error/types/text
- `microclaw-storage`: SQLite DB, memory domain, usage reports
- `microclaw-tools`: tool runtime primitives, sandbox, path guards
- `microclaw-channels`: channel abstractions, delivery boundary
- `microclaw-app`: app-level support (logging, builtin skills, transcribe)

## Agent loop

`process_with_agent` flow:
1. Optional explicit-memory fast path writes structured memory directly
2. Load resumable session from `sessions`, or rebuild from chat history
3. Build system prompt from file memory + structured memory + skills
4. Compact old context if session exceeds limits
5. Call provider with tool schemas
6. If `tool_use`: execute tool(s), append results, loop
7. If `end_turn`: persist session and return text

## Memory architecture

Two layers:
1. File memory: `runtime/groups/AGENTS.md` (global) and `runtime/groups/{chat_id}/AGENTS.md` (per-chat)
2. Structured memory (`memories` table): category, confidence, source, archived lifecycle, reflector extraction, dedup/supersede

## Build and test

```sh
cargo build
cargo test
npm --prefix web run build
```

Docs drift guard:
```sh
node scripts/generate_docs_artifacts.mjs --check
```
