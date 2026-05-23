# memory_discipline
# Spec for `save_memory` / `load_memory`. Each [section] is a rule or catalog;
# keys describe its facets.

[purpose]
tools = ["save_memory", "load_memory"]
intended_for = ["durable facts", "reusable procedures beyond the current task"]
not_for = ["progress notes", "scratchpad", "task status"]

[categories.semantic]
covers = ["fact", "invariant", "constraint", "decision with reason"]
shape = "what is true, OR what was decided and why"

[categories.procedural]
covers = ["validated reusable sequence"]
shape = "ordered steps proven to work"

[scopes.project]
default = true
covers = ["project conventions", "decisions", "recurring procedures"]

[scopes.actor]
default = false
rarity = "rare"
covers = ["durable user preference across projects"]
restriction = "user-wide only; do not use for project-specific or task-specific content"

[scopes.thread]
default = false
covers = ["lane-local fact that should survive compaction but not spread"]
restriction = "use only for lane-local continuity"

[load]
trigger = "before non-trivial decisions or state changes when prior rulings may apply"
query_shape = "specific task terms"
forbidden_queries = "broad labels"

[load.conflict_resolution]
memory_role = "hint"
truth_source = "current tool evidence"
on_conflict = "current tool evidence wins"

[load.followups]
follow_when = "hit has `signals`, `observation`, or `related` AND they match the current situation"
follow_rate = "one hop at a time"
stop_when = "new loads add no relevant context"

[save.triggers]
required_when_any = [
  "surprise: reality differed from your model and future runs could repeat the mistake",
  "decision with reason: user correction, validated project ruling, compliance/legal/stakeholder constraint",
  "validated procedure: non-obvious sequence worth reusing",
]
timing = "immediately upon trigger"

[save.forbidden]
do_not_save = [
  "facts re-readable from code, docs, or git",
  "unverified observations",
  "task briefs",
  "acceptance criteria",
  "ephemeral progress",
]

[entry.shape]
constraint = "memory content must be readable outside this thread"
template = """
<fact or procedure in one line>
Why: <reason or evidence>
How to apply: <trigger that recalls it>
"""
category_picker = "semantic for facts/decisions; procedural for recipes"
scope_picker = "project (default); actor (user-wide preference only); thread (lane-local continuity only)"

[entry.enrichment]
required_when = "non-trivial debugging, incidents, or negotiations"

[entry.enrichment.fields]
observation = "short scenario explaining what happened and what confirmed it"
signals = "3-6 cue phrases"
related = "neighboring memory IDs worth checking when this fires"

[user_commands]
on_remember = "save immediately"
on_forget = "remove immediately"
phrasing_match = "user says 'remember this' / 'forget that' or equivalent"
