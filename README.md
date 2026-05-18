# ownify MicroClaw

[![License: MIT](https://img.shields.io/badge/License-MIT-green.svg)](LICENSE)
[![Docs](https://img.shields.io/badge/docs-upstream%20microclaw-blue)](https://microclaw.ai)

Agent runtime for [ownify](https://ownify.ai) — a multi-tenant Kubernetes platform for running AI agents at scale. This is a production fork of [MicroClaw](https://microclaw.ai) (MIT), extended and hardened for the ownify control plane.

## What it does

Every ownify tenant agent runs on this runtime. It provides:

- **Channel-agnostic agent loop** — same runtime, tools, and memory across Telegram, Discord, Slack, Matrix, and Web
- **LLM routing** — per-request classification and multi-provider routing via ownify-router
- **Persistent memory** — semantic search, knowledge graph, and automatic context compaction via ownify-memgate
- **Agent-to-agent (A2A)** — Ed25519-signed AAE envelopes, MolTrust reputation scoring, per-caller capability ACLs
- **Tool ecosystem** — built-in skills for documents, email, social media, web, and autonomous coding
- **Scheduled tasks** — cron-based recurring work and one-shot automation
- **Enterprise isolation** — per-tenant pods for microclaw, memory, routing, and egress scanning

## Relationship to upstream

This fork extends upstream [MicroClaw](https://microclaw.ai) (MIT license) with ownify-platform components:

| Component | Purpose |
|---|---|
| ownify-router | Per-tenant LLM request classification + multi-provider routing |
| ownify-memgate | Persistent agent memory with semantic search + knowledge graph |
| ownify-a2a-gateway | Agent-to-agent protocol with AAE envelope auth + per-caller ACL |
| ownify-egress-scanner | Outbound data-loss prevention (DLP) |
| ownify-control-plane | Per-tenant pod management, peer registry, signing key lifecycle |

## Quick start (local development)

```sh
git clone https://github.com/HaraldeRoessler/ownify-microclaw.git
cd ownify-microclaw
cargo build --release --features channel-matrix
./target/release/microclaw setup
./target/release/microclaw start
```

Default web UI: `http://127.0.0.1:10961`

## Deployment

ownify MicroClaw runs as a per-tenant pod on the ownify Kubernetes platform. Each tenant gets dedicated instances:

| Pod | Purpose |
|---|---|
| `microclaw` | Agent runtime (this software) |
| `ownify-router` | LLM request classification + multi-provider routing |
| `ownify-memgate` | Persistent memory (vector search + knowledge graph) |
| `ownify-a2a-gateway` | Agent-to-agent protocol with AAE envelope auth |
| `ownify-egress-scanner` | Outbound data-loss prevention (DLP) |

Deployment is managed by the ownify control plane. See [ownify.ai](https://ownify.ai) for platform details.

## Docker

```sh
docker pull ghcr.io/haralderoessler/ownify-microclaw:latest
docker run --rm -it -p 127.0.0.1:10961:10961 ghcr.io/haralderoessler/ownify-microclaw:latest
```

Persist config and data:

```sh
mkdir -p data tmp
chmod a+r microclaw.config.yaml
chmod -R a+rwX data tmp

docker run --rm -it \
  -p 127.0.0.1:10961:10961 \
  -v "$(pwd)/microclaw.config.yaml:/app/microclaw.config.yaml:ro" \
  -v "$(pwd)/data:/home/microclaw/.microclaw" \
  -v "$(pwd)/tmp:/app/tmp" \
  ghcr.io/haralderoessler/ownify-microclaw:latest
```

## Architecture

```
crates/
    microclaw-core/      # Shared error/types/text
    microclaw-storage/   # SQLite DB + memory + usage reporting
    microclaw-tools/     # Tool runtime primitives + sandbox
    microclaw-channels/  # Channel abstractions
    microclaw-app/       # App support (logging, builtin skills, transcribe)

src/
    main.rs              # CLI entry
    runtime.rs           # Runtime bootstrap + adapter startup
    agent_engine.rs      # Channel-agnostic agent loop
    llm.rs               # Provider abstraction (Anthropic/OpenAI)
    channels/*.rs        # Telegram, Discord, Slack, Feishu, IRC adapters
    tools/*.rs           # Built-in tools + registry
    scheduler.rs         # Background scheduler + reflector loop
    web.rs               # Web API + stream endpoints
```

Key design:
- Session resume persists full message history (including tool blocks) in SQLite
- Context compaction summarizes old messages to stay within limits
- Provider abstraction with native Anthropic + OpenAI-compatible endpoints
- SQLite with WAL mode for concurrent access
- Exponential backoff on 429 rate limits (3 retries)

## Key features

- **Agentic tool use** — bash, file I/O, glob, grep, persistent memory
- **Session resume** — full conversation state persisted across restarts
- **Context compaction** — automatic summarization of old messages
- **Sub-agents** — delegate sub-tasks to parallel agent runs
- **Agent skills** — Anthropic Skills-compatible, auto-discovered from `<data_dir>/skills/`
- **Plan & execute** — todo list for breaking down complex tasks
- **Web search** — DuckDuckGo + web page fetching
- **Scheduled tasks** — 6-field cron expressions, natural language management
- **Multi-channel** — one runtime across Telegram, Discord, Slack, Feishu, IRC, and Web
- **Persistent memory** — AGENTS.md files + structured SQLite memory with layered injection

See the [upstream documentation](https://microclaw.ai) for full tool reference, config defaults, and provider matrix.

## Built-in skills

ownify ships with these skills (in `skills/built-in/`):

| Skill | Purpose |
|---|---|
| `docx`, `pdf`, `pptx`, `xlsx` | Document creation and editing |
| `ownify-memory-enhanced` | Memory retrieval and storage protocol |
| `a2a-self-log` | Agent-to-agent interaction logging |
| `sendgrid` | Email via SendGrid API |
| `github` | GitHub repository operations |
| `imap-mail` | IMAP email client |
| `autonomous-coder` | Autonomous coding workflow |
| `weather`, `yahoo-finance` | Data lookup |

## Commands

- `/reset` — clear current chat session
- `/status` — show runtime/session status
- `/skills` — list available skills
- `/usage` — show token usage summary
- `/provider` — show/switch provider profile
- `/model` — show/switch model
- `/archive` — archive current session
- `/clear` — clear chat context, keep tasks
- `/stop` — abort current run

## Documentation

| File | Description |
|---|---|
| [DEVELOP.md](DEVELOP.md) | Developer guide |
| [TEST.md](TEST.md) | Testing guide |
| [CONTRIBUTING.md](CONTRIBUTING.md) | Contribution workflow |
| [SECURITY.md](SECURITY.md) | Security policy |
| [SUPPORT.md](SUPPORT.md) | Support policy |
| [docs/](docs/) | Operations, releases, RFCs, observability |

For the full upstream documentation, see [microclaw.ai](https://microclaw.ai).

## License

MIT — see [LICENSE](LICENSE). Upstream MicroClaw is also MIT-licensed.
