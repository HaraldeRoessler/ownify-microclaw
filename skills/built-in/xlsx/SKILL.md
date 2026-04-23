---
name: xlsx
description: "Create, read, or edit Excel / CSV / TSV spreadsheets. Use when the deliverable is a spreadsheet (.xlsx, .xlsm, .csv, .tsv), when the user asks to add columns, compute formulas, format cells, clean messy tabular data, or convert between tabular formats. Triggers whenever the user references a spreadsheet by name or says Excel, xlsx, or spreadsheet. Do NOT use when the deliverable is a Word document, a PDF report, or a standalone script — even if the input happens to be tabular."
license: MIT (see repository LICENSE)
---

# xlsx — Spreadsheets

Tools in the runtime:

- `openpyxl` — read/write .xlsx with full control over cells, formulas, styles
- `pandas` — bulk data manipulation, fast read of large sheets, CSV ↔ xlsx
- `soffice` (LibreOffice headless) — convert formats, recalculate formulas

Default to `pandas` for data-shaped tasks. Drop down to `openpyxl` when you need formulas, formatting, merged cells, or multi-sheet workbooks.

## Read

```python
import pandas as pd
df = pd.read_excel("in.xlsx")          # first sheet
sheets = pd.read_excel("in.xlsx", sheet_name=None)  # all as dict
```

For formula authoring, preserve strings — don't evaluate them:

```python
from openpyxl import load_workbook
wb = load_workbook("in.xlsx")           # formulas kept as strings
ws = wb.active
print(ws["B2"].value)                   # e.g. "=SUM(A2:A9)"
```

To read the *calculated* values that Excel/LibreOffice last wrote:

```python
wb = load_workbook("in.xlsx", data_only=True)
```

## Create

```python
from openpyxl import Workbook
from openpyxl.styles import Font, PatternFill

wb = Workbook()
ws = wb.active
ws.title = "Summary"
ws.append(["Region", "Revenue", "Growth"])
for row in [("EMEA", 1200000, 0.08), ("APAC", 900000, 0.14)]:
    ws.append(row)

# formulas, not hard-coded results
ws["B5"] = "=SUM(B2:B3)"
ws["C5"] = "=AVERAGE(C2:C3)"

# number formats
for cell in ws["B2":"B5"][0]:
    cell.number_format = '"€"#,##0'
for cell in ws["C2":"C5"][0]:
    cell.number_format = "0.0%"

# header row
for cell in ws[1]:
    cell.font = Font(bold=True)
    cell.fill = PatternFill("solid", fgColor="DDDDDD")

ws.column_dimensions["A"].width = 12
wb.save("out.xlsx")
```

**Always prefer formulas over hard-coded results**. If the user changes an input, a formula-driven sheet updates; a Python-computed one lies.

## Recalculate formulas

Files written by openpyxl contain formula strings but no cached results, so opening in Excel triggers a recalc on first render. Some consumers (ReportLab tables, some BI tools, email previews) only read the cached values. To force a recalc and write the results back:

```bash
soffice --headless --calc --convert-to xlsx --outdir /tmp out.xlsx
mv /tmp/out.xlsx out.xlsx
```

LibreOffice evaluates every formula during the round-trip and stores the results. A quick post-run check for formula errors:

```python
from openpyxl import load_workbook
wb = load_workbook("out.xlsx", data_only=True)
errors = []
for ws in wb.worksheets:
    for row in ws.iter_rows():
        for cell in row:
            if isinstance(cell.value, str) and cell.value.startswith("#") and cell.value.endswith(("!", "?", "0", "A")):
                errors.append((ws.title, cell.coordinate, cell.value))
print(errors or "no formula errors")
```

## CSV / TSV

```python
import pandas as pd
df = pd.read_csv("in.csv")              # or sep="\t" for tsv
df.to_excel("out.xlsx", index=False)
```

For messy inputs (misplaced headers, junk rows), inspect first:

```python
head = pd.read_csv("in.csv", header=None, nrows=20)
print(head)
# then pass header=<real header row> and skiprows= as needed
```

## Edit in place

```python
from openpyxl import load_workbook
wb = load_workbook("in.xlsx")
ws = wb["Sheet1"]
ws.insert_rows(2)                        # blank row at top
ws["A2"] = "TOTAL"
ws["B2"] = "=SUM(B3:B999)"
wb.save("out.xlsx")
```

Never load with `data_only=True` and save — that strips formulas permanently.

## Rules of thumb

- Do calculations with formulas, not Python. Hard-coded values don't react to input changes.
- Column letters: `openpyxl.utils.get_column_letter(n)` / `column_index_from_string(s)` — don't hand-compute.
- For financial models, follow the usual color code (blue = input, black = formula) only if the user hasn't given you a template; existing templates win.
- Zero currency: format as `"€"#,##0;("€"#,##0);"-"` so zeros render as a dash.
- Big files: use `read_only=True` / `write_only=True` with openpyxl to keep memory bounded.
