---
name: sendgrid
description: "Send transactional email through SendGrid. Use when the user asks to send an email, notification, alert, or message via email (and their agent has a SendGrid API key configured). Triggers on mentions of email, send mail, notification email, reminder email, alert. Requires SENDGRID_API_KEY and SENDGRID_FROM_EMAIL env vars on the agent."
license: MIT (see repository LICENSE)
compatibility: "Requires curl (bundled in image). Works on Linux."
---

# SendGrid Skill

Send email via the SendGrid v3 REST API. Uses curl directly — no SDK dependency.

## Prerequisites

Two environment variables must be set on the agent (configure from the portal Settings → Skills → SendGrid):

- `SENDGRID_API_KEY` — a key from https://app.sendgrid.com/settings/api_keys with at least `Mail Send` permission.
- `SENDGRID_FROM_EMAIL` — a verified sender address. Must match a Single Sender or authenticated domain in your SendGrid account.

If either is missing, every send below returns an HTTP error — surface the exact response body to the user so they know what to fix.

## Simple send

```bash
curl -sS -w "\nHTTP %{http_code}\n" -X POST https://api.sendgrid.com/v3/mail/send \
  -H "Authorization: Bearer ${SENDGRID_API_KEY}" \
  -H "Content-Type: application/json" \
  --data @- <<JSON
{
  "personalizations": [{"to": [{"email": "recipient@example.com"}]}],
  "from": {"email": "${SENDGRID_FROM_EMAIL}"},
  "subject": "Hello from klaw",
  "content": [{"type": "text/plain", "value": "This is the body of the message."}]
}
JSON
```

A successful send returns `HTTP 202` with an empty body. Anything else (400 bad request, 401 unauthorized, 403 forbidden) — check the JSON body.

## HTML + plain-text multipart

```bash
curl -sS -w "\nHTTP %{http_code}\n" -X POST https://api.sendgrid.com/v3/mail/send \
  -H "Authorization: Bearer ${SENDGRID_API_KEY}" \
  -H "Content-Type: application/json" \
  --data @- <<JSON
{
  "personalizations": [{"to": [{"email": "recipient@example.com", "name": "Jane Doe"}]}],
  "from": {"email": "${SENDGRID_FROM_EMAIL}", "name": "klaw agent"},
  "reply_to": {"email": "${SENDGRID_FROM_EMAIL}"},
  "subject": "Weekly summary",
  "content": [
    {"type": "text/plain", "value": "Plain-text fallback goes here."},
    {"type": "text/html",  "value": "<p>The <strong>HTML</strong> version.</p>"}
  ]
}
JSON
```

`text/plain` MUST come before `text/html` in the `content` array — SendGrid requires that order.

## Multiple recipients + CC / BCC

```bash
curl -sS -X POST https://api.sendgrid.com/v3/mail/send \
  -H "Authorization: Bearer ${SENDGRID_API_KEY}" \
  -H "Content-Type: application/json" \
  --data @- <<'JSON'
{
  "personalizations": [{
    "to":  [{"email": "alice@example.com"}, {"email": "bob@example.com"}],
    "cc":  [{"email": "team-lead@example.com"}],
    "bcc": [{"email": "archive@example.com"}]
  }],
  "from": {"email": "SENDER_PLACEHOLDER"},
  "subject": "Group update",
  "content": [{"type": "text/plain", "value": "See above."}]
}
JSON
```
(Substitute `SENDER_PLACEHOLDER` with `${SENDGRID_FROM_EMAIL}` at send time — heredoc with `'JSON'` is quoted so no shell expansion; switch to unquoted `JSON` if you need expansion.)

## Send with attachment

Attach a file by base64-encoding its bytes and including it in the `attachments` array:

```bash
ATTACH_B64=$(base64 -w 0 /path/to/report.pdf)
python3 - <<PY
import json, os
payload = {
  "personalizations": [{"to": [{"email": "recipient@example.com"}]}],
  "from": {"email": os.environ["SENDGRID_FROM_EMAIL"]},
  "subject": "Monthly report",
  "content": [{"type": "text/plain", "value": "Report attached."}],
  "attachments": [{
    "content": "${ATTACH_B64}",
    "filename": "report.pdf",
    "type": "application/pdf",
    "disposition": "attachment"
  }]
}
print(json.dumps(payload))
PY
```

Pipe the resulting JSON to `curl --data @- ...` as in the simple-send example. Attachment size limit is 30 MB per email combined.

## Templates

If the user manages dynamic templates in SendGrid, send by template ID:

```bash
curl -sS -X POST https://api.sendgrid.com/v3/mail/send \
  -H "Authorization: Bearer ${SENDGRID_API_KEY}" \
  -H "Content-Type: application/json" \
  --data @- <<JSON
{
  "personalizations": [{
    "to": [{"email": "recipient@example.com"}],
    "dynamic_template_data": {"firstName": "Alice", "orderId": "ORD-0421"}
  }],
  "from": {"email": "${SENDGRID_FROM_EMAIL}"},
  "template_id": "d-xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx"
}
JSON
```

(Template IDs always start with `d-` and are found at https://mc.sendgrid.com/dynamic-templates.)

## Usage guidance

- Always report the HTTP status and response body back to the user — SendGrid's error JSON is specific and actionable.
- Sandbox mode (adds `"mail_settings": {"sandbox_mode": {"enable": true}}`) lets you validate a send without delivering — useful before blasting a large list.
- Do not log the API key or `SENDGRID_API_KEY` verbatim in chat output.
- If the user mentions receiving a bounce or spam issue, recommend they check the SendGrid Activity dashboard rather than guessing.
