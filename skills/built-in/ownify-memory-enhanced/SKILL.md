---
name: ownify-memory-enhanced
description: "Use whenever you interact with the ownify memory system (MemPalace). Triggers on: remembering facts, storing context, retrieving prior information, searching memory, or when answering any question that could benefit from stored knowledge. Also triggers automatically before answering factual questions about the tenant's projects, infrastructure, decisions, or preferences."
license: MIT
---

# ownify-memory-enhanced — Mandatory Memory Protocol

## When This Skill Loads

This skill is **always active** when the ownify-memory MCP tools are available. It replaces any generic memory guidance with a strict protocol.

## Absolute Rules

### 1. Search Before Every Answer

Before answering ANY user message, run `ownify_search`:

```
ownify_search(query="[3-5 key terms from the message]", limit=10)
```

If the message mentions infrastructure, configs, projects, or past work, ALSO search:

```
ownify_search(query="[specific topic]", limit=10, wing="tenant-memory")
```

**No exceptions.** Even for greetings. Even for "thank you". Search first.

### 2. Knowledge Graph for Named Entities

If the user mentions a specific name (IP, project, service, tool, person):

```
ownify_kg_query(entity="[entity name]")
```

### 3. Cite What Came From Memory

Always distinguish stored facts from your training data:
- "According to stored records, ..."
- "Previously we established that ..."

If memory contradicts your training data, **trust memory** — it reflects the user's actual environment.

### 4. Store Immediately (Never Defer)

After completing an answer, if you learned anything new, store it RIGHT NOW:

- `ownify_store_workspace_fact` — paths, configs, system references (security/system wing)
- `ownify_store_decision` — architectural choices, policies (private/decisions)
- `ownify_store_todo` — action items, follow-ups (private/todos)
- `ownify_store_diary_entry` — session summaries, observations (diary/YYYY-MM-DD)
- `ownify_store_user_preference` — output formats, style (private/preferences)
- `ownify_store_user_profile` — personal facts about the user (private/profile)
- `ownify_store_event` — real-world events, news (tenant-memory/events)

Do not ask "should I store this?" — just store it.

## Memory Structure Reference

| Wing | Contents | When to Query |
|------|----------|---------------|
| `private` | User's todos, decisions, preferences | User asks about their stuff |
| `tenant-memory` | Infrastructure, projects, events | Anything technical about this tenant |
| `diary` | Session logs | "What did we do last time?" |
| `public` | Shareable docs | Rarely needed |
| `security` | System paths, configs | Environment questions |
| `shared` | Coordination across agents | Multi-agent tasks |

## Proactive Storage Checklist

After every interaction, did you learn any of these?

- [ ] New config value, path, or environment variable
- [ ] Command that worked (and its exact output)
- [ ] Error encountered and how it was resolved
- [ ] Decision the user made or directed
- [ ] User preference about output format or behavior
- [ ] Interesting observation about the project/system

If YES to any — **store it immediately** before ending the turn.

## Retrieval Best Practices

Use semantic `ownify_search` with specific, keyword-rich queries:
- "kubernetes deployment helm values"
- "domain services subdomains"
- "gateway IP building automation"
- "project scanner IoT"

Use `ownify_list_drawers` to browse a specific wing/room.
Use `ownify_get_drawer` when you have a drawer_id from search results.
Use `ownify_kg_query` to look up relationships for known entities.

## Anti-Patterns (NEVER)

1. Never answer from training data alone — always search first
2. Never say "I don't know" without searching — search, then admit uncertainty
3. Never ask "should I store this?" — just store it
4. Never store raw secrets — references only in `security` wing
5. Never dump all memory at once — retrieve what is relevant
