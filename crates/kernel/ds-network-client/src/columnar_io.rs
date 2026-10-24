//! GeoParquet and Arrow IPC calls: the host stages exact bytes it holds, the
//! captured engine encodes or decodes them through the columnar format
//! contract's one converter, and only its complete, digest-bound reply is
//! admitted here. Hosts keep artifact files, addresses, digests and every
//! placement of what comes back; Core never builds or reads a column.
use super::*;
use ds_network_contracts::{
    columnar_io::{
        FeaturesIpcParamsV1, FeaturesIpcResultV1, GeoparquetEncodeParamsV1,
        GeoparquetEncodeResultV1, GeoparquetReadParamsV1, GeoparquetReadResultV1,
        GeoparquetStreamParamsV1, GeoparquetStreamResultV1, GeoparquetSummaryV1, GeoparquetViewV1,
        ARROW_IPC_ARTIFACT, COLUMNAR_APPENDS_INPUT, COLUMNAR_ARTIFACT_INPUT,
        COLUMNAR_FEATURES_ARTIFACT, COLUMNAR_FEATURES_INPUT, COLUMNAR_LAYERS_INPUT,
        COLUMNAR_REFUSED, GEOPARQUET_ARTIFACT, IPC_CHUNKS_ARTIFACT,
    },
    grid_operation::GridOperationV1,
};
use serde_json::Value;

/// Decoded bytes per features or IPC page. One page restages the held
/// artifact, so pages are large; whole batches keep a page boundary exact.
pub const COLUMNAR_PAGE_BYTES: usize = 64 * 1024 * 1024;

fn reply_invalid(message: &str) -> NetworkFailure {
    NetworkFailure::new("network_result_invalid", message)
}

/// One page of IPC chunks out of a held artifact.
#[derive(Debug)]
pub struct IpcPage {
    pub summary: GeoparquetSummaryV1,
    /// `(rows, Arrow IPC file bytes)` in artifact order.
    pub chunks: Vec<(usize, Vec<u8>)>,
    pub next_row: Option<usize>,
}

impl NetworkFailure {
    /// The converter refused the bytes themselves (its message is the
    /// converter's).
    pub fn is_columnar_refusal(&self) -> bool {
        self.owner_code() == Some(COLUMNAR_REFUSED)
    }
}

impl CapturedNetworkClient {
    /// Whether the captured engine encodes and decodes columnar bytes.
    pub fn offers_columnar_io(&self) -> bool {
        self.identity
            .capabilities
            .contains(&ds_network_contracts::engine::NetworkCapability::ColumnarIoV1)
    }

    fn columnar_call(
        &self,
        call: GridOperationV1,
        input: &str,
        bytes: Vec<u8>,
        cancel: &dyn Fn() -> bool,
    ) -> Result<GridOperationOutcome, NetworkFailure> {
        if !self.offers_columnar_io() {
            return Err(NetworkFailure::new(
                "network_engine_update_required",
                "the captured Network engine was released before it encoded and decoded \
                 GeoParquet and Arrow IPC for Core (columnar_io_v1); update the Network engine",
            ));
        }
        let stage = private_disk_stage()?;
        self.grid_operation(
            &call,
            None,
            &[(input.to_owned(), bytes)],
            stage.path(),
            cancel,
        )
    }

    /// One layer of canonical layers (a `ds.network.source-layers/v1`
    /// document's bytes) encoded whole as one GeoParquet artifact.
    pub fn geoparquet_encode(
        &self,
        params: GeoparquetEncodeParamsV1,
        layers: Vec<u8>,
        cancel: &dyn Fn() -> bool,
    ) -> Result<(GeoparquetEncodeResultV1, Vec<u8>), NetworkFailure> {
        let mut outcome = self.columnar_call(
            GridOperationV1::GeoparquetEncode(params),
            COLUMNAR_LAYERS_INPUT,
            layers,
            cancel,
        )?;
        let result: GeoparquetEncodeResultV1 =
            serde_json::from_value(std::mem::take(&mut outcome.result.result))
                .map_err(|_| reply_invalid("GeoParquet encode reply is not the typed result"))?;
        let artifact = outcome
            .take_artifact(GEOPARQUET_ARTIFACT)
            .ok_or_else(|| reply_invalid("GeoParquet encode reply has no artifact"))?;
        if result.byte_count != artifact.len() {
            return Err(reply_invalid(
                "GeoParquet encode reply miscounts its artifact",
            ));
        }
        Ok((result, artifact))
    }

