//! The host half: public rasters fetched exactly as the engine names them,
//! with byte evidence, and the engine's comparison placed only when its own
//! digest holds. The end-to-end comparison runs against a Network engine
//! that serves elevation_v1 (`DS_NETWORK_ENGINE_BIN`).
use super::*;
use ds_network_contracts::elevation::{RwandaBlockV1, TERRARIUM_ZOOM, TerrariumTileV1};
use flate2::{Compression, write::ZlibEncoder};
use std::io::Write;

struct Fixture {
    index: Vec<u8>,
    block: Vec<u8>,
}

impl Fixture {
    fn new() -> Self {
        let (x, y) = ds_geo_lite::projection::wgs84_to_nix_itrf2005(30.0, -2.0);
        let mut compressor = ZlibEncoder::new(Vec::new(), Compression::default());
        for value in [1600.0_f32, 1601.0, 1602.0, 1603.0] {
            compressor.write_all(&value.to_le_bytes()).unwrap();
        }
        let block = compressor.finish().unwrap();
        let index = serde_json::to_vec(&serde_json::json!({
            "width":2,"height":2,"geotransform":[x-5.0,10.0,0.0,y+5.0,0.0,-10.0],
            "nodata":-9999.0,"block":[2,2],"blocks":[{"i":0,"j":0,"offset":64,"length":block.len()}]
        }))
        .unwrap();
        Self { index, block }
    }
    fn contract(&self) -> IndexContract<'static> {
        IndexContract {
            size: self.index.len(),
            digest: Box::leak(format!("{:x}", Sha256::digest(&self.index)).into_boxed_str()),
            tiff_size: 64 + self.block.len() as u64,
        }
    }
    fn block_v1(&self) -> RwandaBlockV1 {
        RwandaBlockV1 {
            i: 0,
            j: 0,
            offset: 64,
            length: self.block.len() as u64,
        }
    }
}

impl Transport for Fixture {
    fn get(
        &mut self,
        url: &str,
        range: Option<(u64, u64)>,
        _limit: usize,
    ) -> Result<Response, ComparisonError> {
        if url.ends_with("blockindex.json") {
            assert!(range.is_none());
            return Ok(Response {
                status: 200,
                content_range: None,
                body: self.index.clone(),
            });
        }
        if url.ends_with(".tif") {
            assert_eq!(range, Some((64, self.block.len() as u64)));
            let total = 64 + self.block.len() as u64;
            return Ok(Response {
                status: 206,
                content_range: Some(format!("bytes 64-{}/{total}", total - 1)),
                body: self.block.clone(),
            });
        }
        assert!(url.starts_with("https://s3.amazonaws.com/elevation-tiles-prod/terrarium/15/"));
        Ok(Response {
            status: 200,
            content_range: None,
            body: b"png".to_vec(),
        })
    }
}

#[test]
fn rasters_are_fetched_exactly_as_named_with_evidence_and_held_for_the_engine() {
    let fixture = Fixture::new();
    let block = fixture.block_v1();
    let contract = fixture.contract();
    let mut rasters = PublicRasters::with(fixture, contract).unwrap();
    // Nothing is fetched until a job asks.
    assert!(rasters.held().rwanda_index.is_none());
    assert!(rasters.evidence().is_empty());
    rasters
        .acquire(&Needed {
            rwanda_index: true,
            rwanda_blocks: vec![block],
            terrarium_tiles: vec![TerrariumTileV1 {
                z: TERRARIUM_ZOOM,
                x: 3,
                y: 4,
            }],
        })
        .unwrap();
    let held = rasters.held();
    assert!(held.rwanda_index.as_ref().unwrap().is_file());
    assert!(
        held.rwanda_blocks
            .as_ref()
            .unwrap()
            .join(rwanda_block_file(&block))
            .is_file()
    );
    assert!(
        held.terrarium_tiles
            .as_ref()
            .unwrap()
            .join("terrarium-15-3-4.png")
            .is_file()
    );
    assert_eq!(rasters.evidence().len(), 3);
    assert!(rasters.evidence().iter().any(|fetch| fetch.range.is_some()));
    assert!(
        rasters
            .evidence()
            .iter()
            .all(|fetch| fetch.sha256.starts_with("sha256:"))
    );
}

