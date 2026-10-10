# Fast, scoped build cleanup on ds-server

Use this procedure when the owner requests disk cleanup. It is operational memory,
not a scheduled cleaner. A requested target belongs to that cleanup; it does not
replace the workspace's ordinary 90% disk limit or authorize deleting project data.

Start with `df -h /`, `free -h`, active build/release processes and the worktree
registry at `git_trees/gittrees-index.json`. Check candidate process working
directories and open files before removal. Never kill another process to clean its
outputs. Leave active build/release roots alone, including a manual owner build.
No SIGKILL. Preserve unsaved/unlanded work; prove every worktree commit landed on
`origin/run` before retiring its tree through Git.

Inventory these known locations directly instead of rescanning the whole home:

| Class | Locations under `/home/magese` | Rule |
| --- | --- | --- |
| Disposable per-run output | `data-solutions/.release/`, named `ds-*/out/<run>/`, owned disk TMPDIRs, `.cache/claude-build-tmp/` | Delete only completed, unused output whose ownership and remaining evidence are known. A report or admitted artifact in `out/` is not disposable merely because of its parent directory. |
| Cargo build caches | `data-solutions/{ds-cli,ds-network,ds-command-kernel}/target/`, `data-solutions/ds-command-kernel/crates/{ds-layer-store,ds-client-core,ds-report-host}/target/`, `data-solutions/ds-web/src-tauri/target/`, `.cache/ds-griddesign/` | Reusable by default. An explicit fresh-build/cache-clearing request can admit their removal after activity checks; state the rebuild cost and preserve required packaged artifacts. |
| Other reusable caches | `.cache/go-build/`, Go module cache, Cargo registry, web dependencies, Flutter `.dart_tool`/`.pub-cache`, Terraform `.terraform` | Preserve during routine cleanup. Do not bundle them into a temporary-build deletion. |
| Project data and deliverables | `projects/`, Drive mounts, report/publication artifacts, source assets, credentials and session recovery files | Preserve. A disk target is not permission to remove these. |

Recorded inventory on 2026-10-10: CLI target 133 GiB, Network target 156 GiB,
Kernel target 20 GiB plus nested crate targets 28 GiB, Desktop target 37 GiB, Web out 4 GiB,
`.cache/ds-griddesign` 44 GiB and Go build cache 20 GiB. These are hints for the
next bounded `du`, not proof that files are still idle or safe to delete. Those idle Cargo targets and the Desktop/Go compilation caches were removed
during the owner-requested fresh-build cleanup; do not expect them still to exist.
There were zero registered worktrees. Recheck sizes and activity each time.

Remove admitted disposable classes first and recheck `df -h /` after each class;
stop at the requested target. If they are insufficient, report the remaining
space and exact additional class rather than silently erase warm caches or
deliverables. Remove only explicitly authorized idle build caches when a fresh
disk is requested. Do not run the old `~/.local/bin/ds-disk-steward` blindly: its
age-based cleanup rules predate the current ownership/cache agreements.

Keep scratch on disk, never `/tmp` or `/dev/shm`. Keep the next release's captured
sources, active TMPDIR and immutable artifacts intact. Keep reusable caches warm
unless this cleanup explicitly includes them. There is no background cleanup or
automatic deletion merely because a path exceeds a remembered size.