    /// One append log (one `GeoparquetAppendV1` per line) replayed through
    /// the streamed writer.
    pub fn geoparquet_stream(
        &self,
        params: GeoparquetStreamParamsV1,
        appends: Vec<u8>,
        cancel: &dyn Fn() -> bool,
    ) -> Result<(GeoparquetStreamResultV1, Vec<u8>), NetworkFailure> {
        let layer_name = params.layer_name.clone();
        let mut outcome = self.columnar_call(
            GridOperationV1::GeoparquetStream(params),
            COLUMNAR_APPENDS_INPUT,
            appends,
            cancel,
        )?;
        let result: GeoparquetStreamResultV1 =
            serde_json::from_value(std::mem::take(&mut outcome.result.result))
                .map_err(|_| reply_invalid("GeoParquet stream reply is not the typed result"))?;
        let artifact = outcome
            .take_artifact(GEOPARQUET_ARTIFACT)
            .ok_or_else(|| reply_invalid("GeoParquet stream reply has no artifact"))?;
        if result.byte_count != artifact.len() || result.layer_name != layer_name {
            return Err(reply_invalid(
                "GeoParquet stream reply miscounts its artifact or names another layer",
            ));
        }
        Ok((result, artifact))
    }

    fn geoparquet_read(
        &self,
        params: GeoparquetReadParamsV1,
        artifact: &[u8],
        cancel: &dyn Fn() -> bool,
    ) -> Result<(GeoparquetReadResultV1, GridOperationOutcome), NetworkFailure> {
        let outcome = self.columnar_call(
            GridOperationV1::GeoparquetRead(params),
            COLUMNAR_ARTIFACT_INPUT,
            artifact.to_vec(),
            cancel,
        )?;
        let result: GeoparquetReadResultV1 = serde_json::from_value(outcome.result.result.clone())
            .map_err(|_| reply_invalid("GeoParquet read reply is not the typed result"))?;
        if result.summary.byte_count != artifact.len()
            || result
                .next_row
                .is_some_and(|row| row >= result.summary.feature_count)
        {
            return Err(reply_invalid(
                "GeoParquet read reply describes another artifact",
            ));
        }
        Ok((result, outcome))
    }

    /// What a held artifact says about itself.
    pub fn geoparquet_summary(
        &self,
        name: &str,
        artifact: &[u8],
        cancel: &dyn Fn() -> bool,
    ) -> Result<GeoparquetSummaryV1, NetworkFailure> {
        let params = GeoparquetReadParamsV1 {
            name: name.to_owned(),
            batch_rows: 1,
            view: GeoparquetViewV1::Summary,
        };
        Ok(self.geoparquet_read(params, artifact, cancel)?.0.summary)
    }

    /// Every feature of a held artifact, in artifact order, decoded in
    /// `batch_rows` batches and handed to `visit` one at a time. A visitor
    /// that breaks stops the read: the answer is then `None`.
    pub fn geoparquet_features(
        &self,
        name: &str,
        artifact: &[u8],
        batch_rows: usize,
        max_geometry_nodes: usize,
        cancel: &dyn Fn() -> bool,
        visit: &mut dyn FnMut(Value) -> std::ops::ControlFlow<()>,
    ) -> Result<Option<GeoparquetSummaryV1>, NetworkFailure> {
        let mut start_row = 0;
        let mut seen = 0usize;
        loop {
            let params = GeoparquetReadParamsV1 {
                name: name.to_owned(),
                batch_rows,
                view: GeoparquetViewV1::Features {
                    start_row,
                    max_geometry_nodes,
                    max_bytes: COLUMNAR_PAGE_BYTES,
                },
            };
            let (result, mut outcome) = self.geoparquet_read(params, artifact, cancel)?;
            let page = outcome
                .take_artifact(COLUMNAR_FEATURES_ARTIFACT)
                .ok_or_else(|| reply_invalid("GeoParquet features reply has no features"))?;
            let mut count = 0usize;
            for line in page.split(|byte| *byte == b'\n') {
                if line.is_empty() {
                    continue;
                }
                let feature: Value = serde_json::from_slice(line)
                    .map_err(|_| reply_invalid("GeoParquet features reply is not GeoJSON lines"))?;
                count += 1;
                if visit(feature).is_break() {
                    return Ok(None);
                }
            }
            seen = seen.saturating_add(count);
            if count != result.feature_rows || seen > result.summary.feature_count {
                return Err(reply_invalid(
                    "GeoParquet features reply miscounts its rows",
                ));
            }
            match result.next_row {
                Some(next) if next > start_row && next == seen => start_row = next,
                Some(_) => return Err(reply_invalid("GeoParquet features reply does not advance")),
                None if seen == result.summary.feature_count => return Ok(Some(result.summary)),
                None => return Err(reply_invalid("GeoParquet features reply ended early")),
            }
        }
    }

