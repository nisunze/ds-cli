//! ZIP containers (§3.4): central-directory listing, exact-member extraction
//! into memory, shapefile companion resolution. Never a write to disk.
//!
//! ## What a walk reads
//!
//! [`walk`] reads the archive's **central directory** and nothing else: one
//! row per entry with its name and its declared uncompressed size. No member
//! is inflated, nothing is unpacked, and no byte leaves memory — the two
//! tests `assets_zip_walk_no_disk_write` (this file's own text carries no
//! filesystem call) and `assets_zip_walk_never_inflates` (a listing of an
//! archive whose payload bytes are corrupt still succeeds) pin both halves.
//!
//! ## Bounds — refusals, never a silent short list
//!
//! * [`MAX_CONTAINER_MEMBERS`] rows are listed. Every further member is still
//!   counted in `member_count_total`, and the listing says `truncated: true`.
//! * A member deeper than [`MAX_CONTAINER_DEPTH`] path segments is counted and
//!   left out of the listing, again as `truncated: true`.
//! * One nested archive level is supported: an archive *inside* this archive is
//!   listed as `kind: pack`, `nested: true`, and is not walked here. The host
//!   walks that one level deliberately — [`extract_member`] on the nested row,
//!   then [`walk`] on the bytes it gets back — so nothing recurses on its own
//!   and a zip quine cannot cost more than the caller asked for.
//!
//! ## Incomplete shapefiles
//!
//! [`ContainerMember`] has no `reason` field and [`Container`] has no `notes`
//! field, so incompleteness travels as a convention this module owns and
//! `preview/geo.rs` reads: **a `shp` row whose `companions` list is empty is
//! an incomplete shapefile.** It has neither a `.dbf` nor a `.shx` beside it,
//! and §3.4 reports it rather than half-previewing it. Its `.prj` / `.cpg`
//! siblings, if any, stay in the listing as rows of their own instead of
//! being folded into a shapefile that cannot be read.

use std::collections::BTreeMap;
use std::io::{Cursor, Read};

use super::{
    check_bytes, describe_bytes, format, thousands, Container, ContainerMember, Format, Kind,
    MAX_CONTAINER_DEPTH, MAX_CONTAINER_MEMBERS,
};

/// The shapefile sidecars, in the order a listing shows them. The first two
/// are the ones a shapefile cannot be read without (§3.4).
const SHAPEFILE_COMPANIONS: [&str; 4] = ["dbf", "shx", "prj", "cpg"];

/// How many of [`SHAPEFILE_COMPANIONS`] count as required evidence that the
/// shapefile is complete: `.dbf` and `.shx`.
const SHAPEFILE_REQUIRED: usize = 2;

/// Lists the central directory of `bytes` (no member is inflated). Bounds
/// `MAX_CONTAINER_MEMBERS`, `MAX_CONTAINER_DEPTH` and
/// one nested archive level report `truncated: true`, never a short list.
pub fn walk(bytes: &[u8]) -> Result<Container, String> {
    // A message is a container too (correspondence.md §Asset): its MIME
    // parts are the members. Bytes that are not a ZIP are tried as one before
    // the zip refusal, so a host that asked for the default walk of a `.eml`
    // gets its parts rather than "not a zip".
    if !looks_like_zip(bytes) && super::mail_container::is_message(bytes) {
        return super::mail_container::walk(bytes);
    }
    let mut archive = open(bytes)?;
    let mut container = Container::new(Format::Zip);
    let mut members: Vec<ContainerMember> = Vec::new();
    let mut counted: u64 = 0;
    let mut truncated = false;

    for index in 0..archive.len() {
        // The name comes from the central directory alone; no seek, no read.
        let Some(path) = archive.name_for_index(index).map(str::to_string) else {
            continue;
        };
        if is_folder_entry(&path) {
            continue;
        }
        counted = counted.saturating_add(1);
        if depth_of(&path) > MAX_CONTAINER_DEPTH || members.len() >= MAX_CONTAINER_MEMBERS {
            truncated = true;
            continue;
        }
        // `by_index_raw` reads the entry's header for its declared size; it
        // never decompresses, which is the whole point of a central-directory
        // walk (§3.4).
        let declared = archive
            .by_index_raw(index)
            .map(|entry| entry.size())
            .map_err(|error| format!("the container's entry {path} could not be read: {error}"))?;
        members.push(member_row(&path, declared));
    }

    resolve_companions(&mut members);
    container.members = members;
    container.member_count_total = counted;
    container.truncated = truncated;
    Ok(container)
}

