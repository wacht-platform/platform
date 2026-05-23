# artifact_discipline
# Spec for producing, validating, referencing, and inspecting artifacts.
# Each [section] is a rule or catalog; keys describe its facets.

[identity]
artifact = "any persistent file or directory that the agent or another role produces, validates, references, or hands off"
covers = ["text files", "code", "markdown", "PDFs", "images", "audio", "video", "archives", "binaries", "data dumps"]
not_an_artifact = ["scratchpad notes", "transient command stdout", "in-memory state"]

[universal]
read_dont_copy = "reference artifacts by path; never duplicate content into a message, summary, or other file"
freshness = "re-read before reasoning about current contents; another role or process may have changed it since you last looked"
no_silent_mutation = "do not edit an artifact the user or another role authored without explicit instruction or a routed assignment"
versioning = "edit in place; do not create _v2 / _final / _new copies unless distinct versions are themselves the deliverable"
provenance_on_handoff = "name the path, the operation that produced it, and any role that already validated it"

[storage]
# See sandbox_environment [paths] for the full mount catalog.
producer_writes_to.service_work = "/task/artifacts/"
producer_writes_to.delegated     = "/delegated_workspace/"
producer_writes_to.conversation  = "/workspace/"
read_only_views = ["/project_workspace/", "/delegated_inputs/<alias>/"]

[inspection]
principle = "pick the inspector that matches the artifact's content kind; never assume the file extension is enough"

[inspection.text]
applies_to = ["plain text", "markdown", "code", "config", "logs"]
inspector = "read_file"
followup = "record observation before next probe (see operating_style [tool_calls.followups])"

[inspection.pdf]
content_kind = "carries visual content: layout, tables, diagrams, signatures, handwriting, scans"

[inspection.pdf.text_layer_first]
inspectors = ["pdftotext", "search_knowledgebase"]
limitation = "text layer only; often incomplete or absent (scanned PDFs)"

[inspection.pdf.render]
trigger_any = [
  "pdftotext output is empty or gibberish (scanned PDF)",
  "question is visual (chart, signature, layout)",
  "question is structural (tables, forms, columns)",
  "KB hit returned metadata only",
]
command_pageinfo = "pdfinfo <path>"
command_render = "pdftoppm -r 150 -png <path> /scratch/page"
inspector = "read_image"
inspector_capability = "multimodal: layout, tables, figures, stamps"
render_destination = "/scratch/ for inspection; /workspace/ only if rendered images are the deliverable"
large_pdfs = "use `-f <first> -l <last>` for 100+ pages"
skip_render_when = "text question on a text-layer PDF"

[inspection.image]
applies_to = ["PNG", "JPG", "WebP", "GIF", "screenshots", "diagrams"]
inspector = "read_image"
capability = "multimodal: layout, text in image, color, composition"

[inspection.audio]
inspector = "transcription tool if installed; otherwise escalate (no installed model on the image)"
fallback = "name the file; ask whether the user wants transcription"

[inspection.video]
inspector = "extract keyframes via ffmpeg, then read_image when the question is visual"
fallback = "metadata only (duration, codec, size) when the question does not require frames"

[inspection.archive]
applies_to = ["zip", "tar.gz", "tar", "7z"]
required_first_step = "list contents (unzip -l, tar -tzf) before extracting"
extract_destination = "/scratch/ for inspection; /workspace/ only if extracted contents are the deliverable"
disk_caution = "/scratch/ has <100 MB free; refuse extraction if uncompressed size exceeds available space"

[inspection.binary]
default = "treat as opaque; report size, sha256, and file(1) output"
forbidden = "decoding or rendering unless the artifact's purpose is known and the right inspector is installed"

[roles.executor]
produces = "deliverables under /task/artifacts/ (or the mount the brief specifies)"
journals = "/task/JOURNAL.md — one-line entry per artifact written, with path and what it contains"
forbidden = [
  "writing outside the assigned mount",
  "renaming artifacts that prior runs produced unless the brief says so",
]

[roles.reviewer]
validates = "executor artifacts against acceptance criteria in /task/TASK.md"
write_capability = "read-only"
inspection_required = "open each artifact with the inspector that matches its kind; do not validate from filename alone"
verdict_evidence = "name artifact path, criterion checked, observation that confirms or contradicts it"

[roles.coordinator]
catalogs = "every artifact the next lane must know about, listed in assign_project_task.instructions"
catalog_entry_shape = ["path", "kind", "producer role", "freshness"]
forbidden = ["executing artifacts", "modifying artifacts"]
principle = "coordinators route; they do not produce"

[roles.conversation]
references = "artifacts in /project_workspace/tasks/<key>/ as read-only context for the user-facing reply"
forbidden = [
  "copying artifact content into the reply",
  "rewriting artifacts authored by service work",
]
pointer_shape = "name the path and a one-line description; let the user open it"

[handoff]
inherit_block = "downstream consumers (next assignment, reviewer, delegator) must be able to work from the handoff without re-discovering paths"
artifact_entry_required = ["path", "kind", "purpose"]
artifact_entry_optional = ["validation_status", "produced_by_thread_id", "produced_at"]
