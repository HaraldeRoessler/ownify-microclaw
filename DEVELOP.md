# ownify MicroClaw Developer Guide

## Quick start

```sh
git clone https://github.com/ownify/ownify-microclaw.git
cd ownify-microclaw
cp microclaw.config.example.yaml microclaw.config.yaml
cargo run -- start
```

## Prerequisites

- Rust 1.70+ (2021 edition)
- At least one enabled channel adapter
- A model provider API key (Anthropic or OpenAI-compatible)
- No other external dependencies. SQLite is bundled via `rusqlite`.

## Project structure

```
crates/
    microclaw-core/      # Shared error/types/text modules
    microclaw-storage/   # SQLite + memory + usage reporting
    microclaw-tools/     # Tool runtime primitives + sandbox
    microclaw-channels/  # Channel abstraction boundary
    microclaw-app/       # App support modules (logging/skills/transcribe)

src/
    main.rs              # CLI entrypoint
    runtime.rs           # Runtime bootstrap + adapter startup
    agent_engine.rs      # Shared agent loop (process_with_agent)
    llm.rs               # Provider adapters (Anthropic/OpenAI-compatible/Codex)
    web.rs               # Web API + stream endpoints
    scheduler.rs         # Scheduler + memory reflector loops
    channels/*.rs        # Concrete adapters (Telegram/Discord/Slack/Feishu)
    tools/*.rs           # Built-in tools + registry assembly
```

## Architecture overview

### Data flow

```
Platform message (via adapter)
       |
       v
    Store in SQLite (message + chat metadata)
       |
       v
    Determine response: private=always, group=@mention only
       |
       v
    Load session or history
       |
       v
    Build system prompt
       |
       v
    Compact if needed (messages > max_session_messages)
       |
       v
    Agentic loop (up to max_tool_iterations):
        1. Call provider API with messages + tool definitions
        2. If stop_reason == "tool_use" -> execute tools -> append results -> loop
        3. If stop_reason == "end_turn" -> extract text -> return
       |
       v
    Strip image base64 data, save session to SQLite
       |
       v
    Send response (split at channel limits)
       |
       v
    Store bot response in SQLite
```

### Key types

| Type | Location | Description |
|------|----------|-------------|
| `AppState` | `runtime.rs` | Shared runtime state |
| `Database` | `microclaw_storage::db` | SQLite wrapper |
| `ToolRegistry` | `tools/mod.rs` | Holds `Box<dyn Tool>`, dispatches by name |
| `Tool` trait | `microclaw_tools::runtime` | `name()`, `definition()`, `execute()` |
| `LlmProvider` | `llm.rs` | Provider abstraction |
| `MemoryManager` | `memory.rs` | AGENTS.md file memory reader/writer |

### Multi-chat permission model

- `control_chat_ids` defines privileged chats
- Tool execution receives trusted caller context from `process_with_agent`
- Non-control chats can only operate on their own `chat_id`
- Control chats can perform cross-chat actions
- `write_memory` with `scope: "global"` is restricted to control chats

## Adding a new tool

1. Create `src/tools/my_tool.rs` implementing the `Tool` trait
2. Add `pub mod my_tool;` to `src/tools/mod.rs`
3. Register in `ToolRegistry::new()` with `Box::new(my_tool::MyTool::new(...))`

## Debugging

```sh
RUST_LOG=debug cargo run -- start
RUST_LOG=microclaw=debug cargo run -- start

sqlite3 microclaw.data/runtime/microclaw.db
sqlite> SELECT * FROM messages ORDER BY timestamp DESC LIMIT 10;
sqlite> SELECT * FROM scheduled_tasks;
sqlite> SELECT * FROM chats;
```

## Build

```sh
cargo build              # Dev build
cargo build --release    # Release build
cargo run -- start       # Run dev build
cargo run -- help        # Show CLI help
```
