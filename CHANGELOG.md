# Changelog

This is the ownify fork of MicroClaw. For upstream changes, see [microclaw/microclaw](https://github.com/microclaw/microclaw).

## ownify additions

### Agent-to-agent (A2A)

- AAE-signed envelopes for agent-to-agent communication
- Per-caller capability ACLs via `x-ownify-caller-kind` and `x-ownify-caller-grants`
- A2A gateway integration for per-tenant routing

### ownify platform integration

- Egress DLP scanning via ownify-egress-scanner (fail-closed by default)
- URL sanitization for internal cluster DNS names
- Cluster-side scanner client in `src/egress_scan.rs`

### Memory

- ownify-memory-enhanced skill for structured memory protocol
- a2a-self-log skill for agent-to-agent interaction logging
- SOUL.md personality injection with ownify branding

### Skills

- Marp-based pptx creation instead of python-pptx
- Built-in skills: ownify-memory-enhanced, a2a-self-log, autonomous-coder

### Security hardening

- External caller fencing in A2A endpoints
- Path guard blocks: `.ssh`, `.aws`, `.gnupg`, `.kube`, `.env`, and cloud credential files
- DLP fail-closed posture for egress scanner
