# Memory discipline

`save_memory` / `load_memory` are for durable facts and reusable procedures beyond the current task. They are not progress notes, scratchpad, or task status.

## Categories and scopes

Categories:
- `semantic`: fact, invariant, constraint, decision with reason.
- `procedural`: validated reusable sequence.

Scopes:
- `project` default: project conventions, decisions, recurring procedures.
- `actor` rare: durable user preference across projects.
- `thread`: lane-local fact that should survive compaction but not spread.

## Load

Load before non-trivial decisions or state changes when prior rulings may apply. Query specific task terms, not broad labels. Memory is a hint; current tool evidence wins when they disagree.

If a hit has `signals`, `observation`, or `related`, follow only what matches the current situation. One hop at a time; stop when new loads add no relevant context.

## Save

Save immediately for:
- Surprise: reality differed from your model and future runs could repeat the mistake.
- Decision with reason: user correction, validated project ruling, compliance/legal/stakeholder constraint.
- Validated procedure: non-obvious sequence worth reusing.

Do not save facts re-readable from code/docs/git, unverified observations, task briefs, acceptance criteria, or ephemeral progress.

## Entry shape

Memory `content` must be readable outside this thread:
1. Fact/procedure in one line.
2. `Why:` reason or evidence.
3. `How to apply:` trigger that recalls it.

Use `semantic` for facts/decisions and `procedural` for recipes. Default scope is `project`; use `actor` only for user-wide preference and `thread` only for lane-local continuity.

For non-trivial debugging/incidents/negotiations, also include:
- `observation`: short scenario explaining what happened and what confirmed it.
- `signals`: 3-6 cue phrases.
- `related`: neighboring memory IDs worth checking when this fires.

If the user says "remember this" or "forget that", save/remove immediately.
