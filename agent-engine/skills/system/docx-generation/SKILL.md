---
name: docx-generation
description: Generate polished native DOCX documents with Python when the user needs a real .docx deliverable such as a report, proposal, memo, brief, plan, analysis, comparison, meeting pack, customer-facing document, policy draft, or structured working document. Use this whenever the output should be editable in Word or another office suite, when the user asks for a document artifact rather than plain text, or when the content needs headings, tables, images, and professional formatting in a native document format.
---

# DOCX Generation

Use this skill when the deliverable must be a native `.docx` file created directly in Python.

Use this skill for:

- research reports
- proposals
- memos
- briefs
- plans
- one-pagers
- policy drafts
- customer deliverables
- meeting packs
- implementation documents
- comparison documents
- structured internal working documents

Do not use this skill when the user only needs:

- plain markdown
- plain text notes
- a slide deck
- a fixed-layout artifact where PDF is the better primary format

## Core Libraries

- Prefer `python-docx` for most work.
- Use `docxtpl` when there is an approved Word template that should be filled rather than rebuilt.

## What Good DOCX Work Looks Like

A strong `.docx` file should feel like a real office document, not a text dump in a Word container.

It should have:

- a clear title
- a clean heading hierarchy
- readable paragraphs
- disciplined spacing
- tables where comparison or precision matters
- an ordering that fits the audience and purpose

The result should feel editable, reusable, and ready for handoff.

## Common Scenarios

### Research Report

Use DOCX when the user needs a written report with findings, analysis, and recommendations.

Typical structure:

1. title
2. executive summary
3. scope or context
4. findings
5. implications
6. recommendation
7. appendix

### Proposal

Use DOCX when the output needs to be reviewed, revised, and circulated.

Typical structure:

1. title
2. objective
3. current situation
4. proposed approach
5. timeline
6. pricing or resourcing
7. assumptions
8. next steps

### Memo or Brief

Use DOCX when the document is formal but concise.

Typical structure:

1. title
2. key message
3. context
4. recommendation or decision
5. implications

### Comparison or Matrix Document

Use DOCX when the user needs prose plus tables for things like:

- vendor comparisons
- pricing comparisons
- feature matrices
- rollout options
- risk or ownership tables

### Working Document

Use DOCX when the output is meant to stay editable and be further refined by a team.

In that case, optimize for clarity and structure instead of over-polishing every visual detail.

## Quick Start

Use the inline snippets below as the default starting point. Do not jump to the reference files unless the document or deck is large enough that you need a bigger reusable pattern.

## Standard Workflow

1. Decide whether the output should be generated from scratch or populated from a template.
2. Plan the document structure before writing code.
3. Normalize messy source material into structured content.
4. Generate the document natively in Python.
5. Verify that the document opens cleanly and reads like a real deliverable.

## Plan Before You Code

Before writing the generator, decide:

- who the audience is
- what kind of document this is
- what job the document needs to do
- which sections are required
- whether the document is short and decisive or long and evidentiary
- whether tables are needed
- whether images add value
- whether appendix material should be separated from the main story

Useful internal structure before generation:

- title
- subtitle
- metadata
- sections
- bullet groups
- tables
- image references
- appendix sections

If the source material is messy, normalize it first instead of coding directly against raw prose.

## Native Generation Guidance

When using `python-docx`:

- define the hierarchy first
- use headings for structure instead of random bold text
- use paragraphs for prose
- use tables for comparisons, timelines, metrics, pricing, or ownership
- keep styles consistent
- use page breaks intentionally
- keep filenames professional and explicit

### Snippet: basic document skeleton

```python
from docx import Document


doc = Document()
doc.add_heading("Market Analysis", level=0)
doc.add_heading("Executive Summary", level=1)
doc.add_paragraph("This document summarizes the key findings and recommended actions.")
doc.save("output.docx")
```

Start from the inline snippets in this file first. Use `references/python_docx_report.py` only when you need a larger reusable pattern.

## Additional Inline Snippets

### Snippet: simple comparison table

```python
from docx import Document


doc = Document()
table = doc.add_table(rows=1, cols=3)
headers = table.rows[0].cells
headers[0].text = "Option"
headers[1].text = "Cost"
headers[2].text = "Notes"

for option, cost, notes in [("A", "$10k", "Fastest"), ("B", "$7k", "More setup")]:
    row = table.add_row().cells
    row[0].text = option
    row[1].text = cost
    row[2].text = notes

doc.save("comparison.docx")
```

### Snippet: image insertion

