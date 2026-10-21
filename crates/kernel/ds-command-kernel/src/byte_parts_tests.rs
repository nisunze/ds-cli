use super::*;
use rusqlite::Connection;

const CODE: &str = "test_invalid";
const TABLE: Blobs = Blobs {
    blobs: "test_blobs",
    parts: "test_parts",
    code: CODE,
    content_addressed: false,
};

fn stage() -> Stage<'static> {
    Stage {
        code: CODE,
        owner: "owner",
        store: "test",
    }
}
fn share(byte_length: u64, part: u64, staging: Option<u64>) -> Share {
    Share {
        byte_length,
        part,
        staging,
    }
}
fn database() -> Connection {
    let db = Connection::open_in_memory().unwrap();
    db.execute_batch(&TABLE.schema()).unwrap();
    db
}

#[test]
fn a_payload_over_one_part_takes_effect_only_with_every_part_in_order() {
    let db = database();
    let length = PART_BYTES + 3;
    let whole: Vec<u8> = (0..length).map(|at| (at % 251) as u8).collect();
    let (head, tail) = whole.split_at(PART_BYTES as usize);
    let Received::Staged {
        staging,
        received,
        parts,
    } = stage()
        .receive(&db, "op", share(length, 0, None), Some(head.to_vec()), u64::MAX, 1)
        .unwrap()
    else {
        panic!("the first of two parts is staged");
    };
    assert_eq!((received, parts), (1, 2));
    let error = stage()
        .receive(&db, "another op", share(length, 1, Some(staging)), Some(tail.to_vec()), u64::MAX, 2)
        .err()
        .unwrap();
    assert!(error.starts_with(CODE), "a staging continues only its own operation: {error}");
    assert!(stage()
        .receive(&db, "op", share(length, 1, None), Some(tail.to_vec()), u64::MAX, 2)
        .is_err());
    let Received::Complete(payload) = stage()
        .receive(&db, "op", share(length, 1, Some(staging)), Some(tail.to_vec()), u64::MAX, 2)
        .unwrap()
    else {
        panic!("the last part completes the payload");
    };
    let digest = payload.sha256(&db, CODE).unwrap();
    assert_eq!(digest, sha256(&whole));
    let blob = TABLE.write(&db, "owner", &payload, &digest).unwrap();
    payload.release(&db, CODE).unwrap();
    assert_eq!((blob.parts, blob.byte_length), (2, length));
    assert!(TABLE.intact(&db, &blob).unwrap());
    let across = TABLE.range(&db, &blob, PART_BYTES - 2, 4).unwrap().unwrap();
    assert_eq!(across, whole[(PART_BYTES - 2) as usize..(PART_BYTES + 2) as usize]);
    assert_eq!(TABLE.range_sha256(&db, &blob, 0, length).unwrap().unwrap(), digest);
    let staged: i64 = db
        .query_row("SELECT COUNT(*) FROM byte_staging_parts", [], |row| row.get(0))
        .unwrap();
    assert_eq!(staged, 0, "a kept payload leaves no staging behind");
}

#[test]
fn a_part_carries_exactly_its_share_within_the_bound() {
    let db = database();
    for (share, bytes, why) in [
        (share(4, 0, None), Some(vec![1, 2, 3]), "a short part"),
        (share(11, 0, None), Some(vec![0; 11]), "past the bound"),
        (share(0, 0, None), Some(Vec::new()), "an empty payload"),
        (share(4, 0, None), None, "no bytes"),
        (share(4, 1, None), Some(vec![1, 2, 3, 4]), "a part past the end"),
        (share(4, 0, Some(9)), Some(vec![1, 2, 3, 4]), "a staged one-part payload"),
    ] {
        let refused = stage().receive(&db, "op", share, bytes, 10, 1);
        assert!(refused.is_err(), "{why} is refused");
    }
    assert!(matches!(
        stage()
            .receive(&db, "op", share(4, 0, None), Some(vec![1, 2, 3, 4]), 10, 1)
            .unwrap(),
        Received::Complete(_)
    ));
}

#[test]
fn a_corrupt_part_reads_as_missing_while_lengths_alone_still_agree() {
    let db = database();
    let Received::Complete(payload) = stage()
        .receive(&db, "op", share(4, 0, None), Some(vec![1, 2, 3, 4]), 10, 1)
        .unwrap()
    else {
        panic!("one part is a whole payload");
    };
    let digest = payload.sha256(&db, CODE).unwrap();
    let blob = TABLE.write(&db, "owner", &payload, &digest).unwrap();
    assert_eq!(TABLE.part(&db, &blob, 0).unwrap(), Some(vec![1, 2, 3, 4]));
    db.execute("UPDATE test_parts SET bytes=X'01020309'", []).unwrap();
    assert!(TABLE.intact(&db, &blob).unwrap(), "a presence check reads lengths, not bytes");
    assert_eq!(TABLE.part(&db, &blob, 0).unwrap(), None);
    assert_eq!(TABLE.range(&db, &blob, 1, 2).unwrap(), None);
    assert_eq!(TABLE.range_sha256(&db, &blob, 0, 4).unwrap(), None);
    TABLE.delete(&db, blob.id).unwrap();
    assert_eq!(TABLE.get(&db, blob.id).unwrap(), None);
}
