# Desktop geographic data distribution

`ds desktop data rwanda catalog --yes` reconciles geographic BigQuery sources from governed global-tile publication workspaces into the Firestore `eds_data_catalog` collection. Each dataset records its opaque identity, source version, feature rows, source bytes, and—when published—its immutable gzip GeoJSON Sequence bundle, compressed transfer bytes, expanded desktop bytes and digests.

`ds desktop data rwanda status` lists the active project's visible catalog and verifies the exact local version and spatial index. Every row carries `hold`: `held`, `not_held`, `skipped_for_disk` or `unavailable`, and the receipt carries this volume's total, available, reserved and spendable bytes — so "why is this dataset not here?" is answerable from the receipt alone. Catalog rows without a bundle remain visible as unavailable for installation. They do not block maps or printing.

`ds desktop data rwanda publish --resource <id> --yes` publishes one catalog dataset through the existing governed global-tile worker, waits for its compressed Desktop bundle, and reconciles the bundle receipt back into Firestore. The command accepts only an opaque ID returned by status; it never accepts SQL, a BigQuery table or a storage path.

`ds desktop data rwanda install` without `--resource` downloads and indexes **every** published dataset this computer does not already hold, smallest first. It is safe to run at any time, including in the middle of preparing a report or a map: it spends disk and nothing else, changes nothing in the cloud, and `ds desktop data rwanda remove` puts every byte back. Nobody has to ask for it.

The only bound is this volume. The planner holds back a reserve — a tenth of the volume or 20 GiB, whichever is smaller — and spends what is left, smallest dataset first. A dataset whose planned size (expanded rows plus the bundle staged to make them) would cross the reserve is **skipped and named** in the receipt as `{id, planned_bytes, reason: "disk_reserve"}`; it is never a refusal of the whole call, and it is never a statement about the dataset's size in the abstract. Free the disk, or point `ds desktop data rwanda storage --path` at a larger volume, and the next run takes it.

Repeat `--resource <id>` for an explicit set; the reserve still applies and still reports. Use `--max-download-mib 100` during bounded acceptance runs to skip any compressed bundle larger than 100 MiB — its absence no longer means "small datasets only". The receipt reports what was installed, what was already held, what was skipped and why, the volume before and after, and per dataset the source version, compressed transfer, expanded payload, spatial index and total local disk sizes.

The Data page checks the catalog when opened and every 15 minutes while visible. Downloads continue through its application-level background queue when the page closes. A changed source ETag/modified time creates a new immutable version and appears as an explicit update; it is never installed automatically.

`ds desktop data rwanda storage` reports the active app-owned storage root and the filesystem's available and total bytes. `--path /existing/parent` selects a different disk and creates an app-owned child there; `--path recommended` restores the platform app-data root. Changing the root never copies or deletes the old cache.

`ds desktop data rwanda remove --resource <id>` removes every locally indexed version of each repeated dataset from this computer. It leaves the governed Firestore catalog and Cloud source unchanged.

Cloud queries go to Brain with a Polygon or MultiPolygon and query the native BigQuery geography table. Desktop queries use only the explicitly installed local SQLite RTree version. There is no provider fallback: offline or local mode refuses when the exact dataset version has not been installed.