/// Inflates exactly one member into memory, refusing above `max_bytes` by
/// name (declared size and inflated size both count).
pub fn extract_member(bytes: &[u8], path: &str, max_bytes: u64) -> Result<Vec<u8>, String> {
    if !looks_like_zip(bytes) && super::mail_container::is_message(bytes) {
        return super::mail_container::extract_part(bytes, path, max_bytes);
    }
    let mut archive = open(bytes)?;
    let index = archive
        .index_for_name(path)
        .or_else(|| archive.index_for_name(path.trim_start_matches("./")))
        .ok_or_else(|| format!("the container has no member named {path}"))?;

    let declared = {
        let entry = archive
            .by_index_raw(index)
            .map_err(|error| format!("the container's member {path} could not be read: {error}"))?;
        if entry.is_dir() {
            return Err(format!(
                "{path} is a folder inside the container, not a file to preview"
            ));
        }
        entry.size()
    };
    // The declared size is the archiver's claim, checked first because it is
    // free; the bounded read below is what a lying central directory meets.
    check_bytes(
        &format!("container member {path}"),
        declared,
        "member",
        max_bytes,
    )?;

    let mut member = archive
        .by_index(index)
        .map_err(|error| format!("the container's member {path} could not be inflated: {error}"))?;
    // The buffer grows with what is actually inflated: a declared size is an
    // untrusted number and is never turned into an allocation.
    let mut extracted = Vec::new();
    // One byte past the bound is read on purpose: it is how a zip bomb whose
    // header understates it is caught without inflating the rest of it.
    member
        .by_ref()
        .take(max_bytes.saturating_add(1))
        .read_to_end(&mut extracted)
        .map_err(|error| format!("the container's member {path} could not be inflated: {error}"))?;
    if extracted.len() as u64 > max_bytes {
        return Err(format!(
            "container member {path} inflates past {} bytes; the member bound is {}",
            thousands(max_bytes),
            describe_bytes(max_bytes)
        ));
    }
    Ok(extracted)
}

/// Folds `.dbf` / `.shx` / `.prj` / `.cpg` rows into their `.shp` row's
/// `companions`; a `.shp` without `.dbf` and `.shx` is reported incomplete.
pub fn resolve_companions(members: &mut Vec<ContainerMember>) {
    let mut shapefiles: BTreeMap<String, usize> = BTreeMap::new();
    for (index, member) in members.iter().enumerate() {
        if member.format == Format::Shp {
            shapefiles.entry(stem_key(&member.path)).or_insert(index);
        }
    }
    if shapefiles.is_empty() {
        return;
    }

    // (rank in SHAPEFILE_COMPANIONS, member index) per shapefile row.
    let mut found: Vec<Vec<(usize, usize)>> = vec![Vec::new(); members.len()];
    for (index, member) in members.iter().enumerate() {
        let Some(rank) = companion_rank(&member.path) else {
            continue;
        };
        let Some(owner) = shapefiles.get(&stem_key(&member.path)) else {
            continue;
        };
        if let Some(list) = found.get_mut(*owner) {
            list.push((rank, index));
        }
    }

    let mut folded = vec![false; members.len()];
    for (owner, mut list) in found.into_iter().enumerate() {
        // A shapefile with neither `.dbf` nor `.shx` is incomplete: its
        // companions stay empty (the convention this module documents) and
        // its `.prj` / `.cpg` siblings keep their own rows.
        if !list.iter().any(|(rank, _)| *rank < SHAPEFILE_REQUIRED) {
            continue;
        }
        list.sort_unstable();
        let mut paths = Vec::with_capacity(list.len());
        for (_, index) in &list {
            if let Some(member) = members.get(*index) {
                paths.push(member.path.clone());
            }
            if let Some(flag) = folded.get_mut(*index) {
                *flag = true;
            }
        }
        if let Some(member) = members.get_mut(owner) {
            member.companions = paths;
        }
    }

    let mut index = 0;
    members.retain(|_| {
        let keep = !folded.get(index).copied().unwrap_or(false);
        index += 1;
        keep
    });
}

