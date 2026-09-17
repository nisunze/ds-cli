//! Portable report packaging. Hosts supply verified media; Rust owns the plan,
//! bounds, presentation projection and deterministic archive bytes.
use base64::{engine::general_purpose::STANDARD, Engine};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    io::{Cursor, Read, Write},
};
use zip::{write::SimpleFileOptions, CompressionMethod, DateTime, ZipWriter};

pub const MAX_IMAGE_BYTES: usize = 16 << 20;
pub const MAX_MEDIA_BYTES: usize = 128 << 20;
const MAX_IMAGES: usize = 512;
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlannedImage {
    pub alt: String,
    pub reference: String,
    pub bundle_path: String,
}
#[derive(Debug, Clone, Serialize)]
pub struct Plan {
    pub canonical: String,
    pub preview: String,
    pub name: String,
    pub images: Vec<PlannedImage>,
}

/// A presentation copy from a verified generated archive; canonical bytes stay intact.
pub struct RenderCopy {
    pub markdown: String,
    pub media: BTreeMap<String, Vec<u8>>,
}

pub fn render_copy(
    archive: &[u8],
    canonical: &str,
    source_digest: &str,
) -> Result<RenderCopy, String> {
    if archive.len() > MAX_MEDIA_BYTES + 40 * 1024 * 1024 {
        return Err("Report archive exceeds its render bound".into());
    }
    let mut archive =
        zip::ZipArchive::new(Cursor::new(archive)).map_err(|_| "Invalid report archive")?;
    let mut read = |name: &str, limit: usize| -> Result<Vec<u8>, String> {
        let file = archive
            .by_name(name)
            .map_err(|_| "Incomplete report archive")?;
        if file.size() > limit as u64 {
            return Err("Report archive entry exceeds its bound".into());
        }
        let mut bytes = Vec::new();
        file.take(limit as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| "Unreadable report archive entry")?;
        if bytes.len() > limit {
            return Err("Report archive entry exceeds its bound".into());
        }
        Ok(bytes)
    };
    let bytes = read(canonical, 16 * 1024 * 1024)?;
    if ds_solar_contracts::digest::sha256_hex(&bytes) != source_digest {
        return Err("Report archive differs from its sealed source".into());
    }
    let source = std::str::from_utf8(&bytes).map_err(|_| "Report source must be UTF-8")?;
    if source.contains("[LLM_START:")
        || source.contains("[LLM_END:")
        || source.contains("[LLM interpretation will be inserted here]")
    {
        return Err("Import a linted reviewed final before producing DOCX or PDF".into());
    }
    let plan = plan(canonical, source)?;
    let mut media = BTreeMap::new();
    let mut resolved = BTreeMap::new();
    let mut total = 0usize;
    for image in &plan.images {
        let bytes = read(&image.bundle_path, MAX_IMAGE_BYTES)?;
        total = total
            .checked_add(bytes.len())
            .ok_or("Report media size overflow")?;
        if bytes.is_empty() || total > MAX_MEDIA_BYTES {
            return Err("Report media exceeds its render bound".into());
        }
        resolved.insert(image.reference.clone(), image.bundle_path.clone());
        media.insert(image.bundle_path.clone(), bytes);
    }
    let expected =
        ds_solar_contracts::report_media::project(source, &resolved).map_err(|e| e.message)?;
    if read(&plan.preview, 16 * 1024 * 1024)? != expected.as_bytes() {
        return Err("Report preview differs from its canonical projection".into());
    }
    let mut names: std::collections::BTreeSet<_> = [
        plan.canonical.clone(),
        plan.preview.clone(),
        "media-manifest.json".into(),
        "README.txt".into(),
    ]
    .into_iter()
    .collect();
    names.extend(plan.images.iter().map(|image| image.bundle_path.clone()));
    let actual: std::collections::BTreeSet<_> = archive.file_names().map(str::to_owned).collect();
    if archive.len() != names.len() || actual != names {
        return Err("Report archive has undeclared entries".into());
    }
    let start = "<!-- LLM_AUTHORING_GUIDE_START -->";
    let end = "<!-- LLM_AUTHORING_GUIDE_END -->";
    let markdown = match (expected.find(start), expected.find(end)) {
        (None, None) => expected,
        (Some(a), Some(b))
            if a == 0
                && b > a
                && expected.matches(start).count() == 1
                && expected.matches(end).count() == 1 =>
        {
            expected[b + end.len()..]
                .trim_start_matches(['\r', '\n'])
                .to_owned()
        }
        _ => return Err("Report authoring guide markers are malformed".into()),
    };
    let markdown = ds_solar_contracts::report_media::presentation_comments(&markdown)
        .map_err(|e| e.message)?;
    Ok(RenderCopy { markdown, media })
}

