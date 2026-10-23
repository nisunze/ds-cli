//! Local GIS source calls: the host stages one admitted file's exact bytes,
//! the captured engine reads it through the named reader, and only its
//! complete, digest-bound reply is admitted here. Hosts keep the file,
//! its admission and every placement of what comes back.
use super::*;
use ds_network_contracts::{
    grid_operation::GridOperationV1,
    source_io::{
        GeopackageLayersParamsV1, GeopackageLayersV1, LayersGeojsonInputV1, SourceInspectParamsV1,
        SourceInspectResultV1, SourceInspectV1, SourceLayersParamsV1, SourceLayersV1,
        SourceReaderV1, GEOPACKAGE_LAYERS_ARTIFACT, GEOPACKAGE_LAYERS_SCHEMA_V1,
        LAYERS_GEOJSON_ARTIFACT, LAYERS_INPUT, SOURCE_INPUT, SOURCE_LAYERS_ARTIFACT,
        SOURCE_LAYERS_SCHEMA_V1,
    },
};

/// The owner code a reader's own refusal carries; its message is the
/// reader's words, unchanged.
pub const SOURCE_UNREADABLE: &str = "source_unreadable";

fn reply_invalid(message: &str) -> NetworkFailure {
    NetworkFailure::new("network_result_invalid", message)
}

impl NetworkFailure {
    /// The reader refused the source itself (its message is the reader's).
    pub fn is_source_refusal(&self) -> bool {
        self.owner_code() == Some(SOURCE_UNREADABLE)
    }
}

impl CapturedNetworkClient {
    fn source_call(
        &self,
        call: GridOperationV1,
        input: &str,
        bytes: Vec<u8>,
        cancel: &dyn Fn() -> bool,
    ) -> Result<GridOperationOutcome, NetworkFailure> {
        let stage = private_disk_stage()?;
        self.grid_operation(
            &call,
            None,
            &[(input.to_owned(), bytes)],
            stage.path(),
            cancel,
        )
    }

    /// A table's sheets, or the layer summaries of a source that carries
    /// its own geometry.
    pub fn source_inspect(
        &self,
        name: &str,
        bytes: Vec<u8>,
        inspect: SourceInspectV1,
        cancel: &dyn Fn() -> bool,
    ) -> Result<SourceInspectResultV1, NetworkFailure> {
        let call = GridOperationV1::SourceInspect(SourceInspectParamsV1 {
            name: name.to_owned(),
            inspect,
        });
        let outcome = self.source_call(call, SOURCE_INPUT, bytes, cancel)?;
        serde_json::from_value(outcome.result.result)
            .map_err(|_| reply_invalid("source inspection reply is not the typed result"))
    }

    /// The canonical layers one named reader produces.
    pub fn source_layers(
        &self,
        name: &str,
        bytes: Vec<u8>,
        reader: SourceReaderV1,
        cancel: &dyn Fn() -> bool,
    ) -> Result<SourceLayersV1, NetworkFailure> {
        let call = GridOperationV1::SourceLayers(SourceLayersParamsV1 {
            name: name.to_owned(),
            reader,
        });
        let mut outcome = self.source_call(call, SOURCE_INPUT, bytes, cancel)?;
        let artifact = outcome
            .take_artifact(SOURCE_LAYERS_ARTIFACT)
            .ok_or_else(|| reply_invalid("source layers reply has no layers artifact"))?;
        let layers: SourceLayersV1 = serde_json::from_slice(&artifact)
            .map_err(|_| reply_invalid("source layers artifact is not the typed document"))?;
        if !same_family(&layers.schema, SOURCE_LAYERS_SCHEMA_V1) {
            return Err(reply_invalid("source layers artifact names another schema"));
        }
        Ok(layers)
    }

    /// Named feature tables of one closed GeoPackage the host admitted.
    pub fn geopackage_layers(
        &self,
        bytes: Vec<u8>,
        requested: &[String],
        max_rows: usize,
        max_bytes: usize,
        cancel: &dyn Fn() -> bool,
    ) -> Result<GeopackageLayersV1, NetworkFailure> {
        let call = GridOperationV1::GeopackageLayers(GeopackageLayersParamsV1 {
            requested: requested.to_vec(),
            max_rows,
            max_bytes,
        });
        let source_sha256 = digest_bytes(&bytes);
        let mut outcome = self.source_call(call, SOURCE_INPUT, bytes, cancel)?;
        let artifact = outcome
            .take_artifact(GEOPACKAGE_LAYERS_ARTIFACT)
            .ok_or_else(|| reply_invalid("GeoPackage reply has no tables artifact"))?;
        let tables: GeopackageLayersV1 = serde_json::from_slice(&artifact)
            .map_err(|_| reply_invalid("GeoPackage artifact is not the typed document"))?;
        if !same_family(&tables.schema, GEOPACKAGE_LAYERS_SCHEMA_V1)
            || tables.source_sha256 != source_sha256
            || tables.layers.keys().ne(tables.feature_counts.keys())
            || requested
                .iter()
                .any(|name| !tables.layers.contains_key(name))
        {
            return Err(reply_invalid(
                "GeoPackage tables address another source or other tables",
            ));
        }
        Ok(tables)
    }

    /// The shared GeoJSON text for held layers.
    pub fn layers_geojson(
        &self,
        layers: &LayersGeojsonInputV1,
        cancel: &dyn Fn() -> bool,
    ) -> Result<Vec<u8>, NetworkFailure> {
        let bytes = serde_json::to_vec(layers)
            .map_err(|e| NetworkFailure::new("network_request_invalid", e.to_string()))?;
        let mut outcome =
            self.source_call(GridOperationV1::LayersGeojson, LAYERS_INPUT, bytes, cancel)?;
        outcome
            .take_artifact(LAYERS_GEOJSON_ARTIFACT)
            .ok_or_else(|| reply_invalid("GeoJSON reply has no text artifact"))
    }
}
