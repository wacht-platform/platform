---
name: pdf-generation
description: Generate polished native PDF artifacts with Python when the user needs a real .pdf deliverable such as a printable brief, final report, fixed-layout summary, handout, proposal export, one-pager, or presentation-quality document that should render consistently across machines. Use this whenever fixed layout matters more than editability, when the document is meant for distribution or printing, or when the output should feel final rather than like a working draft.
---

# PDF Generation

Use this skill when the deliverable must be a native `.pdf` generated directly in Python.

Use this skill for:

- printable briefs
- one-page summaries
- fixed-layout reports
- formal handouts
- final shareable exports
- leave-behinds
- customer-facing summaries
- tightly formatted structured documents
- presentation-quality written artifacts

Do not use this skill when the user primarily needs:

- an editable office document
- a slide deck
- plain markdown or notes
- HTML conversion as the main strategy

## Core Library

- Use `reportlab`.

## What Good PDF Work Looks Like

A strong PDF should feel final, stable, and deliberate.

It should have:

- clean page structure
- consistent typography
- disciplined spacing
- predictable pagination
- readable sections and tables
- output that renders reliably elsewhere

PDF is the right choice when layout stability matters more than editability.

## Common Scenarios

### One-Pager

Use PDF for concise summaries that should be easy to share or print.

Typical structure:

1. title
2. short summary
3. key points or metrics
4. recommendation or call to action

### Fixed-Layout Brief

Use PDF when the document should feel like a final presentation artifact rather than a working document.

Typical structure:

1. title
2. context
3. findings
4. recommendation
5. appendix or notes when needed

### Formal Report Export

Use PDF when the user wants a report distributed in a stable, non-editable format.

Typical structure:

1. title
2. executive summary
3. detailed sections
4. supporting tables
5. appendix

### Customer-Facing Handout

Use PDF when the document should be easy to open, print, and circulate without layout drift.

### Presentation-Style Summary

Use PDF when the content is visual or summary-driven but does not need to remain editable as slides.

## When To Choose PDF Instead Of DOCX Or PPTX

Choose PDF when:

- consistent rendering matters
- the artifact is intended for distribution
- printability matters
- the document should feel final
- editability is not the priority

Choose DOCX instead when:

- the recipient is expected to revise the content
- the output is primarily a working document

Choose PPTX instead when:

- the content is fundamentally slide-based
- the artifact is meant to be presented slide by slide

## Quick Start

Use the inline snippets below as the default starting point. Do not jump to the reference files unless the document or deck is large enough that you need a bigger reusable pattern.

## Standard Workflow

1. Decide whether the PDF is a one-pager, brief, report, handout, or fixed-layout summary.
2. Plan the page structure before writing code.
3. Convert messy source material into structured content.
4. Generate the PDF natively in Python.
5. Verify readability, pagination, and overall finish.

## Plan Before You Code

Before writing the generator, decide:

- target audience
- intended page count or density
- whether the document is prose-heavy or table-heavy
- whether a title block or cover block is needed
- whether repeated headers or footers help
- what must appear early in the document
- whether the document is optimized for screen reading, print reading, or both

Useful internal structure before generation:

- title
- subtitle
- metadata
- sections
- bullets
- tables
- callout blocks
- footer notes

If the input material is large or messy, normalize it before coding.

## Native Generation Guidance

When using `reportlab`:

- plan the layout before drawing content
- keep spacing and typography consistent
- make the first page carry the main purpose clearly
- paginate intentionally
- keep tables readable at page scale

### Snippet: basic canvas PDF

```python
from reportlab.lib.pagesizes import LETTER
from reportlab.pdfgen import canvas


c = canvas.Canvas("output.pdf", pagesize=LETTER)
c.setFont("Helvetica-Bold", 16)
c.drawString(72, 720, "Executive Brief")
c.setFont("Helvetica", 11)
c.drawString(72, 700, "This PDF summarizes the current state and recommended action.")
c.save()
```

Start from the inline snippets in this file first. Use `references/reportlab_brief.py` only when you need a larger reusable pattern.

## Additional Inline Snippets

### Snippet: multi-line text block

```python
from reportlab.lib.pagesizes import LETTER
from reportlab.pdfgen import canvas


c = canvas.Canvas("brief.pdf", pagesize=LETTER)
text = c.beginText(72, 720)
text.setFont("Helvetica", 11)
for line in [
    "Executive Summary",
    "- Revenue increased 12% quarter over quarter.",
    "- Support backlog declined after workflow changes.",
]:
    text.textLine(line)
c.drawText(text)
c.save()
```

### Snippet: table-like canvas layout

```python
from reportlab.lib.pagesizes import LETTER
from reportlab.pdfgen import canvas


c = canvas.Canvas("table.pdf", pagesize=LETTER)
rows = [
    ["Option", "Cost", "Notes"],
    ["A", "$10k", "Fast launch"],
    ["B", "$7k", "More setup"],
]
y = 720
for row in rows:
    x = 72
    for cell in row:
        c.rect(x, y - 18, 140, 22)
        c.drawString(x + 6, y - 4, cell)
        x += 140
    y -= 22
c.save()
```

