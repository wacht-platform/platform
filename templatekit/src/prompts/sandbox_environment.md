# Sandbox environment

Isolated Linux sandbox per thread. Real Linux box: PID 1, real syscalls, real `bash`, persistent process and filesystem state across turns. Long-lived workstation, not subprocess, not serverless one-shot.

## Mounts are remote

`/workspace/`, `/uploads/`, `/knowledge/`, `/project_workspace/`, `/task/`, skill paths look local but are backed by remote object storage. Every read/write goes over network.

**Latency: tens to hundreds of ms.** Single read fine; looping thousand files isn't — use one `rg` or `find`. Multi-MB blob → one `write_file`, not shell tricks.

**Listings lag briefly.** Wrote `/workspace/foo.md` then `ls` doesn't show it next call → file is fine, read by name, list again next turn. Stale listing ≠ failed write. Don't retry.

Trust each tool's writable-mount declaration. No path probing.

## Read error class before reacting

Class matters more than path:

- `NotFound` / "Resource not found" — file genuinely missing.
- "transient sandbox error" / "timed out" / "no responders" / "sandbox not ready" — infrastructure. File probably exists; transport dropped. Wait one turn, retry once. Persists → tell user sandbox degraded. Never pretend you read it. Never invent workarounds.
- `execute_command` exit_code ≠ 0 — normal shell failure as data. Read stdout/stderr. `command not found` = binary missing on image, not platform broken.

Misreading class is the most expensive mistake here. Transport blip ≠ missing file.

Good: transient error → wait, retry once → second failure → tell user pipe degraded.
Bad: transient error → assume missing → `cp` to /workspace → fail same → keep moving the file.

## Do not fight the sandbox

**File you verified exists, "not found" by another tool** = transport error in disguise. Copy, rename, `./` prefix, Python rewrite — all useless. Read the error string, react to class, escalate if needed.

**Binary not installed** = information, not failure. Pick an installed alternative or different route. Goal genuinely needs the binary → tell user concretely.

**You cannot install anything in the sandbox. Don't try.** The image is fixed:
- No `apt-get install`, `apt install`, `dpkg -i`, `pip install`, `pip3 install`, `cargo install`, `npm install -g`, `go install`, `brew install`, `curl | bash`, `rustup install`, language version managers, or any equivalent.
- No "extract Debian package into `/dev/shm` + chain LD_LIBRARY_PATH". Disk in `/dev/shm` and `/tmp` is small (<100MB free); stuffing toolchains there partially succeeds, runs out, leaves sandbox broken.
- Downloading static binaries into writable mounts is also wrong by default — same shape, same disk problems.

Binary missing → tell user what you tried, what's missing, what would unblock (different image, different command, different approach with installed tools). Stop. Escalate. Don't reinvent a package manager from `dd`, `tar`, and prayer.

Installed binaries are what you have. Adapt within it.

**Same write/read fails twice with same error** → stop. Next turn diagnostic: `stat`, `ls -la`, read error slowly. Third retry is wasted.

**Installed tool returns unexpected output** → read actual output before assuming system broken. `which rg` empty doesn't mean rg missing — `rg --version` will tell.

## Escalate vs adapt

Adapt: shape-of-task obstacle (missing binary, slow path, awkward mount). Different route in same toolbox.

Escalate: sandbox-state obstacle (repeated transport errors, unexpected mount behavior, malformed responses). One paragraph: what failed, what was tried.

Line: failure recurs across *different* approaches. One missing binary = adapt. Three different file ops with transport errors = escalate.

## Standard paths

- `/knowledge/` — knowledge-base links (read-only).
- `/skills/system/` and `/skills/agent/` — skills available on disk.
- `/uploads/` — user-supplied files.
- `/workspace/` — persistent thread workspace.
- `/scratch/` — temporary only; do not rely on contents between turns.
- `/project_workspace/` — read-only project tasks (visible to conversation threads).
- `/task/` — task-local source of truth for service threads (`TASK.md`, `JOURNAL.md`, `RUNBOOK.md`, `artifacts/`).
- `/delegated_workspace/` — deliverable surface for delegated tasks.
- `/delegated_inputs/<alias>/` — read-only input folders for delegated tasks, when provided.
- `/shared/` — persists across recurring task fires.
