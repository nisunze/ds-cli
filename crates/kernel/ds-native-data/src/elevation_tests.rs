//! The host half of attach: exact public ranges, a persisted block cache and
//! no-clobber placement of what the engine wrote.
use super::*;
use crate::engine_host::{validate_range_response, CachedRangeProvider, RangeFetcher};
use ds_network_contracts::elevation::RwandaBlockV1;

#[test]
fn exact_http_range_contract_refuses_full_or_wrong_response() {
    assert!(validate_range_response(206, Some("bytes 10-14/100"), 10, 5, 100).is_ok());
    assert!(validate_range_response(200, Some("bytes 10-14/100"), 10, 5, 100).is_err());
    assert!(validate_range_response(206, Some("bytes 10-15/100"), 10, 5, 100).is_err());
    assert!(validate_range_response(206, None, 10, 5, 100).is_err());
}

struct FakeFetcher {
    calls: usize,
    bytes: Vec<u8>,
}

impl RangeFetcher for FakeFetcher {
    fn fetch(
        &mut self,
        _url: &str,
        _offset: u64,
        _length: u64,
        _total: u64,
    ) -> Result<Vec<u8>, String> {
        self.calls += 1;
        Ok(self.bytes.clone())
    }
}

#[test]
fn exact_compressed_ranges_are_persisted_and_reused_from_disk_cache() {
    let cache = tempfile::tempdir().unwrap();
    let block = RwandaBlockV1 {
        i: 1,
        j: 2,
        offset: 11,
        length: 4,
    };
    let mut provider = CachedRangeProvider {
        url: "https://example.invalid/cog".to_string(),
        total: 100,
        cache: cache.path().to_path_buf(),
        max_bytes: 100,
        max_entries: 4,
        fetcher: FakeFetcher {
            calls: 0,
            bytes: vec![1, 2, 3, 4],
        },
    };
    assert_eq!(provider.read(&block).unwrap(), vec![1, 2, 3, 4]);
    assert_eq!(provider.read(&block).unwrap(), vec![1, 2, 3, 4]);
    assert_eq!(provider.fetcher.calls, 1, "the second read is the cache's");
    // A range the engine names outside the COG is refused before any read.
    let outside = RwandaBlockV1 {
        offset: 98,
        ..block
    };
    assert!(provider.read(&outside).is_err());
    assert_eq!(provider.fetcher.calls, 1);
}

#[test]
fn an_artifact_is_placed_only_with_the_engines_own_digest_and_never_over_a_file() {
    let dir = tempfile::tempdir().unwrap();
    let artifact = dir.path().join("artifact.geojson");
    std::fs::write(&artifact, b"{}").unwrap();
    let (bytes, sha256) = sha256_file(&artifact).unwrap();
    let destination = dir.path().join("out.geojson");
    assert!(place_verified(&artifact, &destination, bytes, &"0".repeat(64)).is_err());
    assert!(!destination.exists());
    std::fs::write(&destination, b"original").unwrap();
    assert!(place_verified(&artifact, &destination, bytes, &sha256).is_err());
    assert_eq!(std::fs::read(&destination).unwrap(), b"original");
    std::fs::remove_file(&destination).unwrap();
    place_verified(&artifact, &destination, bytes, &sha256).unwrap();
    assert_eq!(std::fs::read(&destination).unwrap(), b"{}");
}