```python
from docx import Document
from docx.shared import Inches


doc = Document()
doc.add_heading("Architecture Overview", level=1)
doc.add_picture("diagram.png", width=Inches(5.5))
doc.save("illustrated.docx")
```

### Snippet: template rendering

```python
from docxtpl import DocxTemplate


doc = DocxTemplate("template.docx")
context = {
    "title": "Quarterly Review",
    "summary": "Revenue increased and churn declined.",
}
doc.render(context)
doc.save("quarterly-review.docx")
```

## Template Guidance

Use `docxtpl` when:

- there is an approved template
- layout consistency matters across repeated runs
- placeholders are known and stable

When using templates:

- preserve the source template
- keep placeholders deterministic
- avoid mixing heavy template logic with messy generation logic unless needed

### Snippet: basic document skeleton

```python
from docxtpl import DocxTemplate


doc = DocxTemplate("template.docx")
doc.render({"title": "Q2 Planning Brief", "owner": "Operations"})
doc.save("output.docx")
```

## Writing Rules For Strong Documents

- use a strong top-level title
- keep section titles short and informative
- use bullets for scanability, not as a replacement for structure
- do not pad the document just to make it longer
- do not collapse complex comparisons into prose when a table is clearer
- match tone to audience
- keep the story easy to follow

## Handling Tables, Images, and Structured Content

### Tables

Use tables when the reader needs precision or comparison.

Good table use cases:

- pricing
- feature comparison
- milestones
- owners and deadlines
- risks
- vendor scoring

Keep them readable:

- short headers
- consistent row structure
- no paragraph walls inside cells

### Images

Use images only when they add real value:

- diagrams
- screenshots
- charts
- branded elements for customer-facing documents

Do not add decorative images with no informational value.

### Long Documents

For long documents:

- front-load the executive summary
- separate findings from recommendations
- move bulky detail into appendices when appropriate
- preserve hierarchy so the document is still scannable

## Scenario-Specific Guidance

### Executive Readout

Prioritize:

- short summary
- clear recommendation
- a small number of strong sections

### Internal Working Draft

Prioritize:

- editability
- explicit assumptions
- operational clarity
- structure that others can build on

### Customer or Client Document

Prioritize:

- polished phrasing
- disciplined formatting
- consistency of terms
- removal of internal-only commentary

### Research Deliverable

Prioritize:

- clear framing
- evidence-backed findings
- structured appendices or tables where necessary

## Failure Modes To Avoid

- dumping raw notes into a DOCX without real structure
- using fake hierarchy made of bold paragraphs instead of headings
- overusing bullets until the document loses narrative flow
- packing too much information into a single table
- producing a file that is valid but not professionally readable
- using DOCX when the real need is PPTX or PDF

## Verification Checklist

Before treating the document as finished, check:

- the document opens successfully
- the title and section order make sense
- headings reflect the intended hierarchy
- tables are readable and useful
- no unintended placeholder text remains
- the tone matches the audience
- the document feels editable and reusable

### Snippet: numbered list section

```python
from docx import Document


doc = Document()
doc.add_heading("Implementation Plan", level=1)
for step in ["Confirm scope", "Prepare rollout plan", "Review with stakeholders"]:
    doc.add_paragraph(step, style="List Number")
doc.save("plan.docx")
```

### Snippet: sectioned report from structured data

```python
from docx import Document


payload = {
    "title": "Quarterly Operating Review",
    "sections": [
        {"heading": "Summary", "body": "Revenue grew and incident volume declined."},
        {"heading": "Risks", "body": "Vendor migration remains the main delivery risk."},
    ],
}

doc = Document()
doc.add_heading(payload["title"], level=0)
for section in payload["sections"]:
    doc.add_heading(section["heading"], level=1)
    doc.add_paragraph(section["body"])
doc.save("operating-review.docx")
```

### Snippet: simple header/footer setup

```python
from docx import Document


doc = Document()
section = doc.sections[0]
section.header.paragraphs[0].text = "Internal Use Only"
section.footer.paragraphs[0].text = "Quarterly Review"
doc.add_heading("Review", level=0)
doc.save("review-with-footer.docx")
```

## Reference Material

The inline snippets in this file should be enough for most runs. Use the reference scripts when you want a closer starting point:

- `references/python_docx_report.py`: fuller report with sections and comparison table
- `references/python_docx_table_report.py`: document centered on a comparison table
- `references/python_docx_memo.py`: short decision memo structure
- `references/python_docx_with_image.py`: document with image insertion