### Snippet: Platypus flowable layout

```python
from reportlab.lib.styles import getSampleStyleSheet
from reportlab.platypus import SimpleDocTemplate, Paragraph, Spacer


styles = getSampleStyleSheet()
doc = SimpleDocTemplate("report.pdf")
story = [
    Paragraph("Quarterly Review", styles["Title"]),
    Spacer(1, 12),
    Paragraph("This PDF summarizes progress, risks, and next actions.", styles["BodyText"]),
]
doc.build(story)
```

## Layout Rules

- keep margins readable
- keep line length under control
- use headings consistently
- use spacing to separate ideas
- paginate intentionally
- use tables only when they remain readable
- keep the layout disciplined rather than decorative

## Scenario-Specific Guidance

### Printable Executive Brief

Prioritize:

- first-page clarity
- concise summary
- strong recommendation
- stable layout

### Customer-Facing PDF

Prioritize:

- strong first impression
- precise wording
- careful formatting
- minimal internal jargon

### Internal Summary PDF

Prioritize:

- speed of comprehension
- section clarity
- useful tables
- clear conclusions

### Multi-Page Report PDF

Prioritize:

- section hierarchy
- pagination discipline
- front-loaded summary
- structured appendix treatment

## Handling Tables, Callouts, and Visual Structure

### Tables

Use tables for:

- comparisons
- timelines
- metrics
- pricing
- ownership and risks

Keep them page-aware:

- short headers
- readable widths
- no giant wall-of-text cells

### Callouts

Use callouts or emphasis blocks for:

- key takeaways
- decisions
- warnings
- recommendations

Do not overuse them or the document loses hierarchy.

### Repeated Elements

Headers, footers, and page numbers are useful when the document is multi-page or formal. Use them when they improve orientation.

## Professional Quality Rules

- do not cram too much information onto a page
- keep typography and spacing consistent
- make the first page count
- use PDF because the layout choice matters
- keep sections readable in isolation
- prefer clarity over ornament

## Failure Modes To Avoid

- treating PDF as a screenshot container
- forcing an editable working document into PDF too early
- squeezing too much content onto one page
- creating tables that technically fit but are unreadable
- using inconsistent headings or spacing
- producing a final artifact that still feels unfinished

## Verification Checklist

Before treating the PDF as finished, check:

- the file opens successfully
- the first page communicates the purpose clearly
- pagination feels deliberate
- major sections are easy to scan
- tables fit and remain readable
- typography and spacing are consistent
- the document feels like a final artifact, not a draft export

### Snippet: multi-page canvas PDF

```python
from reportlab.lib.pagesizes import LETTER
from reportlab.pdfgen import canvas


c = canvas.Canvas("multi-page.pdf", pagesize=LETTER)
c.drawString(72, 720, "Page 1 summary")
c.showPage()
c.drawString(72, 720, "Page 2 details")
c.save()
```

### Snippet: styled heading and body with Platypus

```python
from reportlab.lib.styles import getSampleStyleSheet
from reportlab.platypus import SimpleDocTemplate, Paragraph, Spacer


styles = getSampleStyleSheet()
doc = SimpleDocTemplate("styled-brief.pdf")
story = [
    Paragraph("Strategy Brief", styles["Title"]),
    Spacer(1, 12),
    Paragraph("This document summarizes the decision context and recommended path.", styles["BodyText"]),
]
doc.build(story)
```

### Snippet: simple table with Platypus

```python
from reportlab.platypus import SimpleDocTemplate, Table


data = [
    ["Option", "Cost", "Notes"],
    ["A", "$10k", "Fast launch"],
    ["B", "$7k", "More setup"],
]
doc = SimpleDocTemplate("table-flowable.pdf")
doc.build([Table(data)])
```

### Snippet: callout block with canvas

```python
from reportlab.lib.pagesizes import LETTER
from reportlab.lib.colors import lightgrey
from reportlab.pdfgen import canvas


c = canvas.Canvas("callout.pdf", pagesize=LETTER)
c.setFillColor(lightgrey)
c.rect(60, 640, 480, 70, fill=1, stroke=0)
c.setFillColorRGB(0, 0, 0)
c.drawString(72, 680, "Recommendation")
c.drawString(72, 660, "Proceed with phased rollout and monthly review.")
c.save()
```

## Reference Material

The inline snippets in this file should be enough for most runs. Use the reference scripts when you want a closer starting point:

- `references/reportlab_brief.py`: fuller structured brief with styled table
- `references/reportlab_one_pager.py`: concise one-page summary
- `references/reportlab_table_summary.py`: table-led PDF summary
- `references/reportlab_multi_page_report.py`: multi-page report structure
