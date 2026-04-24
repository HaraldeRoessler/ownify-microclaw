---
name: x-twitter
description: "Read timeline + mentions, post tweets, reply, like, retweet, and search on X (formerly Twitter). Use when the user asks to tweet, post to X, check mentions, reply to a tweet, read their timeline, search X for topics. Requires X_BEARER_TOKEN env var on the agent — obtained from developer.x.com. Each agent should have its own X account / token; do not share tokens across agents."
license: MIT (see repository LICENSE)
compatibility: "Requires python3 with tweepy installed (bundled in image). Works on Linux."
---

# X (Twitter) Skill

Post + read + reply on X using the v2 REST API via `tweepy`. Authenticates with a Bearer Token from developer.x.com — scoped to the X account that issued it, so each agent gets its own voice.

## Prerequisites

One env var on the agent (configure from portal Settings → Skills → X):

- `X_BEARER_TOKEN` — OAuth2 User Context token with scopes `tweet.read tweet.write users.read follows.read like.write offline.access`. Generated at https://developer.x.com/en/portal/dashboard → your app → "Keys and Tokens" → "OAuth 2.0 Access Token". Free tier (Basic) is sufficient for posting + reading own timeline; higher tiers add search + engagement metrics.

## Post a tweet

```bash
python3 - <<'PY'
import os, tweepy, json
client = tweepy.Client(bearer_token=os.environ["X_BEARER_TOKEN"])
r = client.create_tweet(text="Hello from my klaw agent.")
print(json.dumps({"id": r.data["id"], "text": r.data["text"]}, indent=2))
PY
```

If the post fails, tweepy raises `tweepy.errors.*` — surface the error to the user unchanged. Common failures: 401 (token expired — re-issue), 403 (write scope missing), 429 (rate limited — wait + retry).

## Reply to a tweet

```bash
python3 - <<'PY'
import os, tweepy, sys
PARENT_TWEET_ID = sys.argv[1] if len(sys.argv) > 1 else "1234567890"
REPLY_TEXT = sys.argv[2] if len(sys.argv) > 2 else "Thanks for the mention."
client = tweepy.Client(bearer_token=os.environ["X_BEARER_TOKEN"])
r = client.create_tweet(text=REPLY_TEXT, in_reply_to_tweet_id=PARENT_TWEET_ID)
print(f"Replied. new tweet id: {r.data['id']}")
PY
```

## Read own recent mentions

```bash
python3 - <<'PY'
import os, tweepy, json
client = tweepy.Client(bearer_token=os.environ["X_BEARER_TOKEN"])
me = client.get_me().data
mentions = client.get_users_mentions(
    id=me.id,
    max_results=20,
    tweet_fields=["created_at", "author_id", "conversation_id", "public_metrics"],
    expansions=["author_id"],
    user_fields=["username", "name"],
)
users = {u["id"]: u for u in (mentions.includes.get("users") or [])}
for t in (mentions.data or []):
    u = users.get(t.author_id, {})
    print(f"@{u.get('username','?')} ({t.created_at}): {t.text}")
PY
```

## Read own timeline

```bash
python3 - <<'PY'
import os, tweepy
client = tweepy.Client(bearer_token=os.environ["X_BEARER_TOKEN"])
me = client.get_me().data
tweets = client.get_users_tweets(
    id=me.id,
    max_results=20,
    tweet_fields=["created_at", "public_metrics"],
    exclude=["retweets", "replies"],
)
for t in (tweets.data or []):
    m = t.public_metrics
    print(f"[{t.created_at}] likes={m.get('like_count')} rt={m.get('retweet_count')} — {t.text[:140]}")
PY
```

## Search recent tweets (paid tier only)

```bash
python3 - <<'PY'
import os, tweepy, sys
QUERY = sys.argv[1] if len(sys.argv) > 1 else "klaw"
client = tweepy.Client(bearer_token=os.environ["X_BEARER_TOKEN"])
results = client.search_recent_tweets(
    query=QUERY,
    max_results=20,
    tweet_fields=["created_at", "author_id", "public_metrics"],
)
for t in (results.data or []):
    print(f"[{t.created_at}] {t.id}: {t.text[:140]}")
PY
```

Note: `search_recent_tweets` requires at least Basic tier ($100/mo). Free tier returns 403. If the agent gets 403 on search, explain that to the user rather than pretending it's broken.

## Delete own tweet

```bash
python3 - <<'PY'
import os, tweepy, sys
TWEET_ID = sys.argv[1]
client = tweepy.Client(bearer_token=os.environ["X_BEARER_TOKEN"])
r = client.delete_tweet(id=TWEET_ID)
print("Deleted." if r.data.get("deleted") else "Delete returned unexpected response.")
PY
```

## Like / unlike

```bash
python3 - <<'PY'
import os, tweepy, sys
TWEET_ID = sys.argv[1]
client = tweepy.Client(bearer_token=os.environ["X_BEARER_TOKEN"])
client.like(tweet_id=TWEET_ID)
# client.unlike(tweet_id=TWEET_ID)
print("OK")
PY
```

## Usage guidance

- X rate limits are strict and enforced per-token. For user-context posting: 200 tweets / 15 min / user (Basic).
- Never log `X_BEARER_TOKEN`. Never print `os.environ["X_BEARER_TOKEN"]` in debug output.
- Tweet text is limited to 280 chars (Basic) / 4000 chars (Premium subscribers posting). When composing, count characters — tweepy does not auto-truncate.
- For threads, the agent should post the first tweet, capture its id, then post each subsequent tweet with `in_reply_to_tweet_id=<prev_id>`.
- Images/videos require a 3-step flow (upload → receive media_id → attach on create_tweet). Not included here; ask the user if they need multimedia support and we'll expand the skill.
- If the agent needs to check "am I authenticated correctly" before posting, call `client.get_me()` — returns 200 with user data if the token is valid, 401 otherwise.
