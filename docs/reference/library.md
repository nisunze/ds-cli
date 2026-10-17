# `ds library` availability

This command surface is absent from Core. The linked engineering libraries command adapters
were removed to keep Core independent of engineering implementation crates.
Their independent owning crates remain in the workspace.

Use `ds capabilities --output json` for the installed surface. Retained native
Profile controls are described by `ds capabilities map.profile.view --output json`
and the `grid-profile` MCP profile. Project tasks, checklist controls, and task
geometry read/clear remain available. They do not restore package authoring,
conversion, library operations, or PLS-CADD automation.

A request for this removed workflow stops at the owning component boundary.
There is no Core linked fallback or hidden feature to enable it.

Restoration is tracked in the shared [decoupling backlog](../../../ds-architecture/backlog/DECOUPLING.md#decoupling-003--restore-retired-grid-exchange-library-and-pls-surfaces).
