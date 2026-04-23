---
name: pdf
description: "Work with PDF files — read text, extract tables, merge, split, rotate pages, add watermarks, create new PDFs, or render pages to images. Use whenever the user mentions a .pdf file or asks to produce one. Do NOT use for Word or spreadsheet output."
license: MIT (see repository LICENSE)
---

# pdf — PDF files

Tools in the runtime:

- `pypdf` — pure-Python read, merge, split, rotate, encrypt
- `pdfplumber` — text and table extraction with layout
- `reportlab` — build new PDFs programmatically
- `pdftotext`, `pdftoppm`, `pdfimages` (poppler) — fast CLI extraction and rasterisation
- `qpdf` — structural operations (repair, linearise, decrypt, page reassembly)

Default to `pdfplumber` for reading real content, `pypdf` for structural operations, `reportlab` for authoring, `pdftoppm` for thumbnails.

## Extract text

```python
import pdfplumber
with pdfplumber.open("in.pdf") as pdf:
    text = "\n\n".join((p.extract_text() or "") for p in pdf.pages)
```

Fast CLI alternative:

```bash
pdftotext -layout in.pdf out.txt          # preserve columns
pdftotext -f 1 -l 5 in.pdf out.txt         # pages 1-5
```

## Extract tables

```python
import pandas as pd
import pdfplumber

with pdfplumber.open("in.pdf") as pdf:
    tables = []
    for page in pdf.pages:
        for t in page.extract_tables():
            if t and len(t) > 1:
                tables.append(pd.DataFrame(t[1:], columns=t[0]))

combined = pd.concat(tables, ignore_index=True) if tables else pd.DataFrame()
combined.to_excel("out.xlsx", index=False)
```

For heavily visual/multi-column documents where `pdfplumber` misses rows, rasterise to images (`pdftoppm`) and run OCR or describe via the LLM's vision path — but prefer structured extraction first.

## Merge / split / reorder

```python
from pypdf import PdfReader, PdfWriter

writer = PdfWriter()
for path in ["a.pdf", "b.pdf", "c.pdf"]:
    for page in PdfReader(path).pages:
        writer.add_page(page)
with open("merged.pdf", "wb") as f:
    writer.write(f)
```

Split every page into its own file:

```python
reader = PdfReader("in.pdf")
for i, page in enumerate(reader.pages, start=1):
    w = PdfWriter()
    w.add_page(page)
    with open(f"page-{i:03d}.pdf", "wb") as f:
        w.write(f)
```

CLI equivalents with `qpdf`:

```bash
qpdf --empty --pages a.pdf b.pdf c.pdf -- merged.pdf
qpdf in.pdf --pages . 1-5 -- first-five.pdf
```

## Rotate pages

```python
reader = PdfReader("in.pdf")
writer = PdfWriter()
for page in reader.pages:
    page.rotate(90)
    writer.add_page(page)
with open("rotated.pdf", "wb") as f:
    writer.write(f)
```

## Watermark

```python
from pypdf import PdfReader, PdfWriter
mark = PdfReader("mark.pdf").pages[0]
reader = PdfReader("in.pdf")
writer = PdfWriter()
for page in reader.pages:
    page.merge_page(mark)
    writer.add_page(page)
with open("out.pdf", "wb") as f:
    writer.write(f)
```

## Render a page as an image

```bash
pdftoppm -jpeg -r 150 -f 1 -l 1 in.pdf preview
# → preview-1.jpg at 150 dpi
```

Python route if integrating into a pipeline:

```python
from pdf2image import convert_from_path
imgs = convert_from_path("in.pdf", dpi=150, first_page=1, last_page=1)
imgs[0].save("preview.jpg", "JPEG")
```

## Create a new PDF

Simple canvas:

```python
from reportlab.lib.pagesizes import A4
from reportlab.pdfgen import canvas

c = canvas.Canvas("out.pdf", pagesize=A4)
w, h = A4
c.setFont("Helvetica-Bold", 18)
c.drawString(72, h - 72, "Invoice")
c.setFont("Helvetica", 11)
c.drawString(72, h - 100, "Harald Roessler · 2026-04-23")
c.showPage()
c.save()
```

Multi-page with paragraphs and tables:

```python
from reportlab.lib.pagesizes import A4
from reportlab.lib.styles import getSampleStyleSheet
from reportlab.platypus import SimpleDocTemplate, Paragraph, Spacer, PageBreak, Table, TableStyle
from reportlab.lib import colors

styles = getSampleStyleSheet()
story = [
    Paragraph("Quarterly report", styles["Title"]),
    Spacer(1, 12),
    Paragraph("Written on behalf of the team.", styles["BodyText"]),
    PageBreak(),
    Table(
        [["Region", "Revenue", "Growth"], ["EMEA", "€1.2M", "+8%"], ["APAC", "€0.9M", "+14%"]],
        style=TableStyle([
            ("BACKGROUND", (0, 0), (-1, 0), colors.lightgrey),
            ("GRID", (0, 0), (-1, -1), 0.25, colors.grey),
        ]),
    ),
]
SimpleDocTemplate("out.pdf", pagesize=A4).build(story)
```

Subscripts and superscripts: use `<sub>`/`<super>` inside a `Paragraph`, never the unicode glyphs (ReportLab's built-in fonts don't have them and render black boxes).

## Passwords

```python
# decrypt
reader = PdfReader("enc.pdf")
reader.decrypt("password")
# encrypt
writer = PdfWriter()
for p in reader.pages: writer.add_page(p)
writer.encrypt("user-pw", "owner-pw")
```

Or via qpdf:

```bash
qpdf --password=pw --decrypt enc.pdf out.pdf
```

## Extract embedded images

```bash
pdfimages -j in.pdf img   # img-000.jpg, img-001.jpg, …
```

## Rules of thumb

- Text extraction quality is "it depends on the producer". Always inspect a page or two before trusting the result.
- A scanned PDF has no text layer — `pdfplumber` returns empty strings. Detect by checking if extracted text is empty across several pages, then fall back to rasterising + OCR (tesseract is not in this image; either request it be added, or render + send to a vision model).
- Prefer CLI tools for structural work on large PDFs — `qpdf` and `pdftotext` are orders of magnitude faster than Python for bulk operations.
- `page.extract_text()` returning `None` means the page had no text objects, not that extraction failed.