#[test]
fn an_index_that_is_not_the_pinned_one_refuses_and_holds_nothing() {
    let fixture = Fixture::new();
    let mut contract = fixture.contract();
    contract.digest = "incorrect";
    let mut rasters = PublicRasters::with(fixture, contract).unwrap();
    assert_eq!(
        rasters.ensure_index().unwrap_err().code(),
        "elevation_integrity_failed"
    );
    assert!(rasters.held().rwanda_index.is_none());
}

#[test]
fn range_status_header_length_and_total_are_exact() {
    let valid = || Response {
        status: 206,
        content_range: Some("bytes 10-13/100".into()),
        body: vec![1; 4],
    };
    assert!(validate_range(&valid(), 10, 4, 100).is_ok());
    let mut response = valid();
    response.status = 200;
    assert!(validate_range(&response, 10, 4, 100).is_err());
    let mut response = valid();
    response.content_range = Some("bytes 10-13/101".into());
    assert!(validate_range(&response, 10, 4, 100).is_err());
    let mut response = valid();
    response.body.pop();
    assert!(validate_range(&response, 10, 4, 100).is_err());
    assert!(validate_range(&valid(), u64::MAX, 4, 100).is_err());
    assert!(validate_range(&valid(), 10, 0, 100).is_err());
}

#[test]
fn engine_refusals_keep_the_comparison_vocabulary() {
    assert_eq!(
        engine_error(NetworkFailure::acquisition("x")).code(),
        "elevation_integrity_failed"
    );
}

#[test]
#[ignore = "requires a Network engine serving elevation_v1 (DS_NETWORK_ENGINE_BIN)"]
fn the_engine_compares_both_sources_and_the_host_places_its_csv() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("input.csv");
    let out = dir.path().join("output.csv");
    fs::write(
        &source,
        "id,alignment,station_m,x,y\na,route1,0,30,-2\nb,route2,10,30.1,-2.1\n",
    )
    .unwrap();
    let fixture = Fixture::new();
    let contract = fixture.contract();
    let mut rasters = PublicRasters::with(fixture, contract).unwrap();
    rasters.ensure_index().unwrap();
    let client = capture_engine(None).unwrap();
    let error = compare_with(
        &client,
        &mut rasters,
        &source,
        &out,
        &ComparisonOptions::default(),
    )
    .unwrap_err();
    // The fixture's tile is not a PNG: the engine refuses and nothing is placed.
    assert_eq!(error.code(), "elevation_integrity_failed");
    assert!(!out.exists());
}

/// The fixture behind a link that stalls `failures` times on block reads.
struct Stalling {
    inner: Fixture,
    failures: u32,
    calls: u32,
}

impl Transport for Stalling {
    fn get(
        &mut self,
        url: &str,
        range: Option<(u64, u64)>,
        limit: usize,
    ) -> Result<Response, ComparisonError> {
        if url.ends_with(".tif") {
            self.calls += 1;
            if self.calls <= self.failures {
                return Err(ComparisonError::Io("timeout: global".into()));
            }
        }
        self.inner.get(url, range, limit)
    }
}

fn stalled_block_read(failures: u32) -> (Result<(), ComparisonError>, usize) {
    let fixture = Fixture::new();
    let block = fixture.block_v1();
    let contract = fixture.contract();
    let mut rasters = PublicRasters::with(
        Stalling {
            inner: fixture,
            failures,
            calls: 0,
        },
        contract,
    )
    .unwrap();
    let result = rasters.acquire(&Needed {
        rwanda_index: true,
        rwanda_blocks: vec![block],
        terrarium_tiles: vec![],
    });
    (result, rasters.evidence().len())
}

#[test]
fn a_stalled_block_read_is_retried_and_a_persistent_one_still_fails() {
    // Two stalls, then the bytes: the read lands once, with one evidence row.
    let (recovered, evidence) = stalled_block_read(FETCH_ATTEMPTS - 1);
    recovered.unwrap();
    assert_eq!(evidence, 2, "the index and the block, each recorded once");
    // Control: a link that never answers fails with the transport's error.
    let (failed, _) = stalled_block_read(FETCH_ATTEMPTS);
    assert!(matches!(failed, Err(ComparisonError::Io(message)) if message.contains("timeout")));
}
