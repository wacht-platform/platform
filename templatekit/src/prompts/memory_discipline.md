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

[categories.fact]
covers = ["specific factual statement about user, project, or world"]
shape = "short, specific, standalone — e.g. \"Project's CI pipeline runs on GitHub Actions\""

[categories.preference]
covers = ["user preference, setting, or recurring choice"]
shape = "what the user consistently wants — e.g. \"User prefers verbose output in shell commands\" or \"User's preferred timezone is UTC+2\""

[categories.observation]
covers = ["event, outcome, or significant detail"]
shape = "what happened, the outcome, and why it matters — e.g. \"Build failed because X; rerun with Y flag\""

[categories.conversation_summary]
covers = ["condensed recap of a conversation or task run"]
shape = "what was discussed, decided, or produced — kept dense and minimal"

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

[load.recency]
note = "the runtime applies a time-decay to every retrieval result — fresh memories surface above equally-similar old ones. At ~0.0005/hour, a memory ages ~0.36 per month of staleness. You do NOT need to compute or pass any decay yourself; it's automatic."
effect_on_agent = "you will see fresher results first whenever semantic scores are close, so prefer the top-ranked hits and don't second-guess their ordering"
implication_for_save = "when you save facts that supersede older memories (e.g. a preference flip), save the new version and trust the retrieval ranking to prefer it; you do NOT need to delete or 'supersede' the old one — recency does it implicitly"

[load.followups]
follow_when = "hit has `signals`, `observation`, or `related` AND they match the current situation"
follow_rate = "one hop at a time"
stop_when = "new loads add no relevant context"

[save.triggers]
required_when_any = [
  "surprise: reality differed from your model and future runs could repeat the mistake",
  "decision with reason: user correction, validated project ruling, compliance/legal/stakeholder constraint",
  "validated procedure: non-obvious sequence worth reusing",
  "established user preference that affects behaviour",
  "recurring observation or failure pattern",
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
category_picker = "semantic (general fact/decision), procedural (how-to), fact (specific fact), preference (user setting), observation (event/outcome), conversation_summary (condensed recap)"
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
