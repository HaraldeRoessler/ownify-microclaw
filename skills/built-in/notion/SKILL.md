---
name: notion
description: "Query databases, create + update pages, search, and append content blocks in a Notion workspace. Use when the user asks to add to their Notion, query a Notion database, create a Notion page, search their workspace, or update a Notion page. Requires NOTION_API_KEY env var (an Internal Integration token) and that the integration has been SHARED with the relevant pages/databases from Notion's UI. Each agent should have its own integration."
license: MIT (see repository LICENSE)
compatibility: "Requires python3 with notion-client installed (bundled in image). Works on Linux."
---

# Notion Skill

Query + create + update Notion content via the official REST API using the `notion-client` Python SDK.

## Prerequisites

One env var + one Notion-side action per target page:

- `NOTION_API_KEY` — Internal Integration token from https://www.notion.so/my-integrations. Create an integration, copy the "Internal Integration Secret" (starts with `secret_`). Configure it on the agent via portal Settings → Skills → Notion.
- For every database or page the agent should access: open it in Notion → top-right `•••` → "Connections" (or "Add connections") → add your integration. Without this share step the API returns 404 for pages it could otherwise see — Notion's permission model is opt-in per-resource.

## Search the workspace

```bash
python3 - <<'PY'
import os, json
from notion_client import Client
n = Client(auth=os.environ["NOTION_API_KEY"])
res = n.search(query="weekly report", page_size=20)
for r in res["results"]:
    obj = r["object"]  # "page" | "database"
    title = ""
    if obj == "page":
        for prop in r.get("properties", {}).values():
            if prop.get("type") == "title":
                title = "".join(t["plain_text"] for t in prop["title"])
                break
    else:
        title = "".join(t["plain_text"] for t in r.get("title", []))
    print(f"{obj:8} {r['id']}  {title}  ({r.get('url','')})")
PY
```

Search only returns objects shared with your integration.

## Get a page's properties + content

```bash
python3 - <<'PY'
import os, sys, json
from notion_client import Client
PAGE_ID = sys.argv[1]  # can include dashes or not; Notion accepts both
n = Client(auth=os.environ["NOTION_API_KEY"])
page = n.pages.retrieve(page_id=PAGE_ID)
print("Properties:")
for name, prop in page["properties"].items():
    t = prop["type"]
    val = prop[t]
    print(f"  {name:30} [{t}]  {val}")

print("\nContent blocks:")
cursor = None
while True:
    resp = n.blocks.children.list(block_id=PAGE_ID, start_cursor=cursor, page_size=100)
    for b in resp["results"]:
        t = b["type"]
        content = b[t]
        if t in ("paragraph", "heading_1", "heading_2", "heading_3", "bulleted_list_item", "numbered_list_item", "quote"):
            text = "".join(r["plain_text"] for r in content.get("rich_text", []))
            print(f"  [{t}] {text}")
        elif t == "to_do":
            text = "".join(r["plain_text"] for r in content.get("rich_text", []))
            print(f"  [todo {'x' if content.get('checked') else ' '}] {text}")
        elif t == "code":
            text = "".join(r["plain_text"] for r in content.get("rich_text", []))
            print(f"  [code/{content.get('language','')}]\n{text}\n")
        else:
            print(f"  [{t}]  (not rendered — see full block JSON if needed)")
    if not resp.get("has_more"): break
    cursor = resp.get("next_cursor")
PY
```

## Query a database

```bash
python3 - <<'PY'
import os, sys, json
from notion_client import Client
DB_ID = sys.argv[1]
n = Client(auth=os.environ["NOTION_API_KEY"])

# Simple filter: status = "In progress"
resp = n.databases.query(
    database_id=DB_ID,
    filter={"property": "Status", "status": {"equals": "In progress"}},
    sorts=[{"property": "Due date", "direction": "ascending"}],
    page_size=50,
)

for r in resp["results"]:
    props = r["properties"]
    title_prop = next((p for p in props.values() if p.get("type") == "title"), None)
    title = "".join(t["plain_text"] for t in title_prop["title"]) if title_prop else "(untitled)"
    print(f"{r['id']}  {title}")
PY
```

Filter shapes vary per property type — see https://developers.notion.com/reference/post-database-query-filter.

## Create a new page inside a database

```bash
python3 - <<'PY'
import os, sys, json
from notion_client import Client
DB_ID = sys.argv[1]
TITLE = sys.argv[2] if len(sys.argv) > 2 else "Created from ownify agent"
n = Client(auth=os.environ["NOTION_API_KEY"])

page = n.pages.create(
    parent={"database_id": DB_ID},
    properties={
        # Replace "Name" with whatever your database calls its title column
        "Name": {"title": [{"text": {"content": TITLE}}]},
        # Example non-title properties:
        # "Status": {"status": {"name": "Not started"}},
        # "Priority": {"select": {"name": "High"}},
        # "Due date": {"date": {"start": "2026-05-01"}},
        # "Tags": {"multi_select": [{"name": "ownify"}, {"name": "automation"}]},
    },
    children=[
        {
            "object": "block",
            "type": "paragraph",
            "paragraph": {"rich_text": [{"text": {"content": "First paragraph of body."}}]},
        },
    ],
)
print(f"Created page: {page['id']}  url={page['url']}")
PY
```

## Append content to an existing page

```bash
python3 - <<'PY'
import os, sys
from notion_client import Client
PAGE_ID = sys.argv[1]
n = Client(auth=os.environ["NOTION_API_KEY"])
n.blocks.children.append(
    block_id=PAGE_ID,
    children=[
        {"object": "block", "type": "heading_2", "heading_2": {"rich_text": [{"text": {"content": "Update"}}]}},
        {"object": "block", "type": "paragraph", "paragraph": {"rich_text": [{"text": {"content": "Context about what changed."}}]}},
        {"object": "block", "type": "to_do", "to_do": {"rich_text": [{"text": {"content": "Follow up"}}], "checked": False}},
    ],
)
print("Appended.")
PY
```

## Update a page's properties (not body)

```bash
python3 - <<'PY'
import os, sys
from notion_client import Client
PAGE_ID = sys.argv[1]
n = Client(auth=os.environ["NOTION_API_KEY"])
n.pages.update(
    page_id=PAGE_ID,
    properties={
        "Status": {"status": {"name": "Done"}},
        # "Priority": {"select": {"name": "Low"}},
    },
)
print("Updated.")
PY
```

## Usage guidance

- The agent will see 404 for any resource not shared with the integration, even if it clearly exists in the workspace. Always suggest "go to the Notion page → Connections → add your integration" as the first fix.
- Rate limit: ~3 requests/sec per integration. Notion returns 429 with a `retry-after` header — respect it.
- Property names in database queries are CASE-sensitive and must match exactly what Notion shows in the database's column header.
- When user says "add a task" without specifying a database, ask which database first; don't guess.
- Never log `NOTION_API_KEY`.
- The SDK is `notion-client` (the unofficial-but-widely-used Python wrapper). For anything truly unusual, fall back to raw HTTP with `requests` — the Notion REST API is stable and well-documented at https://developers.notion.com/reference.