pub fn media_name(reference: &str, index: usize) -> Result<String, String> {
    if index >= MAX_IMAGES || reference.len() > 8192 {
        return Err("Solar report media selector exceeds its bound".into());
    }
    let base = reference
        .split('?')
        .next()
        .unwrap_or("")
        .split('/')
        .rfind(|p| !p.is_empty())
        .unwrap_or("");
    let mut decoded = Vec::new();
    let mut chars = base.as_bytes().iter().copied();
    let mut valid = true;
    while let Some(c) = chars.next() {
        if c == b'%' {
            let digits = [chars.next(), chars.next()];
            let nibble = |c: Option<u8>| c.and_then(|c| (c as char).to_digit(16));
            if let (Some(a), Some(b)) = (nibble(digits[0]), nibble(digits[1])) {
                decoded.push((a * 16 + b) as u8);
            } else {
                valid = false;
                break;
            }
        } else {
            decoded.push(c);
        }
    }
    let decoded = if valid {
        std::str::from_utf8(&decoded).unwrap_or(base)
    } else {
        base
    };
    let mut safe = String::new();
    for c in decoded.chars() {
        if c.is_ascii_alphanumeric() || "._-".contains(c) {
            safe.push(c);
        } else if !safe.ends_with('-') {
            safe.push('-');
        }
    }
    let safe = safe.trim_matches('-');
    let base = if safe.is_empty() || matches!(safe, "." | "..") {
        format!("figure-{}.bin", index + 1)
    } else {
        safe.to_owned()
    };
    if base.len() > 240 {
        return Err("Solar report media filename exceeds 240 bytes".into());
    }
    Ok(format!("media/{:03}-{base}", index + 1))
}

