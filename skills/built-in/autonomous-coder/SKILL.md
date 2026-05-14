---
name: autonomous-coder
description: "Autonomous software development mode. When activated, the agent can read code, propose fixes, create git branches, edit files, run tests, commit changes, and trigger deployments. Always follows safety-first workflow: branch → edit → test → commit → deploy."
version: 1.0.0
author: ownify
license: MIT
---

# Autonomous Coder Skill

## When to activate

Activate this skill when:
- The user asks to "fix", "implement", "add", "refactor", or "debug" code
- An error pattern suggests a code change is needed
- The user says "autonomous mode", "coding mode", or "develop this"
- A scheduled task or monitoring alert indicates a code issue

## Core workflow (ALWAYS follow this order)

```
1. PLAN    → Understand the task, identify files involved
2. BRANCH  → Create a feature branch (never edit main/master directly)
3. EDIT    → Read files, make minimal changes, validate syntax
4. TEST    → Run build/tests to verify the change works
5. COMMIT  → git add + git commit with descriptive message
6. DEPLOY  → Trigger rollout via CP API (if tests pass)
```

## Step 1: Plan

Before touching any file:
- Use `glob` to find relevant files (e.g., `**/*router*.js` for router issues)
- Use `read_file` on the main files involved
- Identify the root cause, not just symptoms
- Write a 1-sentence plan to the user: "I'll fix the 503 retry logic in `completions.js` by adding exponential backoff"

## Step 2: Branch

```bash
cd /home/microclaw/.microclaw/workspace
git checkout -b agent-fix-$(date +%Y%m%d)-$(echo $RANDOM | md5sum | head -c 6)
```

**Rule**: NEVER commit to `main`, `master`, or `production` branches. Always create a feature branch.

## Step 3: Edit

- Use `read_file` before editing (don't guess file contents)
- Use `edit_file` for precise changes (preferred over `write_file` for existing files)
- Use `write_file` only for new files
- Make MINIMAL changes — one fix per commit
- If the change spans >5 files, split into multiple branches

## Step 4: Test

After every edit:
```bash
# Check syntax (for Node.js)
node --check <file>

# Or run tests if available
npm test
# or
make test
# or
cargo test
```

**Rule**: If tests fail, STOP. Do not commit. Fix the test failure first.

## Step 5: Commit

```bash
git add -A
git diff --cached  # Review what you're about to commit
git commit -m "fix: descriptive message explaining WHY not WHAT"
```

Commit message format:
- `fix: <what>` for bug fixes
- `feat: <what>` for new features
- `refactor: <what>` for cleanups
- `docs: <what>` for documentation

**Rule**: Commit messages must explain WHY the change was needed, not just WHAT changed.

## Step 6: Deploy

If tests pass and the user confirmed (or auto-deploy is enabled):
```bash
# Push branch first
git push origin HEAD

# Trigger deployment via CP API
curl -sS -X POST \
  -H "Authorization: Bearer $OWNIFY_ROUTER_ADMIN_SECRET" \
  -H "Content-Type: application/json" \
  -d '{"service":"ownify-router","image":"ghcr.io/haralderoessler/ownify-router:latest"}' \
  http://ownify-control-plane.ownify.svc.cluster.local:4000/api/admin/deploy
```

**Rule**: Only deploy if:
1. Tests passed
2. The change is on a branch (not main)
3. The user explicitly approved OR auto-deploy policy is enabled

## Safety guardrails (ABSOLUTE RULES)

### File system boundaries
- NEVER edit files outside `/home/microclaw/.microclaw/workspace`
- NEVER touch `node_modules/`, `.git/`, `.env`, `secrets/`, `credentials/`
- NEVER run `rm -rf /` or any recursive delete on system directories
- NEVER run `curl ... | bash` or pipe remote scripts to shell

### Git safety
- NEVER force-push (`git push -f`)
- NEVER amend published commits (`git commit --amend` after push)
- NEVER rewrite history on shared branches
- Always create a branch before editing

### Deployment safety
- NEVER deploy to production without tests passing
- NEVER deploy on Friday evening or weekends (unless critical fix)
- Always verify deployment succeeded before declaring done

### Secret handling
- NEVER hardcode API keys, passwords, or tokens in code
- NEVER commit `.env` files or credential files
- If a secret is accidentally committed, IMMEDIATELY rotate it and notify the user

## Error handling

If any step fails:
1. Report the error clearly to the user
2. Do NOT proceed to the next step
3. Offer rollback: `git checkout -- .` to discard changes
4. If tests fail after commit, create a fixup commit — do not amend

## Self-initiation (proactive mode)

When monitoring is enabled, the agent can self-initiate fixes:
1. Watch error logs via `bash(command="tail -f /var/log/...")`
2. Detect patterns (e.g., 3x 503 in 5 minutes)
3. Follow the full workflow above
4. Report to user: "I detected X and created branch Y with fix Z. Deploy?"

## Example session

```
User: "The router is getting 503s from OpenRouter"

Agent:
1. PLAN: "I'll add retry logic with fallback to the builtin provider"
2. BRANCH: `git checkout -b agent-fix-2026-05-14-retry`
3. EDIT: Read `completions.js`, add `fetchUpstream()` function
4. TEST: `node --check completions.js`
5. COMMIT: `git commit -m "fix: add retry+falllback for upstream 503s"`
6. REPORT: "Fixed in branch `agent-fix-2026-05-14-retry`. Tests pass. Deploy?"
```

## Deactivation

This skill deactivates when:
- The user says "stop", "done", "exit", or "quit"
- The task is complete (committed and optionally deployed)
- The user switches to a different context (e.g., "now let's chat about something else")
