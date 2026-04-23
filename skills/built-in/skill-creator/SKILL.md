---
name: skill-creator
description: "Help the user write a new MicroClaw skill or revise an existing one. Use when the user asks to create a skill, write a SKILL.md, update an existing skill, or asks how skill triggers work. Triggers on mentions of new skill, create skill, SKILL.md, skill template, skill frontmatter."
license: MIT (see repository LICENSE)
---

# skill-creator — Write or revise a MicroClaw skill

A *skill* in MicroClaw is a single `SKILL.md` file (optionally with sibling files) that lives under `<data_dir>/skills/<name>/` on the tenant PVC, or under `skills/built-in/<name>/` in the bundled image. The file is loaded into the system prompt when its trigger conditions match the user's message.

## Anatomy

```markdown
---
name: <short-lowercase-name>
description: "One paragraph. What the skill does + when the agent should use it + when NOT to use it. The description is ALL the classifier sees when deciding to load the skill, so be concrete about triggers."
license: <MIT or other, as applicable>
---

# <human title>

<body: concise, imperative recipes, ≤ 250 lines where possible>
```

Only `name` and `description` are load-bearing. Everything else is conventional.

## Writing the `description`

The description is the trigger. It must:

- State what the skill does in one sentence.
- List the kinds of user requests that should load it ("triggers on mentions of X, Y, Z").
- Exclude adjacent skills ("Do NOT use for …") so two skills don't fire on the same request.

Bad: "General document tools."
Good: "Work with Word documents (.docx). Use when the user mentions Word, .docx, or asks to produce a report, memo, letter, or template. Do NOT use for spreadsheets, PDFs, or plain Markdown output."

## Writing the body

The body is the prompt fragment the agent reads when the skill fires. Optimise for the agent, not a human reader.

- Lead with the **tools available** in the runtime so the agent picks the right one. If a tool isn't installed, don't mention it.
- Use short, pasteable code blocks. The agent will copy and adapt them.
- Prefer **library calls** over shell helper scripts: libraries are composable, scripts add a file-path dependency.
- Call out **rules of thumb** at the bottom — things that are easy to get wrong and hard to debug.
- Do not copy content from other skill bundles without verifying the license permits redistribution.

Keep the body tight. A skill that reads like documentation ("Overview … Background … History …") wastes prompt tokens every time it loads. Aim for ~100–250 lines.

## Bundled resources

If the skill needs non-prompt files (templates, lookup tables, JSON schemas), drop them alongside the SKILL.md:

```
skills/built-in/my-skill/
  SKILL.md
  template.docx
  data/regions.json
```

Reference them by relative path from SKILL.md. MicroClaw mounts the skill directory so relative paths resolve at runtime.

## Where skills live

- **Bundled** — committed to `skills/built-in/<name>/` in this repo. Available to every tenant after image build. Use for skills that ship with the platform.
- **Tenant PVC** — written to `<data_dir>/skills/<name>/` inside the tenant pod. Survives restarts, travels with the tenant, does not require an image rebuild. Use for tenant-specific or user-authored skills.

If a name collides, the PVC copy wins. This lets tenants override built-ins without a redeploy.

## Minimum viable skill

```markdown
---
name: convert-temp
description: "Convert between Celsius and Fahrenheit. Use when the user asks for a temperature conversion, or mentions °C, °F, Celsius, Fahrenheit, or degrees."
license: MIT
---

# convert-temp

Celsius → Fahrenheit: `f = c * 9 / 5 + 32`
Fahrenheit → Celsius: `c = (f - 32) * 5 / 9`

Round to one decimal unless the user asked for more precision.
```

## Testing a new skill

1. Drop the file into `skills/built-in/<name>/SKILL.md`.
2. Rebuild and roll the MicroClaw image (or scp to the PVC for hot-iteration).
3. Send a test message whose wording should trigger the skill. Watch the agent's tool calls to confirm the skill loaded and the recipes are being followed.
4. Iterate the description if the classifier doesn't pick the skill up reliably.

## Pitfalls

- A too-generic description will make the skill fire on unrelated requests, polluting the system prompt.
- A too-specific description will cause the skill to miss obvious triggers. Ask yourself "what other words would a user actually type?" and include them.
- Telling the agent to run a script that isn't in the image is a silent failure at runtime. Verify every CLI tool / library mentioned in the body is actually installed.
- License-by-copy from other bundles is a liability — write fresh content, or note an upstream permissive license explicitly.