pub fn plan(canonical: &str, markdown: &str) -> Result<Plan, String> {
    if canonical.len() < 4
        || canonical.len() > 203
        || !canonical.to_ascii_lowercase().ends_with(".md")
        || !canonical.as_bytes()[0].is_ascii_alphanumeric()
        || !canonical
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
    {
        return Err("A Solar prompt bundle requires a safe Markdown source name".into());
    }
    let images = ds_solar_contracts::report_media::images(markdown).map_err(|e| e.message)?;
    let images = images
        .into_iter()
        .enumerate()
        .map(|(i, image)| {
            Ok(PlannedImage {
                bundle_path: media_name(&image.reference, i)?,
                alt: image.alt,
                reference: image.reference,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let stem = &canonical[..canonical.len() - 3];
    Ok(Plan {
        canonical: canonical.into(),
        preview: format!("{stem}.preview.md"),
        name: format!("{stem}-prompt-bundle.zip"),
        images,
    })
}

pub fn build(
    canonical: &str,
    markdown: &str,
    media: &BTreeMap<String, Vec<u8>>,
) -> Result<Vec<u8>, String> {
    let plan = plan(canonical, markdown)?;
    if media.len() != plan.images.len() || media.len() > MAX_IMAGES {
        return Err("The prompt bundle is incomplete or contains undeclared media".into());
    }
    let mut resolved = BTreeMap::new();
    let mut total = 0usize;
    for image in &plan.images {
        let bytes = media
            .get(&image.reference)
            .ok_or("The prompt bundle is incomplete: a referenced image is unavailable")?;
        if bytes.is_empty() || bytes.len() > MAX_IMAGE_BYTES {
            return Err("Solar report image is empty or exceeds 16 MiB".into());
        }
        total = total
            .checked_add(bytes.len())
            .ok_or("Solar report media size overflow")?;
        if total > MAX_MEDIA_BYTES {
            return Err("Solar report images exceed 128 MiB".into());
        }
        resolved.insert(image.reference.clone(), image.bundle_path.clone());
    }
    let preview =
        ds_solar_contracts::report_media::project(markdown, &resolved).map_err(|e| e.message)?;
    let manifest = serde_json::json!({"schema":"ds.solar-report-media/v1","canonical":plan.canonical,"preview":plan.preview,"images":plan.images.iter().map(|image| serde_json::json!({"reference":image.reference,"bundle_path":image.bundle_path})).collect::<Vec<_>>()});
    let manifest = format!(
        "{}\n",
        serde_json::to_string_pretty(&manifest).map_err(|e| e.to_string())?
    );
    let readme = format!("{} is the canonical authoring source. Edit only its named LLM_EDIT regions.\n{} is a presentation copy whose image links point at this bundle's media directory; never import it as the final.\nImages are never factual inputs. Every narration block carries its complete facts, table rows, and trend series in its own CONTEXT_DATA.\n", plan.canonical, plan.preview);
    let options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Stored)
        .last_modified_time(
            DateTime::from_date_and_time(1980, 1, 1, 0, 0, 0)
                .map_err(|_| "Invalid archive timestamp")?,
        )
        .unix_permissions(0o644);
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    let mut add = |name: &str, bytes: &[u8]| -> Result<(), String> {
        writer
            .start_file(name, options)
            .map_err(|e| e.to_string())?;
        writer.write_all(bytes).map_err(|e| e.to_string())
    };
    for image in &plan.images {
        add(&image.bundle_path, &media[&image.reference])?;
    }
    add(&plan.canonical, markdown.as_bytes())?;
    add(&plan.preview, preview.as_bytes())?;
    add("media-manifest.json", manifest.as_bytes())?;
    add("README.txt", readme.as_bytes())?;
    Ok(writer.finish().map_err(|e| e.to_string())?.into_inner())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EncodedRequest {
    canonical: String,
    markdown: String,
    images: Vec<EncodedImage>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EncodedImage {
    reference: String,
    body_base64: String,
}
pub fn build_encoded(input: &[u8]) -> Result<Vec<u8>, String> {
    if input.len() > 192 << 20 {
        return Err("Solar report package request exceeds 192 MiB".into());
    }
    let request: EncodedRequest =
        serde_json::from_slice(input).map_err(|_| "Invalid closed Solar report package request")?;
    if request.images.len() > MAX_IMAGES {
        return Err("Solar report has too many images".into());
    }
    let mut media = BTreeMap::new();
    let mut total = 0;
    for image in request.images {
        if image.body_base64.len() > MAX_IMAGE_BYTES.div_ceil(3) * 4 {
            return Err("Solar report image exceeds 16 MiB".into());
        }
        let bytes = STANDARD
            .decode(image.body_base64)
            .map_err(|_| "Solar report image encoding is invalid")?;
        total += bytes.len();
        if total > MAX_MEDIA_BYTES || media.insert(image.reference, bytes).is_some() {
            return Err("Solar report media is repeated or exceeds 128 MiB".into());
        }
    }
    build(&request.canonical, &request.markdown, &media)
}

#[cfg(test)]
mod tests {
    #[test]
    fn reviewed_render_copy_keeps_source_and_data_and_rejects_unfinished_or_wrong_source() {
        let source = "<!-- LLM_AUTHORING_GUIDE_START -->\nInstructions.\n<!-- LLM_AUTHORING_GUIDE_END -->\n\n# Rapport\n\n<!-- FACT_LEDGER: sealed -->\nNarration vérifiée.\n\n| City | kW |\n|---|---|\n| Beinamar | 100 |\n\n![Plant](plots/plant.png)\n";
        let media = BTreeMap::from([(
            "plots/plant.png".into(),
            b"\x89PNG\r\n\x1a\nfixture".to_vec(),
        )]);
        let archive = build("apd-final-fr.md", source, &media).unwrap();
        let copy = render_copy(
            &archive,
            "apd-final-fr.md",
            &ds_solar_contracts::digest::sha256_hex(source.as_bytes()),
        )
        .unwrap();
        assert!(copy.markdown.starts_with("# Rapport"));
        assert!(
            copy.markdown.contains("| Beinamar | 100 |")
                && copy.markdown.contains("Narration vérifiée.")
        );
        assert!(copy.markdown.contains("media/001-plant.png"));
        assert!(!copy.markdown.contains("Instructions."));
        assert_eq!(copy.media["media/001-plant.png"], media["plots/plant.png"]);
        let mut original = Vec::new();
        zip::ZipArchive::new(Cursor::new(&archive))
            .unwrap()
            .by_name("apd-final-fr.md")
            .unwrap()
            .read_to_end(&mut original)
            .unwrap();
        assert_eq!(original, source.as_bytes());
        assert!(render_copy(
            &archive,
            "apd-final-fr.md",
            &format!("sha256:{}", "a".repeat(64))
        )
        .is_err());
        let unfinished = source.replace(
            "Narration vérifiée.",
            "[LLM_START: summary]Instruction[LLM_END: summary]",
        );
        let draft = build("apd-final-fr.md", &unfinished, &media).unwrap();
        assert!(render_copy(
            &draft,
            "apd-final-fr.md",
            &ds_solar_contracts::digest::sha256_hex(unfinished.as_bytes())
        )
        .is_err());
    }
    use super::*;
    use std::io::Read;
    #[test]
    fn exact_authority_and_media_survive_a_deterministic_archive() {
        let source = "# APD\n\n<!-- FACT_LEDGER: unchanged -->\n![Load](plots/site/load.png)\n";
        let images = BTreeMap::from([("plots/site/load.png".into(), vec![0, 1, 2, 255])]);
        let bytes = build("apd-draft-fr.md", source, &images).unwrap();
        assert_eq!(bytes, build("apd-draft-fr.md", source, &images).unwrap());
        let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
        let mut canonical = String::new();
        zip.by_name("apd-draft-fr.md")
            .unwrap()
            .read_to_string(&mut canonical)
            .unwrap();
        assert_eq!(canonical, source);
        let mut preview = String::new();
        zip.by_name("apd-draft-fr.preview.md")
            .unwrap()
            .read_to_string(&mut preview)
            .unwrap();
        assert!(preview.contains("media/001-load.png"));
        assert!(preview.contains("<!-- FACT_LEDGER: unchanged -->"));
        let mut media = Vec::new();
        zip.by_name("media/001-load.png")
            .unwrap()
            .read_to_end(&mut media)
            .unwrap();
        assert_eq!(media, [0, 1, 2, 255]);
        assert!(build("apd.md", source, &BTreeMap::new()).is_err());
        assert!(build("../apd.md", source, &images).is_err());
        let mut extra = images;
        extra.insert("undeclared.png".into(), vec![1]);
        assert!(build("apd.md", source, &extra).is_err());
    }
    #[test]
    fn names_are_owned_bounded_and_never_escape_the_media_directory() {
        assert_eq!(
            media_name("gs://bucket/%2e%2e?generation=7", 0).unwrap(),
            "media/001-figure-1.bin"
        );
        assert_eq!(
            media_name("https://host/a%20b.png?token=private", 1).unwrap(),
            "media/002-a-b.png"
        );
        assert!(media_name("x", 512).is_err());
        let request = br#"{"canonical":"apd.md","markdown":"![A](a)","images":[{"reference":"a","body_base64":"AQ=="},{"reference":"a","body_base64":"AQ=="}]}"#;
        assert!(build_encoded(request).is_err());
    }
}
