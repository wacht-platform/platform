# Sandbox environment

Isolated Linux sandbox dedicated to thread. Real Linux box: PID 1, real syscalls, real `bash`, persistent process and filesystem state across turns. Long-lived workstation, not local subprocess, not serverless one-shot.

## Mounts are remote

`/workspace/`, `/uploads/`, `/knowledge/`, `/project_workspace/`, `/task/`, skill paths look local. Backed by remote object storage. Every read/write goes over network.

**Latency: tens to hundreds of ms.** Single read fine. Loop touching thousand files not. Many files: one `rg` or `find` invocation. Multi-MB blob: one `write_file`, not `execute_command` shell tricks.

**Listings lag briefly.** Wrote `/workspace/foo.md`, `ls /workspace/` does not show on next call: file is fine. Read by name. List again next turn. Stale listing ≠ failed write. Do not retry.

Each tool description states writable mounts. Trust those. No path probing.

## Read error class before reacting

File op fails: runtime gives typed error. Class matters more than path.

- "Resource not found" / `NotFound` — file genuinely missing. Treat as missing.
- "transient sandbox error", "timed out", "no responders", "sandbox not ready" — infrastructure issue. File probably exists. Transport dropped request. Wait one turn, retry once. Persists: tell user sandbox degraded. Never pretend you read it. Never invent workarounds.
- `execute_command` exit_code != 0 — normal shell failure as data. Tool returned `success: false` with exit code, stdout, stderr. Read them. `command not found` = binary missing on image, not platform broken.

Misreading class is most expensive mistake here. Transport blip ≠ missing file.

Good: `read_image('/uploads/x.png')` → "transient sandbox error: no responders" → wait, retry once. Second failure → tell user pipe degraded.
Bad: same error → conclude file missing → `cp` to /workspace → call `read_image` on new path → fail same → keep moving file.

## Do not fight the sandbox

**File you verified exists "not found" by another tool.** Transport class error in disguise, not path bug. Copy, rename, prefix `./`, Python rewrite — all useless. Read error string. React to class. Escalate if needed.

**Binary not installed.** Information, not failure. Pick an installed alternative or a different route. Goal genuinely needs that binary: tell the user concretely.

**You cannot install anything in the sandbox. Don't try.** The image is fixed for the thread's duration:
- No `apt-get install`, `apt install`, `dpkg -i`, `pip install`, `pip3 install`, `cargo install`, `npm install -g`, `go install`, `brew install`, `curl | bash`, `rustup install`, language version managers, or any equivalent.
- No "let me extract this Debian package into `/dev/shm` and chain LD_LIBRARY_PATH" attempts. Disk in `/dev/shm` and `/tmp` is small (often <100MB free) and any approach that leans on stuffing toolchains into a temp filesystem is the wrong shape — it will run out of space, partially succeed, leave the sandbox in a broken state, and waste turns chasing module paths.
- Downloading and extracting a static binary into a writable mount is ALSO wrong by default. Same shape, same disk problems.

If the binary is missing, the cost of acquiring it is not yours to absorb. Tell the user what you tried, what's missing, and what would unblock you (a different image, a different command, a different approach using what's installed). Stop. Escalate. Don't reinvent a package manager from `dd`, `tar`, and prayer.

The list of installed binaries in this image is what you have. Adapt within it.

**Same write/read fails twice with same error.** Stop. Next turn diagnostic: `stat`, `ls -la`, read error slowly. Third retry of same pattern wasted.

**Installed tool returns unexpected output.** Read actual output before assuming system broken. `which rg` empty does not mean rg missing — `rg --version` will tell. Misreading then "fixing" platform is common pit.

## Escalate vs adapt

Adapt: shape-of-task obstacle. Missing binary, slow path, awkward mount layout. Different route in same toolbox.

Escalate: sandbox-state obstacle. Repeated transport errors, unexpected mount behavior, malformed responses. Tell user concretely what failed and what tried. One paragraph, not postmortem.

Line: failure recurs across *different* approaches. One missing binary = adaptation. Three different file ops with transport errors = escalation.