/// Reads the central directory of `bytes`, naming what is wrong when it is
/// not a zip at all.
/// The local-file or end-of-central-directory signature a ZIP begins with.
fn looks_like_zip(bytes: &[u8]) -> bool {
    bytes.starts_with(b"PK\x03\x04")
        || bytes.starts_with(b"PK\x05\x06")
        || bytes.starts_with(b"PK\x07\x08")
}

fn open(bytes: &[u8]) -> Result<zip::ZipArchive<Cursor<&[u8]>>, String> {
    zip::ZipArchive::new(Cursor::new(bytes))
        .map_err(|error| format!("the container is not a readable zip archive: {error}"))
}

/// One listing row: format from the member's name, `pack` members marked
/// `nested` and left unwalked.
fn member_row(path: &str, bytes: u64) -> ContainerMember {
    let format = member_format(path, bytes);
    let kind = format::kind_of(format);
    ContainerMember {
        path: path.to_string(),
        bytes,
        kind,
        format,
        companions: Vec::new(),
        nested: kind == Kind::Pack,
    }
}

/// Recognition for a member the walk has deliberately not read.
///
/// `format::recognise` is given the member's name and no bytes, because
/// inflating a member to sniff its magic is exactly what §3.4 forbids. When
/// name-only evidence leaves it `unknown`, [`suffix_format`] maps the file
/// suffix onto the closed `Format` vocabulary so a listing still says `pdf`
/// rather than `unknown` for `report.pdf`.
fn member_format(path: &str, bytes: u64) -> Format {
    let recognised = format::recognise(Some(path), &[], bytes).format;
    if recognised != Format::Unknown {
        return recognised;
    }
    suffix_format(path)
}

/// The closed `Format` vocabulary read as file suffixes (its wire spellings
/// are the suffixes), plus the three spellings that differ from their format
/// name. No table of its own: `Format::ALL` is the table.
fn suffix_format(path: &str) -> Format {
    let name = file_name(path).to_ascii_lowercase();
    for format in Format::ALL {
        if *format == Format::Unknown {
            continue;
        }
        if name.ends_with(&format!(".{}", format.as_str())) {
            return *format;
        }
    }
    match name.rsplit('.').next().unwrap_or_default() {
        "jpeg" => Format::Jpg,
        "tiff" => Format::Tif,
        "tgz" => Format::TarGz,
        _ => Format::Unknown,
    }
}

/// The lower-cased path without the file name's final suffix — the key a
/// shapefile and its companions share. A dot in a folder name is not a
/// suffix, so the cut is looked for after the last `/`.
fn stem_key(path: &str) -> String {
    let lower = path.to_ascii_lowercase();
    let cut = lower.rfind('/').map_or(0, |index| index + 1);
    let (folder, name) = lower.split_at(cut);
    match name.rfind('.') {
        Some(dot) if dot > 0 => format!("{folder}{}", &name[..dot]),
        _ => lower,
    }
}

/// Where `path`'s suffix sits in [`SHAPEFILE_COMPANIONS`], if it is one.
fn companion_rank(path: &str) -> Option<usize> {
    let suffix = file_name(path).rsplit_once('.')?.1.to_ascii_lowercase();
    SHAPEFILE_COMPANIONS
        .iter()
        .position(|candidate| *candidate == suffix)
}

fn file_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// Path segments: `Lot3/gis/poles.shp` is three levels deep.
fn depth_of(path: &str) -> usize {
    path.split('/')
        .filter(|segment| !segment.is_empty())
        .count()
}

