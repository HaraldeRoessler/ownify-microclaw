---
name: a2a-self-log
description: "Automatically logs every A2A interaction to the agent's own diary. The agent uses this to remember who contacted it via A2A, what was asked, and what was answered. Must run after EVERY A2A message response — not just once."
license: MIT
---

# a2a-self-log — Persistent A2A interaction memory

MicroClaw A2A sessions are stateless and discarded after each response. Without this skill, the agent forgets every A2A interaction immediately. This skill ensures the agent remembers.

## When to run

After responding to EVERY A2A message. The inbound A2A request carries these headers:

- `x-caller-did` — the DID of the agent that contacted you
- `x-envelope` — the signed authorization envelope (contains caller identity)

## What to do

Extract the caller identity, summarize the interaction, write to your diary:

```
1. Identify caller: read x-caller-did header (or decode auth envelope)
2. Summarize: 1-2 sentences — who asked what, what you answered
3. Store in your memory system:
   store_diary_entry(
     content = "[A2A][from {caller_did}] Received task: {summary}. Replied: {response_summary}.",
     tags = ["a2a", "interaction", "{caller_did}"]
   )

## Example

Inbound A2A from `did:example:alice` (Alice) asking "What's the dev status?"

After responding, immediately write:

```
store_diary_entry(
  content = "[A2A][from did:example:alice] Received: asked for dev status update. Replied: reported service health, open issues, and access level.",
  tags = ["a2a", "interaction", "did:example:alice"]
)
```

## Anti-patterns

- Never skip this for "quick replies" — every A2A interaction counts
- Never log full message content verbatim — summarize, don't dump
- Never defer to "later" — A2A sessions evaporate, write immediately after response
