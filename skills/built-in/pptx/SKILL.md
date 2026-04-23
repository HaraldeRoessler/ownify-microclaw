---
name: pptx
description: "Create, read, or edit PowerPoint presentations (.pptx). Use whenever the user mentions a slide deck, presentation, pitch deck, or .pptx file — whether the goal is producing, modifying, or extracting content. Do NOT use for Word documents, spreadsheets, or PDF-first deliverables."
license: MIT (see repository LICENSE)
---

# pptx — PowerPoint decks

Tools in the runtime:

- `python-pptx` — pythonic read/write of .pptx, slide layouts, speaker notes
- `soffice` (LibreOffice headless) — convert to/from other formats, render thumbnails
- `pdftoppm` (poppler) — rasterise slides via PDF intermediate

Use `python-pptx` for everything content-related. Reach for `soffice` only to go to/from .pptx and other formats, or to generate slide thumbnails.

## Read / extract

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

## Create a deck

```python
from pptx import Presentation
from pptx.util import Inches, Pt
from pptx.dml.color import RGBColor

prs = Presentation()                     # default 13.33" x 7.5" (16:9)

# title slide
slide = prs.slides.add_slide(prs.slide_layouts[0])
slide.shapes.title.text = "Quarterly review"
slide.placeholders[1].text = "Harald Roessler · 2026-04-23"

# content slide
slide = prs.slides.add_slide(prs.slide_layouts[1])
slide.shapes.title.text = "Headline numbers"
tf = slide.placeholders[1].text_frame
tf.text = "Revenue up 11%"
for point in ["EMEA led growth (+8%)", "APAC accelerating (+14%)", "Churn stable"]:
    p = tf.add_paragraph()
    p.text = point
    p.font.size = Pt(18)

# picture slide
slide = prs.slides.add_slide(prs.slide_layouts[6])   # blank
slide.shapes.add_picture("chart.png", Inches(1), Inches(1.5), width=Inches(8))

prs.save("out.pptx")
```

## Edit an existing deck

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

## Speaker notes

```python
from pptx import Presentation
prs = Presentation("in.pptx")
for slide in prs.slides:
    notes = slide.notes_slide.notes_text_frame
    notes.text = "Land the revenue point before moving on."
prs.save("out.pptx")
```

## Tables and charts

Tables:

```python
from pptx.util import Inches
slide = prs.slides.add_slide(prs.slide_layouts[5])
rows, cols = 3, 3
table = slide.shapes.add_table(rows, cols, Inches(1), Inches(2), Inches(8), Inches(2)).table
for j, h in enumerate(["Region", "Revenue", "Growth"]):
    table.cell(0, j).text = h
for i, (r, v, g) in enumerate([("EMEA", "€1.2M", "+8%"), ("APAC", "€0.9M", "+14%")], start=1):
    table.cell(i, 0).text, table.cell(i, 1).text, table.cell(i, 2).text = r, v, g
```

Charts are verbose through python-pptx; a pragmatic workflow for anything beyond a simple bar is to render the chart in matplotlib as a PNG and insert it with `add_picture`.

## Convert / thumbnail

```bash
soffice --headless --convert-to pdf in.pptx          # → in.pdf
pdftoppm -jpeg -r 120 in.pdf thumb                    # → thumb-1.jpg, thumb-2.jpg, …
```

Round-trip through PDF then raster is more robust than trying to render slides directly — LibreOffice handles the deck layout, poppler handles the imaging.

## Rules of thumb

- Start from the right layout: `prs.slide_layouts[0]` is title, `[1]` is title+content, `[5]` is title-only, `[6]` is blank. Inspect `prs.slide_layouts[i].name` on an unfamiliar template.
- Templates (corporate master decks) override most defaults — when the user supplies one, load it as the starting `Presentation(template.pptx)` rather than `Presentation()`.
- Fonts must exist on the system where the deck is *rendered*, not where it's *built*. LibreOffice in this image has `fonts-liberation`; for brand fonts the user wants, they must provide the .ttf files at render time.
- Images: always provide a local path; `add_picture` reads the file when called.
