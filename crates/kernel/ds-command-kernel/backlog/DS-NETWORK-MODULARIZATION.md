# DS Network modularization: shared Rust core, native executable and browser WASM

The maintained guide moved to [ds-architecture](../../ds-architecture/backlog/DS-NETWORK-MODULARIZATION.md). Use its [canonical decoupling queue](../../ds-architecture/backlog/DECOUPLING.md) for new work and acceptance. This one-release pointer retains the old anchors; evidence and unrelated Kernel backlogs remain in place. Remove it after the first qualified release using the new routing and an inbound-link check.

<a id="1-what-counts-as-uncoupled"></a>
<a id="2-source-evidence-and-lessons-to-retain"></a>
<a id="21-local-inspection-boundary"></a>
<a id="22-geolibre-lessons-translated-into-extraction-obligations"></a>
<a id="3-current-coupling-to-remove-rather-than-recreate"></a>
<a id="4-destination-and-dependency-direction"></a>
<a id="proposed-package-roles"></a>
<a id="5-what-moves-to-kernel-and-what-must-stay-out"></a>
<a id="6-one-closed-typed-operation-protocol"></a>
<a id="61-handshake-and-identity"></a>
<a id="62-request-event-and-result-vocabulary"></a>
<a id="63-transport-and-bulk-data"></a>
<a id="64-determinism-and-compatibility"></a>
<a id="7-native-lifecycle-state-and-concurrency"></a>
<a id="71-stateless-lv-and-persistent-grid-work"></a>
<a id="72-cpu-memory-and-cancellation"></a>
<a id="73-updates-during-work"></a>
<a id="74-remaining-cli-managed-admission-edges"></a>
<a id="8-browser-wasm-lifecycle-and-package-boundaries"></a>
<a id="native-continuity-release-and-independent-browser-publication"></a>
<a id="remaining-ui-consumer-and-retirement-edges"></a>
<a id="9-native-and-browser-capability-matrix"></a>
<a id="10-sequential-implementation-slices-and-removal-gates"></a>
<a id="11-detailed-acceptance-packet"></a>
<a id="111-semantic-and-artifact-fixtures"></a>
<a id="112-process-and-isolation-tests"></a>
<a id="113-build-and-dependency-tests"></a>
<a id="114-scale-and-performance-gates"></a>
<a id="115-releaserollback-proof"></a>
<a id="12-consumer-migration-checklist"></a>
<a id="13-explicit-limits-and-decision-handling"></a>
<a id="14-backlog-placement-and-retirement"></a>
<a id="15-first-coding-session-packet"></a>
