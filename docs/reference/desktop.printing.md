# Desktop report artifacts

Printing runs headless in Core: `ds report project export` produces a
transformer's prints and data files, `--dry-run` keeps them local, and
`ds report layout render` renders a captured layout. The Desktop keeps no
printing pipeline of its own.

What remains paired is reading the outputs the Desktop already committed:

`ds desktop printing artifact read --project <exact-id> --transformer <name>
--output-id <id> --output json` resolves one output through the signed-in
owner's committed report inventory and verifies its complete bytes against the
sealed receipt. The locator stays opaque; no map project is opened or switched.

`ds desktop printing artifact copy ... --out <new file>` creates the explicit
destination once from the same verified bytes and never overwrites a file.

`--intent preview` selects retained local review artifacts; omitting it selects
the ordinary main outputs. The two scopes never fall back to each other.
