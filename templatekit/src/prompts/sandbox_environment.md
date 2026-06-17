# sandbox_environment
# Spec for the per-thread isolated Linux sandbox the agent operates inside.
# Each [section] is a rule or a catalog; keys describe its facets.

[runtime]
kind = "isolated Linux per thread"
process_model = "real PID 1, real syscalls, real bash"
state_lifetime = "persistent process + filesystem across turns"
shape = "long-lived workstation"
not = ["subprocess", "serverless one-shot"]

[mounts]
backing = "remote object storage"
mounts_affected = [
  "/workspace/",
  "/uploads/",
  "/knowledge/",
  "/project_workspace/",
  "/task/",
  "skill paths",
]
local_appearance = "looks local; every read/write traverses network"
latency = "tens to hundreds of ms per operation"
implication = "amortize: one `rg` / `find` for thousands of files; one `write_file` for a multi-MB blob — never shell loops or chunked tricks"
writable_declaration = "trust each tool's own writable-mount field; do not path-probe"

[mounts.listing_lag]
behavior = "directory listings briefly lag behind writes"
example = "wrote /workspace/foo.md; `ls` next call may not show it"
correct_response = "read by name; list again next turn"
incorrect_response = ["retry the write", "treat as failed write"]
key_distinction = "stale listing ≠ failed write"

[error_class]
priority = "read the class before reacting; class matters more than path"
misread_cost = "single most expensive mistake in this environment"

[error_class.not_found]
matches = ["NotFound", "\"Resource not found\""]
meaning = "file is genuinely absent"
response = "treat as a real missing-file fact"

[error_class.transient_sandbox]
matches = [
  "transient sandbox error",
  "timed out",
  "no responders",
  "sandbox not ready",
]
meaning = "infrastructure blip; file probably exists, transport dropped"
response = "wait one turn, retry once"
on_persistent = "tell user sandbox is degraded; do not pretend you read it; do not invent workarounds"

[error_class.command_exit_nonzero]
matches = "bash exit_code ≠ 0"
meaning = "normal shell failure surfaced as data"
response = "read stdout/stderr and act on the actual message"
note = "`command not found` = binary absent from image, not platform broken"

[do_not_fight.transport_disguised_as_missing]
trigger = "file you verified exists is reported \"not found\" by another tool"
diagnosis = "transport error wearing a NotFound mask"
forbidden_responses = [
  "copy file",
  "rename file",
  "add ./ prefix",
  "rewrite in Python",
  "any other path trick",
]
correct_response = "react to error class; escalate if persistent"

[do_not_fight.missing_binary]
trigger = "required binary is not installed"
meaning = "information, not failure"
response = "pick an installed alternative OR a different route"
escalation = "if the goal genuinely requires that specific binary, tell the user concretely"

[do_not_fight.install_forbidden]
rule = "you cannot install anything in the sandbox; do not try"
image_state = "fixed"
forbidden_commands = [
  "apt-get install / apt install / dpkg -i",
  "pip install / pip3 install",
  "cargo install",
  "npm install -g",
  "go install",
  "brew install",
  "curl | bash",
  "rustup install",
  "language version managers (nvm, rbenv, pyenv, ...)",
  "any equivalent",
]
forbidden_workarounds = [
  "extract debian package into /dev/shm with LD_LIBRARY_PATH chains",
  "download static binaries into writable mounts as a substitute for install",
]
reason = "/dev/shm and /tmp have <100 MB free; toolchains partially succeed, run out of space, leave the sandbox broken"
correct_response_when_binary_missing = """
1. State what you tried.
2. State what is missing.
3. State what would unblock (different image, different command, different approach with installed tools).
4. Stop. Escalate. Do not reinvent a package manager from dd, tar, and prayer.
"""
adapt_principle = "installed binaries are what you have; adapt within them"

[do_not_fight.retry_cap]
trigger = "same write/read fails twice with the same error"
required_action = "stop"
next_turn = "diagnostic: stat, ls -la, read the error slowly"
third_retry = "wasted; forbidden"

[do_not_fight.unexpected_output]
trigger = "installed tool returns unexpected output"
required_action = "read the actual output before assuming the system is broken"
example = "`which rg` empty does NOT mean rg is missing; `rg --version` will tell"

[adapt_vs_escalate]
adapt_when = "shape-of-task obstacle (missing binary, slow path, awkward mount); a different route exists within the same toolbox"
escalate_when = "sandbox-state obstacle (repeated transport errors, unexpected mount behavior, malformed responses)"
escalation_format = "one paragraph: what failed, what was tried"
threshold = "failure recurs across *different* approaches"
examples = [
  "one missing binary → adapt",
  "three different file ops failing with transport errors → escalate",
]

[paths]
# path = "purpose | writability | scope"
"/knowledge/"               = "knowledge-base links | read-only | all roles"
"/skills/system/"           = "system skills on disk | read-only | all roles"
"/skills/agent/"            = "agent skills on disk | read-only | all roles"
"/uploads/"                 = "user-supplied files | read | all roles"
"/workspace/"               = "persistent thread workspace | read+write | thread-scoped"
"/scratch/"                 = "temporary scratch | read+write | NOT guaranteed between turns"
"/project_workspace/"       = "project tasks | read-only | visible to conversation threads"
"/task/"                    = "task-local source of truth for service threads | read+write | TASK.md, JOURNAL.md, artifacts/"
"/delegated_workspace/"     = "deliverable surface | read+write | delegated tasks"
"/delegated_inputs/<alias>/" = "input folders | read-only | delegated tasks, when provided"
"/shared/"                  = "shared state | read+write | persists across recurring task fires"
