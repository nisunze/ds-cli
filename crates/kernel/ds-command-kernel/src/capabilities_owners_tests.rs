//! Owners -> rows: every Kernel export the page can call either is the
//! `export` of a capability row, or is listed here with why it has no row of
//! its own. A new product export without a row fails this test by name; so
//! does a stale entry. The list is explicit, never a pattern.
use super::*;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

/// The build-time scan of the crate's wasm-bindgen exports.
#[path = "../build_capabilities.rs"]
#[allow(dead_code)]
mod compiler;

/// Why an export is not a row's `export`.
#[derive(Clone, Copy, Debug)]
enum Why {
    /// Decoders, stores (held/keep/forget, outboxes), handles and classes,
    /// init and identity, capability routing, pure helpers, session and
    /// telemetry plumbing: no capability of its own.
    Mechanics(&'static str),
    /// A secondary step of an in-page row whose `export` is its primary owner.
    Step(&'static str),
    /// A Kernel decision or computation over held data that the row's Core
    /// command shares; the page runs it where the row is not a Kernel cell.
    Shared(&'static str),
    /// Rows the coordinator adds when the named work lands.
    Pending(&'static str),
    /// A product export whose capability needs an owner decision for its row.
    NeedsRow(&'static str),
    /// No production caller: a delete candidate.
    Dead(&'static str),
}
use Why::{Dead, Mechanics, NeedsRow, Pending, Shared, Step};

const NOT_ROWS: &[(&str, Why)] = &[
    ("CapturedLvProcess", Mechanics("handle")),
    ("DurableStoreReply", Mechanics("store")),
    ("EndedSession", Mechanics("session handle")),
    // The browser report export session; its rows (`report.project.export`,
    // `report.project.combined`) stay pending until the page builds the
    // native request from held inputs (OPEN.md §7).
    ("ReporterExportSession", Pending("web report exports: no page request builder yet")),
    ("SelectionCoordinator", Mechanics("handle")),
    ("SessionHandle", Mechanics("handle")),
    ("SolarCalculation", Mechanics("handle")),
    ("StateSignal", Mechanics("handle")),
    ("StateSubscription", Mechanics("handle")),
    ("SurveyPhotoBundleWasm", Mechanics("handle")),
    ("SurveyPhotoImageWasm", Mechanics("handle")),
    ("accountCapabilities", Dead("no ds-web caller")),
    ("accountCapabilitiesHeld", Mechanics("store")),
    ("accountIdentityRepair", Mechanics("session")),
    ("accountProfileHeld", Mechanics("store")),
    ("adminChildren", Step("data.admin-bounds.list")),
    ("adminChildrenHeld", Mechanics("store")),
    ("adminDescendants", Step("data.admin-bounds.read")),
    ("adminLocate", Step("data.admin-bounds.read")),
    ("adminName", Step("data.admin-bounds.read")),
    ("applicationDestination", Mechanics("helper")),
    ("applicationNavigation", Mechanics("helper")),
    ("applicationSettings", Mechanics("helper")),
    ("assetsCatalogueHeld", Mechanics("store")),
    ("assetsCatalogueRefresh", Step("assets.list")),
    ("assetsGet", Step("assets.download")),
    ("assetsList", Step("assets.list")),
    ("authGoogleBegin", Pending("Google sign-in waits for its OAuth redirect (row pending)")),
    ("authGoogleComplete", Pending("Google sign-in waits for its OAuth redirect (row pending)")),
    ("authSessionSignal", Mechanics("session signal")),
    ("backupLedgerCommand", Step("assets.backup.list")),
    ("backupLedgerHeld", Mechanics("store")),
    ("bindSolarHostedPortfolioData", Mechanics("helper")),
    ("browserModule", Shared("core.component.selection")),
    ("buildSolarPromptBundle", NeedsRow("the Solar prompt bundle deliverable")),
    ("capabilityCommandAdmission", Mechanics("capability routing")),
    ("capabilityCoreRefusal", Mechanics("capability routing")),
    ("capabilityCoreRequestAdmission", Mechanics("capability routing")),
    ("capabilityFailure", Mechanics("capability routing")),
    ("capabilityIsKnown", Mechanics("capability routing")),
    ("capabilityIsRunnable", Mechanics("capability routing")),
    ("capabilityRoute", Mechanics("capability routing")),
    ("capabilityRun", Mechanics("capability routing")),
    ("captureLvProcess", Mechanics("helper")),
    ("collisions", Shared("design.collisions")),
    ("combinedReadiness", NeedsRow("Combined Report readiness")),
    ("commandShortcutPreferences", Mechanics("helper")),
    ("compareGeoJsonLayers", Shared("design.project.compare")),
    ("computeArtifactDownload", Step("report.publication.download")),
    ("computeArtifactLocator", Mechanics("helper")),
    ("computeArtifactPublishedHead", Shared("report.publication.show")),
    ("connectivityCommand", Shared("desktop.offline.set")),
    ("coreReleasePresentation", Shared("core.release-identity")),
    ("deadLetterSummary", Mechanics("sync diagnostics")),
    ("decodeDesignCellLockVocabulary", Mechanics("decoder")),
    ("decodeDesignDomainValue", Mechanics("decoder")),
    ("decodeDesignFeaturesSelect", Mechanics("decoder")),
    ("decodeDesignSessionCellEdit", Mechanics("decoder")),
    ("decodeDesignSessionCellEdits", Mechanics("decoder")),
    ("decodeDesignSessionDiff", Mechanics("decoder")),
    ("decodeDesignSessionHover", Mechanics("decoder")),
    ("decodeDesignSessionProjection", Mechanics("decoder")),
    ("decodeDesignSessionRow", Mechanics("decoder")),
    ("decodeDesignSessionScopeStatus", Mechanics("decoder")),
    ("decodeDesignSessionSelection", Mechanics("decoder")),
    ("decodeDesignSessionSnap", Mechanics("decoder")),
    ("decodeFeatureLookupResult", Mechanics("decoder")),
    ("decodeGeoJsonComparison", Mechanics("decoder")),
    ("decodeGeoJsonStreamReply", Mechanics("decoder")),
    ("decodeGeoJsonStreamStep", Mechanics("decoder")),
    ("decodeGeometryOperation", Mechanics("decoder")),
    ("decodeGpkgGeometry", Mechanics("decoder")),
    ("decodeGridEngineReply", Mechanics("decoder")),
    ("decodeLayerExport", Mechanics("decoder")),
    ("decodeLayerExportReply", Mechanics("decoder")),
    ("decodeLayerImport", Mechanics("decoder")),
    ("decodeLayerImportReply", Mechanics("decoder")),
    ("decodeSelectionFieldProfile", Mechanics("decoder")),
    ("decodeSolarCalculateReply", Mechanics("decoder")),
    ("decodeTableEdit", Mechanics("decoder")),
    ("decodeTableFilterAnswer", Mechanics("decoder")),
    ("decodeTableFilterResult", Mechanics("retired page export: typed update refusal")),
    ("decodeTableProfilesAnswer", Mechanics("decoder")),
    ("decodeTableProfilesResult", Mechanics("retired page export: typed update refusal")),
    ("decodeUploadBatch", Mechanics("decoder")),
    ("decodeUploadBatchReply", Mechanics("decoder")),
    ("decodeUploadCleaning", Mechanics("decoder")),
    ("decodeUploadCleaningReply", Mechanics("decoder")),
    ("decodeVectorTool", Mechanics("decoder")),
    ("designActivities", Step("design.activities.sweep")),
    ("designAttachments", Mechanics("helper")),
    ("designAutoprocess", Shared("design.autoprocess.plan")),
    ("designCellLockVocabulary", Mechanics("helper")),
    ("designConfigInspect", Mechanics("helper")),
    ("designDomainValue", Mechanics("helper")),
    ("designEdit", Shared("design.project.edit")),
    ("designFeaturesSelectEffect", Shared("design.features.select")),
    ("designFeaturesSelectOutcome", Shared("design.features.select")),
    ("designForceGate", Shared("design.force-gate.check")),
    ("designHealth", Step("design.status")),
    ("designMaterialPropagationReceipt", Mechanics("helper")),
    ("designMaterialPropagationRequest", Mechanics("helper")),
    ("designPresence", Mechanics("helper")),
    ("designProcessPlan", Shared("design.project.process")),
    ("designStatusQuery", Step("design.status")),
    ("designStatusRow", Step("design.status")),
    ("designStatusUpload", Shared("design.intake.upload")),
    ("designTags", Mechanics("helper")),
    ("designVersion", Mechanics("helper")),
    ("designVersionAuthorityCommand", Step("design.version.begin")),
    ("desktopInstance", Mechanics("helper")),
    ("durableStoreLock", Mechanics("store")),
    ("durableStoreMemoryReset", Mechanics("store")),
    ("durableStoreMemoryTransaction", Mechanics("store")),
    ("durableStoreTransaction", Mechanics("store")),
    ("editOverlays", Shared("map.local.register")),
    ("encodeDesignFeaturesSelect", Mechanics("helper")),
    ("encodeLayerImport", Pending("migration lane in flight")),
    ("encodeTableEdit", Mechanics("helper")),
    ("engineUnit", Pending("migration lane in flight")),
    ("engineUnitDevelopmentCapture", Pending("migration lane in flight")),
    ("engineUnitDevelopmentMember", Pending("migration lane in flight")),
    ("executionContext", Mechanics("helper")),
    ("fieldDomain", Mechanics("helper")),
    ("findFeatureInTabs", Mechanics("helper")),
    ("find_feature_in_tabs_v2", Mechanics("helper")),
    ("formsHeld", Mechanics("store")),
    ("gatewayAudience", Mechanics("helper")),
    ("gatewayTarget", Mechanics("helper")),
    ("geoJsonStreamEffect", Pending("migration lane in flight")),
    ("geocodeAvailable", Step("map.geocode.search")),
    ("geodataAudit", Step("map.data.list")),
    ("geodataRetry", Step("map.data.upload")),
    ("governanceHeldPage", Mechanics("store")),
    ("gpkgLayerPolicy", Mechanics("import policy over held bytes")),
    ("gridCatalogRetain", Mechanics("store")),
    ("gridEngineUnavailable", Mechanics("refusal")),
    ("gridModelCrs", Mechanics("helper")),
    ("gridModelManagement", Shared("dsgrid.model.list")),
    ("gridProjectCatalog", Mechanics("helper")),
    ("gridSavedSourceDigest", Mechanics("helper")),
    ("gridUploadStart", Step("design.model.publish")),
    ("installationInventory", Step("install.list")),
    ("installationsHeld", Mechanics("store")),
    ("layerDocument", Mechanics("helper")),
    ("layerExportEffect", Pending("migration lane in flight")),
    ("layerImportEffect", Pending("migration lane in flight")),
    ("layerState", Step("map.layer.list")),
    ("layersReorder", Dead("only a retired Desktop bridge operation reaches it")),
    ("localData", Shared("core.store")),
    ("localLayers", Shared("map.local.list")),
    ("mapActiveView", Shared("map.camera.set")),
    ("mapStateForget", Mechanics("store")),
    ("mapStateHeld", Mechanics("store")),
    ("mapStateKeep", Mechanics("store")),
    ("mediaGrant", Step("survey.photo.fetch")),
    ("mediaGrantHeld", Mechanics("store")),
    ("messagingCacheForget", Mechanics("store")),
    ("messagingCacheHeld", Mechanics("store")),
    ("messagingCacheKeep", Mechanics("store")),
    ("messagingOutboxDiscard", Step("messaging.send")),
    ("messagingOutboxEnqueue", Step("messaging.send")),
    ("messagingOutboxHeld", Mechanics("store")),
    ("messagingOutboxRetry", Step("messaging.send")),
    ("notesAttach", Step("notes.update")),
    ("notesAttachmentDownload", Step("notes.read")),
    ("notesAttachmentRemove", Step("notes.update")),
    ("notificationsHeld", Mechanics("store")),
    ("pinnedContext", Shared("design.pinned.preview")),
    ("pipelineStatusHeld", Mechanics("store")),
    ("planProjectControlBulkAction", Shared("design.bulk.plan")),
    ("planProjectControlReportDownload", Step("design.download.plan")),
    ("planProjectControlRooms", Mechanics("helper")),
    ("planSolarPortfolioMutation", Shared("solar.portfolio.create")),
    ("printingCommand", Shared("report.layout.list")),
    ("processSettings", Shared("design.process.settings")),
    ("processingUpload", Dead("never imported")),
    ("projectConfigAcquire", Step("design.config.read")),
    ("projectConfigAdmit", Mechanics("decoder")),
    ("projectConfigDocumentAdmit", Mechanics("decoder")),
    ("projectConfigFileReplyAdmit", Mechanics("decoder")),
    ("projectConfigForget", Mechanics("store")),
    ("projectConfigHeld", Mechanics("store")),
    ("projectConfigReplyAdmit", Mechanics("decoder")),
    ("projectDatasetCacheCommand", Mechanics("store")),
    ("projectDesignExtentCommand", Mechanics("helper")),
    ("projectLayerCatalog", Mechanics("helper")),
    ("projectLifecycle", Mechanics("helper")),
    ("projectManagement", Step("pm.plan")),
    ("projectSolarReportImages", Mechanics("helper")),
    ("projectWorkGraphHeld", Mechanics("store")),
    ("projectWorkGraphKeep", Mechanics("store")),
    ("projectWorkOutboxDrain", Step("pm.task.create")),
    ("projectWorkOutboxEnqueue", Step("pm.task.create")),
    ("projectWorkOutboxHeld", Mechanics("store")),
    ("projectWorkOutboxOverlay", Mechanics("helper")),
    ("projectWorkSubmittedAdmit", Mechanics("decoder")),
    ("projectWorkSubmittedClear", Mechanics("helper")),
    ("projectWorkSubmittedHeld", Mechanics("store")),
    ("projectsDirectoryHeld", Mechanics("store")),
    ("pushConfig", Step("messaging.notifications")),
    ("pushRegister", Step("messaging.notifications")),
    ("pushRevoke", Step("messaging.notifications")),
    ("readBoundSolarPortfolioResult", Mechanics("helper")),
    ("referenceCatalogueHeld", Mechanics("store")),
    ("referenceCatalogueRetain", Mechanics("store")),
    ("referencePublicationStatus", Shared("desktop.data.rwanda.status")),
    ("referencePublish", Shared("desktop.data.rwanda.publish")),
    ("referenceQueryCloud", Dead("never called in cloud mode")),
    ("referenceReconcile", Shared("desktop.data.rwanda.catalog")),
    ("releaseHeld", Mechanics("store")),
    ("reportArtifactAttach", NeedsRow("print reference attach (Desktop bridge)")),
    ("reportArtifactStart", NeedsRow("print reference attach (Desktop bridge)")),
    ("reportBrowser", Shared("report.inventory")),
    ("reportBrowserStore", Mechanics("store")),
    ("reportCommand", Shared("report.export")),
    ("reportHeadStore", Mechanics("store")),
    ("reportJobReceiptsHeld", Mechanics("store")),
    ("reportJobReceiptsHold", Mechanics("helper")),
    ("reporterEngineUnavailable", Mechanics("helper")),
    ("resolveDesignSources", Shared("design.project.resolve-sources")),
    ("responsePolicy", Mechanics("helper")),
    ("resumableUpload", Mechanics("helper")),
    ("serverSnapshot", Step("sre.overview")),
    ("serverSnapshotLiveness", Mechanics("helper")),
    ("serverStatus", Mechanics("helper")),
    ("signStorageUrls", Step("design.download.fetch")),
    ("singlePutUpload", Mechanics("helper")),
    ("solarCalculateEffect", Shared("solar.application")),
    ("solarDashboard", Shared("solar.dashboard.compose")),
    ("solarEngineUnavailable", Mechanics("helper")),
    ("solarMigration", Shared("solar.migrate.plan")),
    ("solarNetworkForm", Shared("solar.network.resolve")),
    ("solarPortfolioBatch", Shared("solar.portfolio.calculate")),
    ("solarPortfolioGraphAddress", Mechanics("helper")),
    ("solarPortfolioGraphPlan", Mechanics("helper")),
    ("solarPortfolioMembershipIdentity", Mechanics("helper")),
    ("solarPromptBundlePlan", NeedsRow("the Solar prompt bundle deliverable")),
    ("solarReportImages", Mechanics("helper")),
    ("solarResultSection", Shared("solar.results.read")),
    ("solarSavedAnalysis", Mechanics("helper")),
    ("solarSeed", Shared("solar.project.seed")),
    ("solarSnapshot", Shared("solar.input.capture")),
    ("sreActivityHeld", Mechanics("store")),
    ("sreClientEvent", Mechanics("telemetry")),
    ("storagePath", Mechanics("helper")),
    ("storedObject", Mechanics("helper")),
    ("storedObjectUrl", Step("survey.photo.fetch")),
    ("styleCatalogCurrent", Mechanics("store")),
    ("styleCatalogInvalidate", Mechanics("store")),
    ("styleCatalogRead", Dead("no production caller")),
    ("styleChannels", Mechanics("helper")),
    ("styleDocument", Mechanics("helper")),
    ("styleLegend", Mechanics("helper")),
    ("stylesAssembleRuntime", Step("style.resolve")),
    ("stylesReadShared", Step("style.read")),
    ("stylesSeed", Step("style.read")),
    ("surveyDiscardAudit", Mechanics("sync audit")),
    ("surveyEntryMutation", Step("survey.entries.update")),
    ("surveyMediaAddress", Mechanics("helper")),
    ("surveyMediaInspect", Step("survey.photo.rotate")),
    ("surveyMediaSourceBytes", Step("survey.photo.rotate")),
    ("surveyMoments", Shared("survey.moments.list")),
    ("surveyPhotoAccumulateRotation", Mechanics("helper")),
    ("surveyPhotoMediaPlan", Mechanics("helper")),
    ("surveyPhotoNormalizeRotation", Mechanics("helper")),
    ("surveyPhotoRender", Mechanics("helper")),
    ("surveyPhotoThumbnail", Mechanics("helper")),
    ("surveyPhotoThumbnailPath", Mechanics("helper")),
    ("surveyPhotoTransport", Mechanics("helper")),
    ("surveyQueueDiscard", Step("survey.entries.create")),
    ("surveyQueueEnqueue", Step("survey.entries.create")),
    ("surveyQueueRetry", Step("survey.entries.create")),
    ("surveyQueueSteps", Mechanics("helper")),
    ("surveyReportHeld", Mechanics("store")),
    ("surveyWorkingAreaForms", Step("survey.working-area.forms")),
    ("syncPlan", Shared("desktop.sync.plan")),
    ("syncStore", Mechanics("store")),
    ("taskGeometry", Step("pm.task.geometry.read")),
    ("temporaryUploadGroups", Pending("migration lane in flight")),
    ("tilingPlacement", Mechanics("helper")),
    ("transformStyle", Step("style.document.publish")),
    ("transformerStatusStore", Mechanics("store")),
    ("transformerVersionDownloaded", Mechanics("helper")),
    ("transformerVersionDownloads", Mechanics("helper")),
    ("transformerVersionDownloadsForget", Mechanics("store")),
    ("transformerVersionsHeld", Mechanics("store")),
    ("uiEngineBuildIdentity", Mechanics("helper")),
    ("uploadBatchEffect", Pending("migration lane in flight")),
    ("uploadCleaningEffect", Pending("migration lane in flight")),
    ("uploadSourceSha256", Mechanics("helper")),
    ("validateSolarPortfolioCatalog", Mechanics("helper")),
    ("validateSolarPortfolioMutationReceipt", Mechanics("helper")),
    ("validateSolarPortfolioResult", Mechanics("helper")),
    ("validateTableEditSnapshot", Mechanics("helper")),
    ("vectorToolId", Step("data.vector.run")),
    ("vectorToolReply", Step("data.vector.run")),
    ("verifiedBytes", Mechanics("helper")),
    ("verifySolarPortfolioGraphBytes", Mechanics("helper")),
];

fn crate_exports() -> BTreeMap<String, Vec<String>> {
    compiler::crate_exports(&Path::new(env!("CARGO_MANIFEST_DIR")).join("src"))
}

/// Exports no row names and the list does not explain, and listed names
/// that are no longer unrowed exports.
fn unrowed(exports: &BTreeMap<String, Vec<String>>) -> (Vec<String>, Vec<&'static str>) {
    let rowed: BTreeSet<&str> = CAPABILITIES.iter().filter_map(|c| c.export).collect();
    let listed: BTreeSet<&str> = NOT_ROWS.iter().map(|(name, _)| *name).collect();
    let missing = exports
        .keys()
        .filter(|name| !rowed.contains(name.as_str()) && !listed.contains(name.as_str()))
        .cloned()
        .collect();
    let stale = NOT_ROWS
        .iter()
        .map(|(name, _)| *name)
        .filter(|name| !exports.contains_key(*name) || rowed.contains(name))
        .collect();
    (missing, stale)
}

#[test]
fn every_ui_facing_export_is_a_rows_owner_or_listed_with_its_reason() {
    let (missing, stale) = unrowed(&crate_exports());
    assert!(
        missing.is_empty(),
        "Kernel exports with no capability row: add a row in routing/capabilities.json naming each \
         as its `export`, or list it in NOT_ROWS with why it has none: {missing:?}"
    );
    assert!(stale.is_empty(), "NOT_ROWS lists names that are rowed or no longer exported: {stale:?}");
    let mut seen = BTreeSet::new();
    for (name, why) in NOT_ROWS {
        assert!(seen.insert(name), "{name} is listed twice");
        match why {
            Step(id) => {
                let row = capability(id).unwrap_or_else(|_| panic!("{name}: no row `{id}`"));
                assert!(row.export.is_some(), "{name} is a step of `{id}`, which runs nowhere in the page");
            }
            Shared(id) => assert!(capability(id).is_ok(), "{name}: no row `{id}`"),
            Mechanics(text) | Pending(text) | NeedsRow(text) | Dead(text) => {
                assert!(!text.is_empty(), "{name} names no reason")
            }
        }
    }
}

#[test]
fn an_unrowed_product_export_fails_the_owners_test() {
    let mut exports = crate_exports();
    exports.insert(
        "deliberatelyUnrowedProduct".into(),
        vec!["gateway_exchange/deliberate.rs".into()],
    );
    let (missing, stale) = unrowed(&exports);
    assert_eq!(missing, ["deliberatelyUnrowedProduct"]);
    assert!(stale.is_empty());
    // A row's owner that disappears from the sources leaves a stale entry.
    let mut gone = crate_exports();
    gone.remove("decodeVectorTool");
    assert_eq!(unrowed(&gone).1, ["decodeVectorTool"]);
}
