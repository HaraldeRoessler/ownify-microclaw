---
name: linkedin
description: "Post updates and articles, fetch your own profile, and interact with LinkedIn via the v2 REST API. Use when the user asks to post on LinkedIn, share an article, check their LinkedIn profile, or DM a connection. Requires LINKEDIN_ACCESS_TOKEN env var on the agent — an OAuth 2.0 access token from a LinkedIn developer app. Each agent should have its own token; tokens are scoped to the account that authorized them."
license: MIT (see repository LICENSE)
compatibility: "Requires curl (bundled in image). Works on Linux."
---

# LinkedIn Skill

Post + read on LinkedIn via the v2 REST API. Uses curl directly — no SDK. Authenticates via OAuth 2.0 Bearer token.

## Prerequisites

One env var on the agent (configure from portal Settings → Skills → LinkedIn):

- `LINKEDIN_ACCESS_TOKEN` — OAuth 2.0 access token. Generate from https://www.linkedin.com/developers/apps → your app → "Auth" tab → "OAuth 2.0 tools" → "Generate access token". Minimum scopes for this skill:
  - `w_member_social` — post shares on your behalf
  - `openid profile email` — read your own profile (OIDC userinfo)
  - `r_liteprofile r_emailaddress` (legacy) may be required for some endpoints.

Tokens expire (typically 60 days). When a call returns 401, the agent should tell the user to re-generate + paste the new token.

## Get your own profile (OIDC userinfo)

```bash
curl -sS -H "Authorization: Bearer ${LINKEDIN_ACCESS_TOKEN}" \
  https://api.linkedin.com/v2/userinfo
```

Returns `{ sub: "<member-id>", name, given_name, family_name, picture, email, email_verified, locale }`. The `sub` field is the LinkedIn member ID used in `author` URNs below. Cache it for subsequent post calls.

## Post a text share

```bash
MEMBER_ID=$(curl -sS -H "Authorization: Bearer ${LINKEDIN_ACCESS_TOKEN}" \
  https://api.linkedin.com/v2/userinfo | python3 -c "import sys,json; print(json.load(sys.stdin)['sub'])")

curl -sS -X POST https://api.linkedin.com/v2/ugcPosts \
  -H "Authorization: Bearer ${LINKEDIN_ACCESS_TOKEN}" \
  -H "Content-Type: application/json" \
  -H "X-Restli-Protocol-Version: 2.0.0" \
  --data @- <<JSON
{
  "author": "urn:li:person:${MEMBER_ID}",
  "lifecycleState": "PUBLISHED",
  "specificContent": {
    "com.linkedin.ugc.ShareContent": {
      "shareCommentary": {"text": "Hello from my klaw agent."},
      "shareMediaCategory": "NONE"
    }
  },
  "visibility": {"com.linkedin.ugc.MemberNetworkVisibility": "PUBLIC"}
}
JSON
```

`visibility` values: `PUBLIC` (anyone on LinkedIn) or `CONNECTIONS` (your network only).

## Post a link share (with preview card)

LinkedIn auto-generates a preview card from the URL.

```bash
MEMBER_ID=$(curl -sS -H "Authorization: Bearer ${LINKEDIN_ACCESS_TOKEN}" \
  https://api.linkedin.com/v2/userinfo | python3 -c "import sys,json; print(json.load(sys.stdin)['sub'])")

curl -sS -X POST https://api.linkedin.com/v2/ugcPosts \
  -H "Authorization: Bearer ${LINKEDIN_ACCESS_TOKEN}" \
  -H "Content-Type: application/json" \
  -H "X-Restli-Protocol-Version: 2.0.0" \
  --data @- <<JSON
{
  "author": "urn:li:person:${MEMBER_ID}",
  "lifecycleState": "PUBLISHED",
  "specificContent": {
    "com.linkedin.ugc.ShareContent": {
      "shareCommentary": {"text": "Sharing an article I found useful."},
      "shareMediaCategory": "ARTICLE",
      "media": [{
        "status": "READY",
        "originalUrl": "https://example.com/some-article",
        "title": {"text": "Optional override title"},
        "description": {"text": "Optional override description"}
      }]
    }
  },
  "visibility": {"com.linkedin.ugc.MemberNetworkVisibility": "PUBLIC"}
}
JSON
```

A successful post returns HTTP 201 with the new post's URN in the `x-restli-id` header.

## Delete a post

```bash
POST_URN="urn:li:ugcPost:7000000000000000000"  # from create-post response
curl -sS -X DELETE \
  -H "Authorization: Bearer ${LINKEDIN_ACCESS_TOKEN}" \
  -H "X-Restli-Protocol-Version: 2.0.0" \
  "https://api.linkedin.com/v2/ugcPosts/${POST_URN}"
```

## Check auth before acting

If the agent is uncertain whether the token is still valid:

```bash
curl -sS -o /dev/null -w "%{http_code}\n" \
  -H "Authorization: Bearer ${LINKEDIN_ACCESS_TOKEN}" \
  https://api.linkedin.com/v2/userinfo
```

- `200` → good
- `401` → expired, re-generate from the developer portal
- `403` → missing scope, re-authorize with the right scopes selected

## Usage guidance

- LinkedIn is STRICT about rate limits + API-terms compliance. Don't auto-post aggressively; LinkedIn can suspend an app for posting spam.
- Never log `LINKEDIN_ACCESS_TOKEN`.
- Image + video posts require a multi-step "register upload → upload binary → include media URN in post" flow. Not shown here. Ask the user if needed and we can extend.
- LinkedIn's "InMail", connection search, and feed-read endpoints are behind partner-only products (`r_member_social`, `r_network`) and not available to standard developer apps. Don't pretend these work with a standard token.
- For "who viewed my profile" or "my inbox" read access, you need LinkedIn's Sales Navigator or Recruiter API — both require a commercial agreement. Out of scope for this skill.
- On 401/403, give the user the SPECIFIC fix: 401 → re-generate token; 403 → re-authorize with the missing scope.
