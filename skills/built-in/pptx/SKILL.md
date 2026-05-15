---
name: pptx
description: "Create, read, or edit PowerPoint presentations (.pptx). Use whenever the user mentions a slide deck, presentation, pitch deck, or .pptx file — whether the goal is producing, modifying, or extracting content. Do NOT use for Word documents, spreadsheets, or PDF-first deliverables."
license: MIT (see repository LICENSE)
---

# pptx — PowerPoint decks

Tools in the runtime:

- **`marp`** (Node) — convert Markdown to .pptx with built-in themes. **Preferred for creating new presentations.**
- `python-pptx` — pythonic read/write of .pptx, slide layouts, speaker notes. **Use for editing/updating existing decks** (template fills, text replacements, speaker notes).
- `soffice` (LibreOffice headless) — convert to/from other formats, render thumbnails.
- `pdftoppm` (poppler) — rasterise slides via PDF intermediate.

**Decision rule:**
- User says "create a presentation" or "make a slide deck" → **use Marp** (Markdown → pptx)
- User says "edit this slide" or "update the template" or "add notes" → **use python-pptx** (programmatic manipulation)

---

## Create a deck — Marp (Markdown → pptx)

Write a Markdown file with Marp front-matter, then convert with `marp`.

### Basic structure

```markdown
---
marp: true
theme: uncover
---

# Main Title
## Subtitle · Date

---

# Slide Title

- Bullet point one
- Bullet point two
- **Bold** and *italic* text

---

![bg left:40%](image.png)

# Split Layout

Content on the right side of the slide.
```

### Then convert

```bash
marp slides.md -o out.pptx
```

### Themes

| Theme    | Style               | Best for                    |
|----------|---------------------|-----------------------------|
| `uncover`| Clean, modern       | Default — professional      |
| `gaia`   | Bold, colorful      | Creative / pitch decks      |
| `default`| Minimal             | Internal / drafts           |

Set in front-matter: `theme: gaia`

### Slide features (Markdown → pptx)

| Feature | Syntax |
|---------|--------|
| Background image | `![bg](bg.png)` or `![bg right:40%](chart.png)` |
| Background color | `<!-- backgroundColor: #e0f0ff -->` |
| Split columns | `![bg left:50%](img.png)` in same slide as content |
| Tables | Standard Markdown tables |
| Code blocks | ` ```python` with optional filename ` ```python title="script.py"` |
| Lists | Standard Markdown bullets and numbered lists |
| Images | `![Caption](file.png)` — path relative to the .md file |
| Multi-column text | Not native — use table layout instead |
| Speaker notes | `<!-- _footer: "speaker note here" -->` |
| Slide numbers | Set in front-matter: `paginate: true` |
| Header/footer | `header: "Section title"` and `footer: "Confidential"` in front-matter |

### Example: full deck with multiple features

```markdown
---
marp: true
theme: uncover
paginate: true
header: "Q1 Review · Confidential"
---

# Quarterly Review
## Harald Roessler · 2026-05-15

---

# Agenda

1. Executive summary
2. Financial highlights
3. Product roadmap
4. Q&A

---

# Revenue Overview

| Region    | Q1 2025 | Q1 2026 | Change |
|-----------|---------|---------|--------|
| EMEA      | €3.8M   | €4.2M   | +11%   |
| APAC      | €2.1M   | €2.8M   | +33%   |
| North Am. | €4.7M   | €5.1M   | +9%    |

---

![bg right:45%](growth-chart.png)

# Key Drivers

- **EMEA**: New enterprise deals closed in DE/FR
- **APAC**: Partner channel expansion in Singapore
- **NA**: Renewal rates stable at 94 %

---

# Product Roadmap

| Quarter | Milestone |
|---------|-----------|
| Q2 2026 | v3.0 release · AI search GA |
| Q3 2026 | Mobile apps (iOS + Android) |
| Q4 2026 | Enterprise SSO + audit |

---

# Thank You

## Questions?

Contact: harald.roessler@example.com
```

### Images in Marp

Images must exist as files in the working directory before `marp` runs. Copy or download them first:

```bash
cp /some/path/chart.png ./
marp slides.md -o out.pptx
```

---

## Read / extract content from existing pptx

```python
from pptx import Presentation

prs = Presentation("in.pptx")
for i, slide in enumerate(prs.slides, 1):
    print(f"--- slide {i} ---")
    for shape in slide.shapes:
        if shape.has_text_frame:
            for para in shape.text_frame.paragraphs:
                print("".join(r.text for r in para.runs))
    if slide.has_notes_slide:
        print("NOTES:", slide.notes_slide.notes_text_frame.text)
```

---

## Edit an existing deck (template fill, text replace)

```python
from pptx import Presentation
prs = Presentation("in.pptx")
for slide in prs.slides:
    for shape in slide.shapes:
        if shape.has_text_frame:
            for p in shape.text_frame.paragraphs:
                for run in p.runs:
                    run.text = run.text.replace("{{YEAR}}", "2026")
prs.save("out.pptx")
```

Run-level replacement preserves font, weight, colour. Paragraph-level `text = ` assignment discards all inline formatting.

---

## Speaker notes

```python
from pptx import Presentation
prs = Presentation("in.pptx")
for slide in prs.slides:
    notes = slide.notes_slide.notes_text_frame
    notes.text = "Land the revenue point before moving on."
prs.save("out.pptx")
```

---

## Convert / thumbnail

```bash
soffice --headless --convert-to pdf in.pptx          # → in.pdf
pdftoppm -jpeg -r 120 in.pdf thumb                    # → thumb-1.jpg, thumb-2.jpg, …
```

Round-trip through PDF then raster is more robust than trying to render slides directly — LibreOffice handles the deck layout, poppler handles the imaging.

---

## Rules of thumb

- **New presentations → Marp.** Faster, more reliable, the LLM writes Markdown not buggy Python.
- **Editing existing decks → python-pptx.** Marp cannot round-trip existing .pptx files — always use python-pptx for modifications.
- Templates (corporate master decks) override most defaults — when the user supplies one, load it as `Presentation(template.pptx)`.
- Fonts must exist on the system where the deck is *rendered*, not where it's *built*. LibreOffice in this image has `fonts-liberation`; for brand fonts the user wants, they must provide the .ttf files at render time.
- Images in Marp: always provide a local path. Copy the file to the working directory before running `marp`.