    /// At most `limit` rows from `offset`: property maps, or GeoJSON features
    /// when `geometry` is set. Returns the summary, rows and next offset.
    pub fn geoparquet_rows(
        &self,
        name: &str,
        artifact: &[u8],
        offset: usize,
        limit: usize,
        geometry: bool,
        cancel: &dyn Fn() -> bool,
    ) -> Result<GeoparquetReadResultV1, NetworkFailure> {
        let params = GeoparquetReadParamsV1 {
            name: name.to_owned(),
            batch_rows: 5000,
            view: GeoparquetViewV1::Rows {
                offset,
                limit,
                geometry,
            },
        };
        let (result, _) = self.geoparquet_read(params, artifact, cancel)?;
        if result.rows.len() > limit {
            return Err(reply_invalid("GeoParquet rows reply exceeds its limit"));
        }
        Ok(result)
    }

    /// One page of browser-layout IPC chunks of `batch_rows` rows from
    /// `start_row` (a multiple of `batch_rows`, or 0).
    pub fn geoparquet_ipc_page(
        &self,
        name: &str,
        artifact: &[u8],
        batch_rows: usize,
        start_row: usize,
        cancel: &dyn Fn() -> bool,
    ) -> Result<IpcPage, NetworkFailure> {
        let params = GeoparquetReadParamsV1 {
            name: name.to_owned(),
            batch_rows,
            view: GeoparquetViewV1::Ipc {
                start_row,
                max_bytes: COLUMNAR_PAGE_BYTES,
            },
        };
        let (result, mut outcome) = self.geoparquet_read(params, artifact, cancel)?;
        let bytes = outcome
            .take_artifact(IPC_CHUNKS_ARTIFACT)
            .ok_or_else(|| reply_invalid("GeoParquet IPC reply has no chunks"))?;
        let mut chunks = Vec::with_capacity(result.chunks.len());
        let mut at = 0usize;
        let mut rows = 0usize;
        for chunk in &result.chunks {
            let end = at
                .checked_add(chunk.byte_count)
                .filter(|end| *end <= bytes.len())
                .ok_or_else(|| reply_invalid("GeoParquet IPC reply miscounts its chunks"))?;
            chunks.push((chunk.rows, bytes[at..end].to_vec()));
            rows = rows.saturating_add(chunk.rows);
            at = end;
        }
        let end = start_row.saturating_add(rows);
        if at != bytes.len()
            || end > result.summary.feature_count
            || result.next_row.is_some_and(|next| next != end || rows == 0)
            || (result.next_row.is_none() && end != result.summary.feature_count)
        {
            return Err(reply_invalid("GeoParquet IPC reply miscounts its chunks"));
        }
        Ok(IpcPage {
            summary: result.summary,
            chunks,
            next_row: result.next_row,
        })
    }

    /// Features as one Arrow IPC file in the browser working-set layout.
    pub fn features_ipc(
        &self,
        features: &[Value],
        columns: &[String],
        cancel: &dyn Fn() -> bool,
    ) -> Result<Vec<u8>, NetworkFailure> {
        let staged = serde_json::to_vec(features)
            .map_err(|e| NetworkFailure::new("network_request_invalid", e.to_string()))?;
        let mut outcome = self.columnar_call(
            GridOperationV1::FeaturesIpc(FeaturesIpcParamsV1 {
                columns: columns.to_vec(),
            }),
            COLUMNAR_FEATURES_INPUT,
            staged,
            cancel,
        )?;
        let result: FeaturesIpcResultV1 =
            serde_json::from_value(std::mem::take(&mut outcome.result.result))
                .map_err(|_| reply_invalid("Arrow IPC reply is not the typed result"))?;
        let bytes = outcome
            .take_artifact(ARROW_IPC_ARTIFACT)
            .ok_or_else(|| reply_invalid("Arrow IPC reply has no file"))?;
        if result.rows != features.len() || result.byte_count != bytes.len() {
            return Err(reply_invalid("Arrow IPC reply miscounts its rows or bytes"));
        }
        Ok(bytes)
    }
}
