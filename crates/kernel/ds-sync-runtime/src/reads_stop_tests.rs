//! A stopping host abandons the object it is reading: nothing unverified is
//! placed and no partial file is left behind for a later pass to trust.
use super::*;

/// A body that asks the host to stop once its first slice has been read.
struct StopsAfterFirstSlice<'a> {
    bytes: &'a [u8],
    offset: usize,
    stop: &'a AtomicBool,
}

impl Read for StopsAfterFirstSlice<'_> {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        let count = buffer.len().min(4).min(self.bytes.len() - self.offset);
        buffer[..count].copy_from_slice(&self.bytes[self.offset..self.offset + count]);
        self.offset += count;
        self.stop.store(true, Ordering::Release);
        Ok(count)
    }
}

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

#[test]
fn a_stop_mid_object_places_nothing_and_leaves_no_partial_file() {
    let dir = crate::test_directory::tempdir().unwrap();
    let destination = dir.path().join("plot-map");
    let bytes = b"a plot map of several slices".to_vec();
    let stop = AtomicBool::new(false);
    let body = Stoppable {
        reader: StopsAfterFirstSlice {
            bytes: &bytes,
            offset: 0,
            stop: &stop,
        },
        stop: &stop,
    };
    let outcome = place_verified(body, &destination, &digest(&bytes), bytes.len() as u64);
    assert_eq!(
        outcome.unwrap_err(),
        "the download stopped: other error",
        "the read after the stop ends the object"
    );
    assert!(!destination.exists());
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    assert!(!holds_output(&destination, &digest(&bytes), bytes.len() as u64).unwrap());
}

#[test]
fn without_a_stop_the_same_body_is_placed_and_then_held() {
    let dir = crate::test_directory::tempdir().unwrap();
    let destination = dir.path().join("plot-map");
    let bytes = b"a plot map of several slices".to_vec();
    let never = AtomicBool::new(false);
    let observed = AtomicBool::new(false);
    let body = Stoppable {
        reader: StopsAfterFirstSlice {
            bytes: &bytes,
            offset: 0,
            stop: &observed,
        },
        stop: &never,
    };
    place_verified(body, &destination, &digest(&bytes), bytes.len() as u64).unwrap();
    assert_eq!(std::fs::read(&destination).unwrap(), bytes);
    assert!(holds_output(&destination, &digest(&bytes), bytes.len() as u64).unwrap());
    // The held check reads the bytes again: other bytes of the same size are
    // not these.
    let other = b"A plot map of several slices";
    assert!(!holds_output(&destination, &digest(other), bytes.len() as u64).unwrap());
}