/// A central-directory entry that names a folder, not a file.
fn is_folder_entry(path: &str) -> bool {
    path.ends_with('/') || path.ends_with('\\')
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assets::MAX_PREVIEW_INPUT_BYTES;
    use std::io::Write;
    use zip::write::{SimpleFileOptions, ZipWriter};

    fn zip_of(entries: &[(&str, &[u8])], stored: bool) -> Vec<u8> {
        let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
        let options = SimpleFileOptions::default().compression_method(if stored {
            zip::CompressionMethod::Stored
        } else {
            zip::CompressionMethod::Deflated
        });
        for (path, bytes) in entries {
            writer.start_file(*path, options).expect("start member");
            writer.write_all(bytes).expect("write member");
        }
        writer.finish().expect("finish archive").into_inner()
    }

    fn paths(container: &Container) -> Vec<String> {
        container
            .members
            .iter()
            .map(|member| member.path.clone())
            .collect()
    }

    fn row<'a>(container: &'a Container, path: &str) -> &'a ContainerMember {
        container
            .members
            .iter()
            .find(|member| member.path == path)
            .unwrap_or_else(|| panic!("{path} is not in the listing: {:?}", paths(container)))
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn assets_zip_walk_no_disk_write() {
        // The needles are assembled from fragments so that this assertion is
        // not itself the thing it forbids.
        let source = include_str!("container.rs");
        for needle in [
            concat!("std", "::fs"),
            concat!("File", "::create"),
            concat!("fs", "::write"),
            concat!(".", "extract("),
            concat!("extract_to", "_directory"),
        ] {
            assert!(
                !source.contains(needle),
                "container.rs must not reach the filesystem, but it contains {needle}"
            );
        }
        assert!(
            source.contains("by_index_raw"),
            "the walk must read the central directory through by_index_raw"
        );

        // And behaviourally: a walk of a real archive returns rows, and the
        // only bytes it produces are the ones the caller asked to extract.
        let bytes = zip_of(&[("Lot3/drawings/site.pdf", b"%PDF-1.7 site")], false);
        let container = walk(&bytes).expect("walk");
        assert!(container.walked);
        assert_eq!(container.schema, crate::assets::CONTAINER_SCHEMA);
        assert_eq!(container.member_count_total, 1);
        assert_eq!(paths(&container), vec!["Lot3/drawings/site.pdf"]);
        assert_eq!(
            row(&container, "Lot3/drawings/site.pdf").format,
            Format::Pdf
        );
        assert_eq!(row(&container, "Lot3/drawings/site.pdf").kind, Kind::Doc);
        assert_eq!(row(&container, "Lot3/drawings/site.pdf").bytes, 13);
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn assets_zip_walk_never_inflates() {
        // A stored member whose payload is overwritten still lists, because
        // the walk never touches the payload; extracting it fails on the CRC.
        let mut bytes = zip_of(&[("notes/plain.txt", b"PAYLOADPAYLOAD")], true);
        let at = bytes
            .windows(14)
            .position(|window| window == b"PAYLOADPAYLOAD")
            .expect("payload is stored verbatim");
        for offset in at..at + 14 {
            if let Some(byte) = bytes.get_mut(offset) {
                *byte = b'!';
            }
        }
        let container = walk(&bytes).expect("a corrupt payload does not stop a listing");
        assert_eq!(paths(&container), vec!["notes/plain.txt"]);
        assert_eq!(row(&container, "notes/plain.txt").bytes, 14);
        assert!(extract_member(&bytes, "notes/plain.txt", 1024).is_err());
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn assets_zip_bounds_truncate() {
        let deep = "l1/l2/l3/l4/l5/l6/l7/l8/l9/deep.txt";
        let mut names: Vec<String> = (0..=MAX_CONTAINER_MEMBERS)
            .map(|index| format!("many/file{index:05}.txt"))
            .collect();
        names.push(deep.to_string());
        let entries: Vec<(&str, &[u8])> = names
            .iter()
            .map(|name| (name.as_str(), b"x".as_slice()))
            .collect();
        let bytes = zip_of(&entries, false);

        let container = walk(&bytes).expect("walk");
        assert!(container.truncated, "past the bounds the listing says so");
        assert_eq!(container.members.len(), MAX_CONTAINER_MEMBERS);
        assert_eq!(
            container.member_count_total,
            MAX_CONTAINER_MEMBERS as u64 + 2,
            "the total counts every member, listed or not"
        );
        assert!(
            !paths(&container).contains(&deep.to_string()),
            "a member {} levels deep is past MAX_CONTAINER_DEPTH {MAX_CONTAINER_DEPTH}",
            depth_of(deep)
        );

        // The depth bound alone truncates a listing that is otherwise tiny.
        let bytes = zip_of(&[("a/b.txt", b"b"), (deep, b"x")], false);
        let container = walk(&bytes).expect("walk");
        assert!(
            container.truncated,
            "a 10-level path is past the depth bound"
        );
        assert_eq!(paths(&container), vec!["a/b.txt"]);
        assert_eq!(container.member_count_total, 2);

        // A container inside every bound is not truncated, and folder entries
        // are not members of it.
        let bytes = zip_of(&[("a/", b""), ("a/b.txt", b"b")], false);
        let container = walk(&bytes).expect("walk");
        assert!(!container.truncated);
        assert_eq!(paths(&container), vec!["a/b.txt"]);
        assert_eq!(container.member_count_total, 1);
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn assets_zip_nested_archive_is_reported_not_walked() {
        let inner = zip_of(&[("inner/one.txt", b"one")], false);
        let outer = zip_of(
            &[("bundle/inner.zip", &inner), ("read.me", b"hello")],
            false,
        );

        let container = walk(&outer).expect("walk");
        let nested = row(&container, "bundle/inner.zip");
        assert_eq!(nested.format, Format::Zip);
        assert_eq!(nested.kind, Kind::Pack);
        assert!(nested.nested, "an archive member is reported, not walked");
        assert!(
            !paths(&container).contains(&"inner/one.txt".to_string()),
            "the walk never descends into a nested archive"
        );
        assert!(!row(&container, "read.me").nested);

        // The host walks that one level deliberately: extract, then walk.
        let extracted = extract_member(&outer, "bundle/inner.zip", MAX_PREVIEW_INPUT_BYTES)
            .expect("extract the nested archive");
        assert_eq!(
            paths(&walk(&extracted).expect("walk the nested archive")),
            vec!["inner/one.txt"]
        );
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn assets_shapefile_companion_resolution() {
        let bytes = zip_of(
            &[
                ("Lot3/gis/poles.shp", b"shp"),
                ("Lot3/gis/poles.dbf", b"dbf"),
                ("Lot3/gis/poles.shx", b"shx"),
                ("Lot3/gis/poles.PRJ", b"prj"),
                ("Lot3/gis/poles.cpg", b"cpg"),
                ("Lot3/gis/other.dbf", b"loose"),
                ("Lot3/drawings/site.pdf", b"%PDF"),
            ],
            false,
        );
        let container = walk(&bytes).expect("walk");
        let shapefile = row(&container, "Lot3/gis/poles.shp");
        assert_eq!(shapefile.kind, Kind::Geo);
        assert_eq!(
            shapefile.companions,
            vec![
                "Lot3/gis/poles.dbf".to_string(),
                "Lot3/gis/poles.shx".to_string(),
                "Lot3/gis/poles.PRJ".to_string(),
                "Lot3/gis/poles.cpg".to_string(),
            ],
            "companions are folded in dbf, shx, prj, cpg order and match case-insensitively"
        );
        let listed = paths(&container);
        assert!(
            !listed.contains(&"Lot3/gis/poles.dbf".to_string()),
            "a folded companion is not a row of its own: {listed:?}"
        );
        assert!(
            listed.contains(&"Lot3/gis/other.dbf".to_string()),
            "a companion with no shapefile keeps its own row"
        );
        assert_eq!(
            container.member_count_total, 7,
            "folding companions never changes the honest member total"
        );

        // Incomplete: a .shp with neither .dbf nor .shx keeps an empty
        // companions list, and its .prj stays a row of its own.
        let bytes = zip_of(&[("gis/lone.shp", b"shp"), ("gis/lone.prj", b"prj")], false);
        let container = walk(&bytes).expect("walk");
        assert!(
            row(&container, "gis/lone.shp").companions.is_empty(),
            "an incomplete shapefile reports no companions"
        );
        assert!(paths(&container).contains(&"gis/lone.prj".to_string()));
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn assets_zip_extract_member_is_bounded() {
        let bytes = zip_of(&[("docs/one.txt", b"one two three four")], false);
        assert_eq!(
            extract_member(&bytes, "docs/one.txt", 1024).expect("extract"),
            b"one two three four"
        );
        assert_eq!(
            extract_member(&bytes, "./docs/one.txt", 1024).expect("extract"),
            b"one two three four"
        );

        let refusal = extract_member(&bytes, "docs/one.txt", 4).expect_err("past the bound");
        assert!(
            refusal.contains("docs/one.txt") && refusal.contains("18") && refusal.contains("4"),
            "a bound names the member and both numbers: {refusal}"
        );

        let missing = extract_member(&bytes, "docs/two.txt", 1024).expect_err("no such member");
        assert!(missing.contains("docs/two.txt"), "{missing}");

        // A zip bomb: 4 MiB of zeroes that deflate to a few hundred bytes is
        // refused by its declared size, and the read never allocates it.
        let bomb = zip_of(&[("bomb.bin", &vec![0u8; 4 * 1024 * 1024])], false);
        assert!(bomb.len() < 64 * 1024, "the fixture really is a bomb");
        let refusal = extract_member(&bomb, "bomb.bin", 8 * 1024).expect_err("past the bound");
        assert!(
            refusal.contains("4,194,304") && refusal.contains("8 KiB"),
            "{refusal}"
        );
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn assets_zip_refuses_malformed_bytes() {
        for bytes in [
            b"".as_slice(),
            b"not a zip at all".as_slice(),
            b"PK\x03\x04 truncated right here".as_slice(),
        ] {
            let refusal = walk(bytes).expect_err("malformed bytes are refused, never a panic");
            assert!(
                refusal.contains("zip archive"),
                "a refusal says what was wrong: {refusal}"
            );
            assert!(extract_member(bytes, "any.txt", 1024).is_err());
        }
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn assets_zip_walk_answers_the_two_slice_request() {
        let bytes = zip_of(
            &[
                ("Lot3/gis/poles.shp", b"shp"),
                ("Lot3/gis/poles.dbf", b"dbf"),
            ],
            false,
        );
        let ask = |value: serde_json::Value| serde_json::to_vec(&value).expect("request");

        let answer = crate::assets::evaluate_with_bytes(
            &bytes,
            &ask(serde_json::json!({
                "schema": crate::assets::REQUEST_SCHEMA,
                "action": "container_walk",
            })),
        )
        .expect("the two-slice walk");
        let answer: serde_json::Value = serde_json::from_str(&answer).expect("json");
        assert_eq!(answer["action"], "container_walk");
        assert_eq!(answer["schema"], crate::assets::CONTAINER_SCHEMA);
        assert_eq!(answer["walked"], true);
        assert_eq!(answer["truncated"], false);
        assert_eq!(answer["member_count_total"], 2);
        assert_eq!(answer["members"][0]["path"], "Lot3/gis/poles.shp");
        assert_eq!(answer["members"][0]["kind"], "geo");
        assert_eq!(answer["members"][0]["companions"][0], "Lot3/gis/poles.dbf");
        assert_eq!(answer["members"][1], serde_json::Value::Null);

        let answer = crate::assets::evaluate_with_bytes(
            &bytes,
            &ask(serde_json::json!({
                "schema": crate::assets::REQUEST_SCHEMA,
                "action": "extract_member",
                "member": "Lot3/gis/poles.dbf",
                "max_bytes": 1024,
            })),
        )
        .expect("the two-slice extraction");
        let answer: serde_json::Value = serde_json::from_str(&answer).expect("json");
        assert_eq!(answer["action"], "extract_member");
        assert_eq!(answer["bytes"], 3);
        assert_eq!(answer["digest"], crate::assets::digest_of(b"dbf"));

        let refusal = crate::assets::evaluate_with_bytes(
            &bytes,
            &ask(serde_json::json!({
                "schema": crate::assets::REQUEST_SCHEMA,
                "action": "extract_member",
                "member": "Lot3/gis/poles.dbf",
                "max_bytes": 1,
            })),
        )
        .expect_err("past the bound");
        assert!(refusal.contains("Lot3/gis/poles.dbf"), "{refusal}");
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn assets_zip_member_names_are_read_not_guessed() {
        assert_eq!(suffix_format("a/b/report.PDF"), Format::Pdf);
        assert_eq!(suffix_format("photo.jpeg"), Format::Jpg);
        assert_eq!(suffix_format("scan.TIFF"), Format::Tif);
        assert_eq!(suffix_format("bundle.tar.gz"), Format::TarGz);
        assert_eq!(suffix_format("archive.7z"), Format::SevenZ);
        assert_eq!(suffix_format("model.dsgrid"), Format::Dsgrid);
        assert_eq!(suffix_format("nightly.bak"), Format::Unknown);
        assert_eq!(suffix_format("README"), Format::Unknown);
        assert_eq!(depth_of("Lot3/gis/poles.shp"), 3);
        assert_eq!(stem_key("Lot3/gis/Poles.SHP"), "lot3/gis/poles");
        assert_eq!(companion_rank("gis/poles.dbf"), Some(0));
        assert_eq!(companion_rank("gis/poles.shp"), None);
        assert!(is_folder_entry("Lot3/gis/"));
    }

    // ── adversarial review (stage 4) ────────────────────────────────────────

    /// Rewrites the uncompressed size every header of `archive` declares for
    /// its single member to `declared`: the archiver's claim, made a lie.
    fn lie_about_size(archive: &mut [u8], declared: u32) {
        let local = archive
            .windows(4)
            .position(|window| window == b"PK\x03\x04")
            .expect("local header");
        let central = archive
            .windows(4)
            .position(|window| window == b"PK\x01\x02")
            .expect("central directory");
        archive[local + 22..local + 26].copy_from_slice(&declared.to_le_bytes());
        archive[central + 24..central + 28].copy_from_slice(&declared.to_le_bytes());
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn assets_review_zip_bomb_with_lying_header_stops_at_the_bound() {
        // 4 MiB of zeroes deflate to a few KiB; every header then claims the
        // member is 0 bytes. The declared-size check is passed by the lie,
        // and the bounded read is what refuses it — after at most bound + 1
        // bytes, never the 4 MiB.
        let mut bomb = zip_of(&[("bomb.bin", &vec![0u8; 4 * 1024 * 1024])], false);
        lie_about_size(&mut bomb, 0);
        let listing = walk(&bomb).expect("a lying central directory still lists");
        assert_eq!(
            row(&listing, "bomb.bin").bytes,
            0,
            "the listing reports the claim as made"
        );
        let refusal = extract_member(&bomb, "bomb.bin", 8 * 1024).expect_err("past the bound");
        assert_eq!(
            refusal,
            "container member bomb.bin inflates past 8,192 bytes; the member bound is 8 KiB"
        );
        // The same lie through the two-slice entry point, and through a
        // preview of the member (its own format cap is the bound).
        let ask = |value: serde_json::Value| serde_json::to_vec(&value).expect("request");
        let refusal = crate::assets::evaluate_with_bytes(
            &bomb,
            &ask(serde_json::json!({
                "schema": crate::assets::REQUEST_SCHEMA, "action": "extract_member",
                "member": "bomb.bin", "max_bytes": 1024
            })),
        )
        .expect_err("past the bound");
        assert!(refusal.contains("1,024 bytes"), "{refusal}");
        let mut bomb = zip_of(&[("notes.txt", &vec![b' '; 4 * 1024 * 1024])], false);
        lie_about_size(&mut bomb, 0);
        let refusal = crate::assets::evaluate_with_bytes(
            &bomb,
            &ask(serde_json::json!({
                "schema": crate::assets::REQUEST_SCHEMA, "action": "preview",
                "format": "txt", "member": "notes.txt"
            })),
        )
        .expect_err("past the txt cap");
        assert!(
            refusal.contains("notes.txt") && refusal.contains("1 MiB"),
            "{refusal}"
        );
        // A header that overstates is refused by its claim before any read.
        let mut small = zip_of(&[("small.txt", b"tiny")], false);
        lie_about_size(&mut small, 40 * 1024 * 1024);
        let refusal = extract_member(&small, "small.txt", 1024).expect_err("past the bound");
        assert!(refusal.contains("41,943,040 bytes"), "{refusal}");
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn assets_review_zip_truncated_at_every_length_never_panics() {
        let bytes = zip_of(
            &[
                ("Lot3/gis/poles.shp", b"shp"),
                ("Lot3/gis/poles.dbf", b"dbf"),
                ("Lot3/notes.txt", b"a few notes"),
            ],
            false,
        );
        assert!(walk(&bytes).is_ok());
        let mut refusals = 0usize;
        for length in 0..bytes.len() {
            let cut = &bytes[..length];
            if walk(cut).is_err() {
                refusals += 1;
            }
            let _ = extract_member(cut, "Lot3/notes.txt", 1024);
            let _ = extract_member(cut, "Lot3/gis/poles.shp", 1024);
        }
        assert!(
            refusals > bytes.len() / 2,
            "a chopped central directory is a refusal, not a listing: {refusals} of {}",
            bytes.len()
        );
        // Bit flips in the central directory: still never a panic.
        for offset in (bytes.len().saturating_sub(120))..bytes.len() {
            let mut flipped = bytes.clone();
            flipped[offset] ^= 0xff;
            let _ = walk(&flipped);
            let _ = extract_member(&flipped, "Lot3/notes.txt", 1024);
        }
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn assets_review_zip_member_paths_are_reported_verbatim_never_used_as_paths() {
        // The writer normalises names, so the hostile ones are written over
        // same-length placeholders after the fact — in the local header and
        // the central directory alike.
        let mut bytes = zip_of(
            &[
                ("aa/bb/evil.txt", b"dotdot"),
                ("xabs/root.txt", b"absolute"),
                ("cwin/x.txt_1", b"drive"),
            ],
            true,
        );
        for (placeholder, hostile) in [
            ("aa/bb/evil.txt", "../../evil.txt"),
            ("xabs/root.txt", "/abs/root.txt"),
            ("cwin/x.txt_1", "C:\\win\\x.txt"),
        ] {
            assert_eq!(placeholder.len(), hostile.len());
            let mut at = 0;
            while let Some(found) = bytes[at..]
                .windows(placeholder.len())
                .position(|window| window == placeholder.as_bytes())
            {
                let start = at + found;
                bytes[start..start + hostile.len()].copy_from_slice(hostile.as_bytes());
                at = start + hostile.len();
            }
        }
        let listing = walk(&bytes).expect("walk");
        assert_eq!(
            paths(&listing),
            vec!["../../evil.txt", "/abs/root.txt", "C:\\win\\x.txt"],
            "member names are reported exactly as the archive spells them"
        );
        assert_eq!(row(&listing, "../../evil.txt").format, Format::Txt);
        assert_eq!(
            extract_member(&bytes, "../../evil.txt", 64).expect("extract by verbatim name"),
            b"dotdot"
        );
        assert_eq!(
            extract_member(&bytes, "/abs/root.txt", 64).expect("extract"),
            b"absolute"
        );
        assert_eq!(
            extract_member(&bytes, "C:\\win\\x.txt", 64).expect("extract"),
            b"drive"
        );
        // Nothing resolves them: a normalised spelling is a different name.
        assert!(extract_member(&bytes, "evil.txt", 64).is_err());
        assert!(extract_member(&bytes, "abs/root.txt", 64).is_err());
    }
}
