# Memory discipline

You have `save_memory` / `load_memory`. Memory is for durable facts and procedures that matter beyond the current task — not for progress, not for notes.

## Two categories

Pick one when saving:

- **`semantic`** — a fact. What is true. Invariants, definitions, constraints, decisions with reason.
- **`procedural`** — a how-to. A validated sequence of steps worth reusing.

No other category.

## Three scopes

Pick one when saving. Scope controls who can recall the memory.

- **`actor`** — visible to every task this user/actor ever runs. Use for user preferences, personal style rules.
- **`project`** — visible within one project. Use for project conventions, decisions, constraints.
- **`thread`** — visible within one task lane. Use when a fact outlives compaction but doesn't matter outside this task.

Default to `project`. `actor` is rare (real personal preferences). `thread` is short-lived but more durable than conversation.

## When to save

Three triggers:

1. **Surprise** — reality differed from your model. Future-you would make the same mistake without this note. → `semantic`.
2. **Decision with reason** — a user correction to honor, or a validated "we chose X because Y". → `semantic`.
3. **Validated procedure** — a multi-step sequence that worked, non-obvious, worth reusing. → `procedural`.

Do NOT save:
- Anything re-readable from code, docs, or `git log`.
- Ephemeral progress ("working on X") — journal's job.
- Observations that might be true — verify first, save if confirmed.
- Restatements of the task brief or acceptance criteria.

## When to load

- At task start, before making non-trivial decisions: `load_memory` with **specific** query terms tied to the task.
- Before any decision where a prior ruling might apply.

Specific query wins. `load_memory("oauth refresh token rotation")` beats `load_memory("auth")`.

## Entry format

Three parts, in order:

1. **The fact or procedure** — one line, independently readable outside this thread.
2. **`Why:`** — the reason. Prior incident, explicit user statement, or validated outcome.
3. **`How to apply:`** — the trigger that should make you recall it.

- Good (semantic, project):
  ```
  OAuth refresh tokens must be rotated on every use; reuse is treated as theft.
  Why: legal flagged reuse as non-compliant with spec 2025-11.
  How to apply: any code path that stores or re-reads refresh_token.
  ```
- Good (procedural, project):
  ```
  To apply a schema migration safely: run `cargo sqlx prepare` locally, commit the cache, then deploy.
  Why: skipping the prepare step ships stale query metadata and breaks the worker.
  How to apply: any PR that changes SQL in commands/ or queries/.
  ```
- Bad: "Don't reuse OAuth tokens." (no why, no trigger)

Entries must make sense in a week, read by a different execution. No references to "this thread", "the current task", "we just discussed".

## Category + scope quick table

| Saving… | Category | Scope |
|---------|----------|-------|
| A surprise about the system or project | `semantic` | `project` |
| A decision made in this project with reason | `semantic` | `project` |
| A hidden constraint (spec, legal, stakeholder) | `semantic` | `project` |
| A personal preference of the user | `semantic` | `actor` |
| "How we do X in this project" (validated) | `procedural` | `project` |
| A validated recipe that outlives this task but not this project | `procedural` | `project` |
| A fact that matters across tasks on this same lane | `semantic` | `thread` |

## Building the chain of thought

A distilled rule is not enough on its own. When you save a non-trivial memory, also capture the scenario around it — so a future execution can reconstruct *why* the rule applies, not just *that* it does.

`save_memory` accepts three additional optional fields beyond `content`:

- **`observation`** — the narrative that led to the insight. The "scenario". One paragraph: what you were doing, what you saw, what surprised you, what confirmed the rule.
- **`signals`** — short cue phrases (3–6 words each). What would tell a future execution "this memory applies to what I'm doing right now"?
- **`related`** — memory IDs of neighbors in the reasoning chain. When this memory fires, these are worth considering too.

Populate `observation` + `signals` for anything non-trivial. A one-line rule without a scenario is fine for common facts; anything born of debugging, incident, or negotiation deserves the full shape.

- Good (full shape):
  ```
  content:
    OAuth refresh tokens must be rotated on every use.
    Why: legal flagged reuse as non-compliant with spec 2025-11.
    How to apply: any code path that stores or re-reads refresh_token.

  observation:
    During the 2025-11 audit, the token refresh endpoint returned the same
    refresh_token after rotation. Legal reviewed under the new spec and
    classified reuse as theft-equivalent. Fix was generating a new
    refresh_token every rotation; verified via compliance e2e test.

  signals: ["oauth audit", "token rotation flow", "refresh token reuse"]
  related: ["<id of session storage memory>", "<id of legal spec memory>"]
  ```

## Loading — follow the chain until clues saturate

A single `load_memory` hit is often not enough. Teach yourself to follow the chain:

1. **First query** — `load_memory` with specific terms tied to the current task.
2. For each hit: read `content` + `signals` — do the signals match your situation?
3. **If signals match** — read `observation`. This is where assumptions die: the narrative tells you *when* the rule applies and *when it doesn't*.
4. **If `related` is non-empty and the current decision is non-trivial** — load those memories too. Follow the chain one hop at a time.
5. **Stop when clues saturate** — new loads return the same memories, or add no new context. State explicitly in a `note` that you're stopping and why ("signals across three memories all point to the same constraint; no further loading needed").

The goal is: before you act on a recalled rule, you should know the *scenario* that produced it well enough not to misapply it. No assumption is acceptable when the observation was there to be read.

- Good: `load_memory("oauth refresh token") → 1 hit, signals match, read observation, follow 2 related IDs, saturated after 4 memories. Proceeding with full context.`
- Bad: `load_memory("auth") → got something, applying blindly.`

## If the user says "remember this" or "forget that"

Save or remove immediately. Do not batch. Do not ask — use the table.
