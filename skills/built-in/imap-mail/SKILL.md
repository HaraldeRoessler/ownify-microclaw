---
name: imap-mail
description: "Read, search, triage, and move email in any IMAP mailbox — Gmail, iCloud, Proton Bridge, Fastmail, self-hosted, anything speaking IMAP. Use when the user asks to check their inbox, search emails, summarize messages, flag, delete, or move email. Triggers on mentions of check mail, inbox, unread, search email, archive, move to folder. Requires IMAP_HOST, IMAP_USER, IMAP_PASSWORD (and optionally IMAP_PORT) env vars on the agent."
license: MIT (see repository LICENSE)
compatibility: "Requires python3 stdlib only (imaplib, email). Works on Linux."
---

# IMAP Mail Skill

Read and manipulate email via IMAP using Python stdlib only — no external packages. Any IMAP server works; examples below assume TLS on port 993.

## Prerequisites

Four environment variables (configure from the portal Settings → Skills → IMAP Mail):

- `IMAP_HOST` — e.g. `imap.gmail.com`, `imap.mail.me.com`, `127.0.0.1` (Proton Bridge), `imap.fastmail.com`
- `IMAP_USER` — full email address
- `IMAP_PASSWORD` — an app-specific password, NOT the main account password
  - Gmail: https://myaccount.google.com/apppasswords (requires 2FA enabled)
  - iCloud: https://appleid.apple.com → App-Specific Passwords
  - Proton: Proton Bridge generates a local password
  - Fastmail: Settings → Privacy & Security → App Passwords
- `IMAP_PORT` — optional, defaults to 993 (TLS)

## List recent messages

```bash
python3 - <<'PY'
import os, imaplib, email
from email.header import decode_header

M = imaplib.IMAP4_SSL(os.environ["IMAP_HOST"], int(os.environ.get("IMAP_PORT", 993)))
M.login(os.environ["IMAP_USER"], os.environ["IMAP_PASSWORD"])
M.select("INBOX", readonly=True)

typ, data = M.search(None, "ALL")
ids = data[0].split()[-20:]  # last 20 messages

for mid in reversed(ids):
    typ, d = M.fetch(mid, "(BODY.PEEK[HEADER.FIELDS (FROM SUBJECT DATE)])")
    hdr = email.message_from_bytes(d[0][1])
    def dec(v):
        parts = decode_header(v or "")
        return "".join(p.decode(c or "utf-8", "replace") if isinstance(p, bytes) else p for p, c in parts)
    print(f"[{mid.decode()}] {dec(hdr['Date']):25} | {dec(hdr['From']):40} | {dec(hdr['Subject'])}")

M.logout()
PY
```

Use `readonly=True` on SELECT whenever you are not modifying state — it prevents \Seen flag changes.

## List unread only

```bash
python3 - <<'PY'
import os, imaplib, email
from email.header import decode_header

M = imaplib.IMAP4_SSL(os.environ["IMAP_HOST"], int(os.environ.get("IMAP_PORT", 993)))
M.login(os.environ["IMAP_USER"], os.environ["IMAP_PASSWORD"])
M.select("INBOX", readonly=True)
typ, data = M.search(None, "UNSEEN")
unread_ids = data[0].split()
print(f"{len(unread_ids)} unread")

for mid in unread_ids[-30:]:
    typ, d = M.fetch(mid, "(BODY.PEEK[HEADER.FIELDS (FROM SUBJECT DATE)])")
    hdr = email.message_from_bytes(d[0][1])
    def dec(v):
        parts = decode_header(v or "")
        return "".join(p.decode(c or "utf-8", "replace") if isinstance(p, bytes) else p for p, c in parts)
    print(f"[{mid.decode()}] {dec(hdr['From']):40} | {dec(hdr['Subject'])}")

M.logout()
PY
```

## Search

IMAP search syntax — no quotes around field tokens, quoted strings only for values containing spaces:

```bash
python3 - <<'PY'
import os, imaplib
M = imaplib.IMAP4_SSL(os.environ["IMAP_HOST"], int(os.environ.get("IMAP_PORT", 993)))
M.login(os.environ["IMAP_USER"], os.environ["IMAP_PASSWORD"])
M.select("INBOX", readonly=True)

# Examples — replace criteria as needed:
# - FROM "alice@example.com"
# - SUBJECT "invoice"
# - SINCE 01-Jan-2025
# - UNSEEN
# - TEXT "refund"
# - LARGER 5000000  (bytes)

typ, data = M.search(None, 'FROM "notifications@github.com"', 'SINCE', '01-Jan-2026')
ids = data[0].split()
print(f"{len(ids)} matches")
print([i.decode() for i in ids[-20:]])
M.logout()
PY
```

