---
name: pptx-generation
description: Generate professional native PowerPoint decks with Python when the user needs a real .pptx presentation such as an executive briefing, strategy deck, project update, proposal deck, investor-style summary, training deck, sales presentation, product overview, or structured slide-based artifact. Use this whenever the output should be a true presentation rather than plain text or a document, especially when the content must be organized slide by slide, visually scannable, and ready for presentation or distribution.
---

# PPTX Generation

Use this skill when the deliverable must be a native `.pptx` deck created directly in Python.

Use this skill for:

- executive briefings
- strategy decks
- project updates
- sales decks
- proposal decks
- board-style updates
- investor-style summaries
- training decks
- product overviews
- roadmap presentations
- market landscape briefings
- decision decks

Do not use this skill when the user only needs:

- a text document
- a markdown artifact
- a printable fixed-layout document where PDF is the better format
- informal notes with no presentation requirement

## Core Library

- Use `python-pptx`.

## What Good PPTX Work Looks Like

A strong deck is not a report split into slides. It should have:

- a clear narrative
- one main point per slide
- short, useful titles
- readable bullets or tables
- disciplined slide sequencing
- a clear audience-aware takeaway

The output should feel presentable and intentionally structured.

## Common Scenarios

### Executive Briefing

Use PPTX when the audience needs a concise decision-oriented summary.

Typical structure:

1. title
2. executive summary
3. current situation
4. key findings
5. recommendation
6. next steps

### Project Update Deck

Use PPTX when progress, risks, and next actions need to be communicated slide by slide.

Typical structure:

1. title
2. status snapshot
3. completed work
4. current focus
5. blockers and risks
6. milestones
7. next actions

### Sales or Proposal Deck

Use PPTX when the content needs persuasion, sequencing, and pacing.

Typical structure:

1. title
2. problem or opportunity
3. context
4. proposed solution
5. proof points
6. commercial shape
7. next steps

### Research or Market Deck

Use PPTX when findings should be communicated visually rather than as a long written report.

Typical structure:

1. title
2. scope and method
3. market landscape
4. key findings
5. comparisons
6. implications
7. recommendation

### Training or Enablement Deck

Use PPTX when the goal is to explain, teach, or guide a process.

Typical structure:

1. title
2. learning goals
3. core concepts
4. examples
5. process or walkthrough
6. recap

## Quick Start

Use the inline snippets below as the default starting point. Do not jump to the reference files unless the document or deck is large enough that you need a bigger reusable pattern.

## Standard Workflow

1. Decide the audience and purpose of the deck.
2. Build the slide narrative before writing code.
3. Convert source material into a slide-by-slide structure.
4. Generate the deck natively in Python.
5. Verify that each slide has a distinct role and the full deck reads cleanly.

## Plan Before You Code

Before writing the generator, decide:

- who will present or read the deck
- whether the deck is persuasive, informational, operational, or instructional
- how many sections it needs
- what the title slide should establish
- which slides are summary slides
- which slides need bullets, tables, visuals, or diagrams
- what the final takeaway should be

Useful internal structure before generation:

- deck title
- subtitle
- audience
- sections
- slides
- slide type
- title
- bullets
- table data
- image references
- emphasis notes

If the source material is long, summarize it into slide-ready structure before coding.

## Slide Design Rules

- keep one main message per slide
- use short, direct titles
- keep bullets shallow and scannable
- avoid paragraph walls
- use section divider slides for larger decks
- use whitespace intentionally
- keep narrative flow stronger than slide-level cleverness

## Slide Types To Use Deliberately

### Title Slide

Use it to establish:

- subject
- audience
- scope or time period

### Executive Summary Slide

Use it to give the answer early. Senior audiences should not wait until the end to learn the recommendation.

### Section Divider Slide

Use it to create transitions in longer decks.

### Bullet Slide

Use it for:

- takeaways
- argument structure
- concise summaries

### Table Slide

Use it for:

- comparisons
- pricing
- milestone snapshots
- metrics summaries

### Visual or Diagram Slide

Use it when a process, architecture, or comparison is clearer visually than as bullets.

### Recommendation Slide

End important decks with a concrete recommendation, decision, or next-step frame.

## Native Generation Guidance

When using `python-pptx`:

- define slide purpose before slide content
- keep layout patterns consistent
- preserve enough whitespace to make slides readable
- prefer a smaller number of strong slides over a larger number of weak ones

### Snippet: basic bullet slide

```python
from pptx import Presentation


prs = Presentation()
slide = prs.slides.add_slide(prs.slide_layouts[1])
slide.shapes.title.text = "Executive Summary"
slide.placeholders[1].text = "- Key finding\n- Recommendation"
prs.save("output.pptx")
```

Start from the inline snippets in this file first. Use `references/python_pptx_briefing.py` only when you need a larger reusable pattern.

## Additional Inline Snippets

### Snippet: bullet slide with multiple points

```python
from pptx import Presentation


prs = Presentation()
slide = prs.slides.add_slide(prs.slide_layouts[1])
slide.shapes.title.text = "Key Findings"
body = slide.placeholders[1].text_frame
body.text = "Finding 1"
for item in ["Finding 2", "Finding 3"]:
    p = body.add_paragraph()
    p.text = item
prs.save("findings.pptx")
```

