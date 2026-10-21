# Foundation Datasets Contract (pointer)

Owned by ds-brain: [`ds-brain/docs/contracts/foundation-datasets.md`](https://github.com/nisunze/ds-brain/blob/run/docs/contracts/foundation-datasets.md)
(in a workspace checkout, `../ds-brain/docs/contracts/foundation-datasets.md`).
Cloud-resident reference data (customers, UPI parcels) is never installed
nationally: it is read bounded or seeded per project, and the catalogue row's
residency decides which. The R1–R7 rule numbers cited by this repo's code
(`ds-client-core::data_distribution`, `ds-project-data`, `project_dataset_cache`)
refer to that text; its planned spatial reads (`spatial_plan`/`spatial_execute`)
are the bounded reads `ds-client-core` sends.