Combine criteria by passing them as separate positional args (implicit AND). For OR, prefix with `OR`:
```python
M.search(None, 'OR', 'FROM', '"alice@example.com"', 'FROM', '"bob@example.com"')
```

## Read a message (full body, including HTML + attachments list)

```bash
python3 - <<'PY'
import os, imaplib, email, sys
from email.header import decode_header

MSG_ID = sys.argv[1] if len(sys.argv) > 1 else "1"

M = imaplib.IMAP4_SSL(os.environ["IMAP_HOST"], int(os.environ.get("IMAP_PORT", 993)))
M.login(os.environ["IMAP_USER"], os.environ["IMAP_PASSWORD"])
M.select("INBOX", readonly=True)
typ, data = M.fetch(MSG_ID.encode(), "(RFC822)")
msg = email.message_from_bytes(data[0][1])

def dec(v):
    parts = decode_header(v or "")
    return "".join(p.decode(c or "utf-8", "replace") if isinstance(p, bytes) else p for p, c in parts)

print(f"From:    {dec(msg['From'])}")
print(f"To:      {dec(msg['To'])}")
print(f"Date:    {dec(msg['Date'])}")
print(f"Subject: {dec(msg['Subject'])}\n")

if msg.is_multipart():
    for part in msg.walk():
        ct = part.get_content_type()
        disp = str(part.get("Content-Disposition") or "")
        if ct == "text/plain" and "attachment" not in disp:
            payload = part.get_payload(decode=True) or b""
            charset = part.get_content_charset() or "utf-8"
            print(payload.decode(charset, "replace"))
            break  # just the first text/plain part
    print("\n--- ATTACHMENTS ---")
    for part in msg.walk():
        if "attachment" in str(part.get("Content-Disposition") or ""):
            print(f"  {part.get_filename()} ({part.get_content_type()})")
else:
    print(msg.get_payload(decode=True).decode(msg.get_content_charset() or "utf-8", "replace"))

M.logout()
PY
# Usage: python3 script.py <message-id>
```

## Mark read / unread

```bash
python3 - <<'PY'
import os, imaplib
M = imaplib.IMAP4_SSL(os.environ["IMAP_HOST"], int(os.environ.get("IMAP_PORT", 993)))
M.login(os.environ["IMAP_USER"], os.environ["IMAP_PASSWORD"])
M.select("INBOX")  # writable
M.store(b"42", "+FLAGS", "\\Seen")   # mark read
# M.store(b"42", "-FLAGS", "\\Seen") # mark unread
# M.store(b"42", "+FLAGS", "\\Flagged") # star it
M.expunge()
M.logout()
PY
```

## Move to folder / archive / delete

Most IMAP servers support `MOVE`. Fallback is COPY + mark-deleted + EXPUNGE.

```bash
python3 - <<'PY'
import os, imaplib
M = imaplib.IMAP4_SSL(os.environ["IMAP_HOST"], int(os.environ.get("IMAP_PORT", 993)))
M.login(os.environ["IMAP_USER"], os.environ["IMAP_PASSWORD"])
M.select("INBOX")

msg_id = b"42"
dest = "Archive"

# Try MOVE first (RFC 6851, supported by most servers)
typ, _ = M.uid("MOVE", msg_id, dest) if False else M.move(msg_id, dest)
if typ != "OK":
    # Fallback: COPY + \Deleted + EXPUNGE
    M.copy(msg_id, dest)
    M.store(msg_id, "+FLAGS", "\\Deleted")
    M.expunge()

M.logout()
PY
```

Gmail-specific: "archive" means removing the `\\Inbox` label — use `M.store(id, "-X-GM-LABELS", "\\\\Inbox")`. "Trash" uses `[Gmail]/Trash`.

## List folders / mailboxes

```bash
python3 - <<'PY'
import os, imaplib
M = imaplib.IMAP4_SSL(os.environ["IMAP_HOST"], int(os.environ.get("IMAP_PORT", 993)))
M.login(os.environ["IMAP_USER"], os.environ["IMAP_PASSWORD"])
typ, data = M.list()
for row in data:
    print(row.decode())
M.logout()
PY
```

## Usage guidance

- Always `logout()` — or wrap in try/finally — to avoid lingering connections.
- Message IDs ("sequence numbers") are **not stable across sessions**. For stable IDs use UIDs: `M.uid("SEARCH", ...)`, `M.uid("FETCH", ...)`.
- IMAP dates are day-only (no time) and must be `DD-Mon-YYYY` format: `01-Jan-2026`.
- Never log the password. Never print `os.environ["IMAP_PASSWORD"]`.
- Large attachments: fetch headers first (`BODY.PEEK[HEADER]`), decide whether to pull the full RFC822 body.
- If the user mentions "OAuth Gmail" rather than an app-specific password, that's XOAUTH2 — out of scope for this skill; suggest app-password route or wait for the OAuth-flow rollout.
