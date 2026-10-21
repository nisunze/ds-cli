# Contributing to ds-command-kernel

Start with [AGENTS.md](AGENTS.md) and its owning contract/check links.
Read [the shared collaboration agreement](../ds-command-kernel/docs/agent-guidance/collaboration.md).

1. Inspect current run/HEAD, worktree and index; preserve concurrent authors' work.
2. Identify one owning behavior and acceptance proof; change one coherent slice.
3. Run the affected checks and negative cases, then the required wider gates.
4. Update the owning contract and consumers when a boundary changes.
5. Review the diff, stage only owned paths, commit and push run after proof.

Long domain references live under docs/. Root AGENTS.md stays a short entry point.
No feature branch/worktree, deployment or new product surface is implied by this
contribution guide. Use the existing local release contract when release is requested.
