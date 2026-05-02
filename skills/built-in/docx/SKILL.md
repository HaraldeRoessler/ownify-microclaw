---
name: docx
description: "Create, read, or edit Microsoft Word documents (.docx). Use when the user asks to produce a report, memo, letter, or template as a Word document, or to extract/modify content inside an existing .docx. Triggers on mentions of Word, .docx, docx, or any request whose deliverable is a Word file. Do NOT use for PDFs, spreadsheets, or plain Markdown output."
license: MIT (see repository LICENSE)
---

# docx — Word documents

Tools available in the runtime:

- `python-docx` — pythonic read/write of .docx, preserves most formatting
- `pandoc` — convert between .docx / Markdown / HTML
- `docx` (npm, globally installed) — JavaScript builder for rich new documents
- `soffice` (LibreOffice headless) — convert .doc→.docx, render to PDF
- `pdftoppm` (poppler) — rasterise PDF pages to PNG for previews

Pick the smallest tool that does the job.

## Read / extract text

```python
from docx import Document
doc = Document("input.docx")
for p in doc.paragraphs:
    print(p.text)
for t in doc.tables:
    for row in t.rows:
        print([c.text for c in row.cells])
```

Or via pandoc for clean Markdown:

```bash
pandoc -f docx -t gfm input.docx -o out.md
```

## Convert legacy `.doc` → `.docx`

```bash
soffice --headless --convert-to docx input.doc
```

## Create a new document

```python
from docx import Document
from docx.shared import Pt, Cm

doc = Document()
doc.add_heading("Quarterly report", level=1)
doc.add_paragraph("Written on behalf of the team.")

table = doc.add_table(rows=1, cols=3)
hdr = table.rows[0].cells
hdr[0].text, hdr[1].text, hdr[2].text = "Region", "Revenue", "Growth"
for region, rev, gr in [("EMEA", "€1.2M", "+8%"), ("APAC", "€0.9M", "+14%")]:
    row = table.add_row().cells
    row[0].text, row[1].text, row[2].text = region, rev, gr

doc.save("out.docx")
```

For documents with a table of contents, complex numbering, or header/footer page numbers, `docx` (npm) is more ergonomic:

```bash
node -e '
const {Document, Packer, Paragraph, TextRun, HeadingLevel, TableOfContents} = require("docx");
const fs = require("fs");
const doc = new Document({
  sections: [{ children: [
    new Paragraph({ children: [new TextRun("Contents")] }),
    new TableOfContents("Contents", { hyperlink: true, headingStyleRange: "1-3" }),
    new Paragraph({ heading: HeadingLevel.HEADING_1, children: [new TextRun("Chapter 1")] }),
  ]}]
});
Packer.toBuffer(doc).then(b => fs.writeFileSync("out.docx", b));
'
```

## Edit an existing document in place

Modify paragraphs directly with python-docx — it preserves styles, images, and most layout on round-trip:

```python
from docx import Document
doc = Document("in.docx")
for p in doc.paragraphs:
    if "{{NAME}}" in p.text:
        for run in p.runs:
            run.text = run.text.replace("{{NAME}}", "Harald")
doc.save("out.docx")
```

Run-level replacement (not paragraph-level) is important when the substring spans only part of a styled run — replacing at paragraph level destroys inline formatting.

## Convert to PDF

```bash
soffice --headless --convert-to pdf input.docx
```

Render a thumbnail:

```bash
soffice --headless --convert-to pdf input.docx
pdftoppm -jpeg -r 150 -f 1 -l 1 input.pdf preview
# → preview-1.jpg
```

## Tracked changes / comments

python-docx does not model tracked changes. Use pandoc to read them:

```bash
pandoc --track-changes=all input.docx -o out.md
```

For documents where the user explicitly asked for *insertions as tracked changes*, current tooling cannot produce valid `<w:ins>` markup cleanly. Prefer producing a clean revised document + a summary of what changed, unless the user has a workflow that requires the tracked-changes XML.

## Rules of thumb

- Always set explicit page size if the output will be printed (`Document().sections[0].page_width/height`). The default is A4.
- Don't emit unicode bullet glyphs (`•`, `•`) — use `doc.add_paragraph("…", style="List Bullet")`.
- Tables need explicit column widths (`cell.width = Cm(5)`) on each row, or Word's auto-layout will override you.
- After any write, the file should open cleanly in Word / LibreOffice. If the user reports an error, `soffice --headless --convert-to docx` round-trips and repairs most malformed XML.