### Snippet: comparison table slide

```python
from pptx import Presentation
from pptx.util import Inches


prs = Presentation()
slide = prs.slides.add_slide(prs.slide_layouts[5])
slide.shapes.title.text = "Option Comparison"

table = slide.shapes.add_table(3, 3, Inches(0.7), Inches(1.5), Inches(8.0), Inches(1.8)).table
table.cell(0, 0).text = "Option"
table.cell(0, 1).text = "Cost"
table.cell(0, 2).text = "Notes"
table.cell(1, 0).text = "A"
table.cell(1, 1).text = "$10k"
table.cell(1, 2).text = "Fast launch"
table.cell(2, 0).text = "B"
table.cell(2, 1).text = "$7k"
table.cell(2, 2).text = "More setup"
prs.save("comparison.pptx")
```

### Snippet: image slide

```python
from pptx import Presentation
from pptx.util import Inches


prs = Presentation()
slide = prs.slides.add_slide(prs.slide_layouts[5])
slide.shapes.title.text = "System Diagram"
slide.shapes.add_picture("diagram.png", Inches(1.0), Inches(1.5), width=Inches(8.0))
prs.save("diagram-deck.pptx")
```

## Templates and Branding

If a branded template exists, preserve its structure and theme.

Use templates when:

- brand consistency matters
- repeated output should look similar
- master slide choices are already solved

If there is no template, create a clean deck rather than trying to fake complex brand systems with messy code.

## Handling Tables, Images, and Visual Evidence

### Tables

Use tables when precision matters. Keep them readable at slide scale:

- short headers
- limited rows per slide
- no overloaded cells

### Images

Use images when they support the message:

- screenshots
- diagrams
- charts
- product visuals

Do not place visuals on slides just to fill space.

### Chart-Like Content

If real chart generation is not available, prefer clear tables or comparison slides over fake charts that look unprofessional.

## Scenario-Specific Guidance

### Senior Leadership Deck

Prioritize:

- short titles
- strong summaries
- evidence only where it supports decisions
- a clear recommendation

### Working Session Deck

Prioritize:

- tradeoffs
- options
- open questions
- action framing

### Customer-Facing Deck

Prioritize:

- polished phrasing
- careful claims
- reduced internal jargon
- clear flow

### Product or Roadmap Deck

Prioritize:

- sequencing
- milestones
- dependencies
- tradeoffs
- priorities

## Failure Modes To Avoid

- turning a written report into text-heavy slides
- using titles that do not communicate the point
- overloading a single slide with too much content
- creating a deck with no narrative arc
- treating PPTX as just another document format
- filling slides with raw notes instead of presentation-ready language

## Verification Checklist

Before treating the deck as finished, check:

- the deck opens successfully
- the slide sequence makes narrative sense
- each slide has a clear role
- titles communicate the slide point
- no slide is overloaded with text
- tables remain readable
- visuals support the message
- the deck ends with a clear takeaway

### Snippet: title slide

```python
from pptx import Presentation


prs = Presentation()
slide = prs.slides.add_slide(prs.slide_layouts[0])
slide.shapes.title.text = "Q3 Strategy Review"
slide.placeholders[1].text = "Prepared for leadership"
prs.save("title-slide.pptx")
```

### Snippet: agenda slide

```python
from pptx import Presentation


prs = Presentation()
slide = prs.slides.add_slide(prs.slide_layouts[1])
slide.shapes.title.text = "Agenda"
text_frame = slide.placeholders[1].text_frame
text_frame.text = "Market context"
for item in ["Key findings", "Options", "Recommendation"]:
    p = text_frame.add_paragraph()
    p.text = item
prs.save("agenda.pptx")
```

### Snippet: section divider slide

```python
from pptx import Presentation
from pptx.util import Inches


prs = Presentation()
slide = prs.slides.add_slide(prs.slide_layouts[6])
textbox = slide.shapes.add_textbox(Inches(1.0), Inches(2.2), Inches(8.0), Inches(1.0))
textbox.text_frame.text = "Competitive Landscape"
prs.save("section-divider.pptx")
```

### Snippet: image plus caption slide

```python
from pptx import Presentation
from pptx.util import Inches


prs = Presentation()
slide = prs.slides.add_slide(prs.slide_layouts[5])
slide.shapes.title.text = "System Overview"
slide.shapes.add_picture("diagram.png", Inches(1.0), Inches(1.5), width=Inches(7.5))
caption = slide.shapes.add_textbox(Inches(1.0), Inches(6.3), Inches(7.5), Inches(0.5))
caption.text_frame.text = "Current architecture and data flow"
prs.save("image-caption.pptx")
```

## Reference Material

The inline snippets in this file should be enough for most runs. Use the reference scripts when you want a closer starting point:

- `references/python_pptx_briefing.py`: fuller executive briefing deck
- `references/python_pptx_status_deck.py`: project or program status deck
- `references/python_pptx_comparison_table.py`: option or vendor comparison deck
- `references/python_pptx_image_showcase.py`: image-led slide deck starter
