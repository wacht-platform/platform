# Memory discipline

`save_memory` / `load_memory`. Memory = durable facts and procedures beyond current task. Not progress. Not notes.

## Two categories

- `semantic` — fact. Invariants, definitions, constraints, decisions with reason.
- `procedural` — how-to. Validated reusable sequence.

No other category.

## Three scopes

- `actor` — visible to every task this user runs. User preferences, personal style.
- `project` — visible within one project. Project conventions, decisions, constraints.
- `thread` — visible within one task lane. Outlives compaction but not other tasks.

Default `project`. `actor` rare. `thread` short-lived but more durable than conversation.

## When to save

Three triggers:

1. **Surprise** — reality differed from model. Future-you would repeat the mistake. → `semantic`.
2. **Decision with reason** — user correction or validated "we chose X because Y". → `semantic`.
3. **Validated procedure** — non-obvious multi-step sequence worth reuse. → `procedural`.

Do NOT save:
- Re-readable from code, docs, `git log`.
- Ephemeral progress ("working on X") — journal's job.
- Observations not yet verified.
- Restatements of task brief or acceptance criteria.

## When to load

- Task start, before non-trivial decisions. Specific terms tied to task.
- Before any decision where prior ruling might apply.

Specific query wins. "oauth refresh token rotation" beats "auth".

## Entry format

This format is for the **content you pass to `save_memory`**, not for turn output. Never emit `Why:` / `How to apply:` as labels in your conversational reply — they belong inside saved memory entries only.

Three parts in order:

1. Fact or procedure — one line, readable outside this thread.
2. `Why:` — reason. Prior incident, user statement, validated outcome.
3. `How to apply:` — trigger that recalls it.

Good (semantic, project):
```
OAuth refresh tokens must rotate every use; reuse is theft.
Why: legal flagged reuse non-compliant with spec 2025-11.
How to apply: any code path storing or re-reading refresh_token.
```
Good (procedural, project):
```
Apply schema migration safely: run `cargo sqlx prepare`, commit cache, deploy.
Why: skipping prepare ships stale query metadata, breaks worker.
How to apply: any PR changing SQL in commands/ or queries/.
```
Bad: "Don't reuse OAuth tokens." (no why, no trigger)

Entries readable in a week by different execution. No "this thread", "current task", "we just discussed".

## Category + scope quick table

| Saving… | Category | Scope |
|---|---|---|
| Surprise about system or project | `semantic` | `project` |
| Decision in this project with reason | `semantic` | `project` |
| Hidden constraint (spec, legal, stakeholder) | `semantic` | `project` |
| User personal preference | `semantic` | `actor` |
| "How we do X in this project" (validated) | `procedural` | `project` |
| Validated recipe outliving task but not project | `procedural` | `project` |
| Fact across tasks on same lane | `semantic` | `thread` |

## Chain of thought

Distilled rule alone is not enough. Capture scenario around it. Future execution must reconstruct *why* the rule applies, not just *that* it does.

`save_memory` accepts three optional fields beyond `content`:

- `observation` — narrative leading to insight. One paragraph: what doing, what saw, what surprised, what confirmed.
- `signals` — short cue phrases (3-6 words). Tells future execution "this memory applies now".
- `related` — memory IDs of neighbors in reasoning chain. When this memory fires, these worth considering.

Populate `observation` + `signals` for non-trivial. One-line rule fine for common facts. Debugging, incident, negotiation: full shape required.

Good (full shape):
```
content:
  OAuth refresh tokens must rotate every use.
  Why: legal flagged reuse non-compliant with spec 2025-11.
  How to apply: any code path storing or re-reading refresh_token.

observation:
  2025-11 audit, refresh endpoint returned same refresh_token after
  rotation. Legal classified reuse theft-equivalent. Fix: generate new
  refresh_token every rotation; verified via compliance e2e test.

signals: ["oauth audit", "token rotation flow", "refresh token reuse"]
related: ["<id of session storage memory>", "<id of legal spec memory>"]
```

## Loading — follow chain until saturate

Single hit often not enough. Follow chain:

1. First query: specific terms tied to task.
2. Each hit: read `content` + `signals`. Signals match situation?
3. Signals match → read `observation`. Tells when rule applies AND when not.
4. `related` non-empty AND decision non-trivial → load those too. One hop at a time.
5. Stop saturated. New loads return same memories or add no context. State explicitly in note.

Goal: before acting on recalled rule, know the scenario well enough not to misapply.

## User says "remember this" or "forget that"

Save or remove immediately. No batch. No ask. Use the table.
