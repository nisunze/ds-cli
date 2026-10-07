# Principles

The standing owner rulings for everyone who works the backlog. [OPEN.md](OPEN.md) holds the unfinished code, [OPEN-X.md](OPEN-X.md) the owner decisions, [SEEDING.md](SEEDING.md) parameters, seeding and operational acceptance, [ORDER.md](ORDER.md) the order in which slices start, and [FEEDBACK.md](FEEDBACK.md) a snapshot of unclosed feedback. A ruling lives here once; the lists point to it rather than repeat it. Change a ruling only on the owner's word, with its date.

## 1. The backlog split (owner, 2026-10-07)

- **OPEN.md** — unfinished code that an agent can implement and prove without further owner input: product defects, missing native CLI/MCP capabilities, UI bindings, test repairs and integration.
- **OPEN-X.md** — unresolved owner decisions, authority and supervised steps, each naming the exact input and the boundary it controls.
- **SEEDING.md** — template building and seeding that require **no code change** (owner, 2026-10-07): templates, parameters, style documents, engineering/data seeds and library curation through existing `ds` contracts, plus deployment, install and acceptance of already-landed code. Any part that needs a code change moves to OPEN.md.
- Section numbers keep the former work-domain identity across the three files.

## 2. How to work an entry

- **Use the live `ds` contract before coding.** First try the admitted project-owned parameters and seeds. A valid configuration ignored by the renderer, an absent required capability, incorrect computation, clipping/overflow or a broken contract is code (OPEN.md); a colour/pen/label/title/extent choice is seeding (SEEDING.md). Do not alter engineering facts to improve a picture.
- **Recheck legacy claims** against current code and tests before implementing.
- **Delete proven fixes permanently.** Keep evidence in owning code, contracts, tests or project receipts, with no completed-work archive.
- **Split mixed obligations.** Finish the independently actionable code and link only its unresolved owner decision in OPEN-X.md.
- **Do not park routine work as owner questions:** implementation, tests, already-authorized features and work awaiting another host are not owner questions. If existing contracts or evidence settle a question, proceed and remove that dependency.
- **Never-landed work** (owner, 2026-10-02): finished work found on a branch or tree that never reached `run` is landed adapted to current `run`.
- **One slice at a time per repository**, proven, committed and pushed on `run` before the next.

## 3. Priorities (owner, 2026-10-07)

1. **`ds` reliability reporting and proactive feedback** (OPEN.md block 25) is a priority.
2. First-release work: clear the reproduced CLI source gates, complete the provider-model library lifecycle (block 18), integrate the existing PLS-CADD controls (block 21), and move touched UI controls to the kernel with JSON input/preview and replacement-code deletion (block 23).
3. Fix measured renderer and compute failures.
4. Unrelated tuning and native candidate qualification continue through SEEDING.md.

[ORDER.md](ORDER.md) turns these into a start sequence.

## 4. Report as you go (owner, 2026-10-07)

`ds` logs its own failures to SRE and files feedback without being asked; anyone using `ds` — agent or person — files sightings unprompted, and lists the report ids at the end of the task. Nobody waits for the owner to ask. Contract: [ds-cli-reliability-and-feedback.md](../docs/contracts/ds-cli-reliability-and-feedback.md).

## 5. Kernel, CLI and UI (owner, 2026-10-07)

- **Migrate touched controls for the first release.** Work one owning vertical at a time; a polished UI can follow the native contract and a usable JSON input/preview. Trace the explicitly named consumers rather than starting an all-repository rewrite.
- **Complicated TypeScript goes to the CLI; its UI becomes JSON.** When a UI rests on complicated TypeScript logic, migrate that logic into the kernel and CLI/MCP, then delete the whole UI and replace it with a JSON input editor and a JSON output view over the same CLI contract. Losing UI elements in favour of the CLI is perfectly acceptable: the product comes from the future and there is no going back.
- **Never destroy validation paths.** Keep, and keep working, every view that renders results for a person to check — Markdown, HTML, PDF, XLSX, images and maps, and the like. Keep the capacity to download those results from the UI as well; downloading the same artifacts through the CLI is welcome, but it does not replace the UI download. Removing a control is allowed; removing the way a person sees, verifies and downloads a result is not.

## 6. DS Grid engineering

- Windows owns DS Grid engineering; the server session integrates the other surfaces.
- Measure → test → fix → remeasure on exact reference revisions.
- Preserve engineering authority and record numbers as owning tests — never per-model constants or widened tolerances.

## 7. No single-country assumptions

Owner: "it will be adopted internationally, so no hardcoding at all." No fallback may turn one country's input into another's; code must not know rule-set names (the live Chad rule set is `rule_tchd`). The queued slices are in OPEN.md block 19.
