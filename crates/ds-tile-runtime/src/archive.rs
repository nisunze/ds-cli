use super::*;
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TippecanoeOptions {
    pub min_zoom: u8,
    pub max_zoom: u8,
    pub base_zoom: u8,
    pub full_detail: u8,
    pub drop_densest_as_needed: bool,
    pub no_feature_limit: bool,
    pub no_tile_size_limit: bool,
}

impl TippecanoeOptions {
    pub fn project_default() -> Self {
        Self {
            min_zoom: 4,
            max_zoom: 20,
            base_zoom: 15,
            full_detail: 16,
            drop_densest_as_needed: true,
            no_feature_limit: true,
            no_tile_size_limit: true,
        }
    }

    pub fn dataset_default(
        min_zoom: u8,
        max_zoom: u8,
        base_zoom: u8,
        drop_densest_as_needed: bool,
    ) -> Self {
        Self {
            min_zoom,
            max_zoom,
            base_zoom,
            full_detail: 16,
            drop_densest_as_needed,
            no_feature_limit: true,
            no_tile_size_limit: true,
        }
    }
}

pub(super) async fn run_tippecanoe_with(
    tippecanoe: &Path,
    geojsonl_files: &BTreeMap<String, PathBuf>,
    output_path: &Path,
    options: &TippecanoeOptions,
    cancel: &Cancel,
) -> Result<()> {
    let mut command = Command::new(tippecanoe);
    command
        .arg("-o")
        .arg(output_path)
        .arg("--force")
        .arg(format!("--minimum-zoom={}", options.min_zoom))
        .arg(format!("--maximum-zoom={}", options.max_zoom))
        .arg(format!("--full-detail={}", options.full_detail))
        .arg("-B")
        .arg(options.base_zoom.to_string())
        .arg("-r1")
        .arg("--quiet");
    if options.drop_densest_as_needed {
        command.arg("--drop-densest-as-needed");
    }
    if options.no_feature_limit {
        command.arg("--no-feature-limit");
    }
    if options.no_tile_size_limit {
        command.arg("--no-tile-size-limit");
    }
    for (layer_name, path) in geojsonl_files {
        command
            .arg("-L")
            .arg(format!("{layer_name}:{}", path.display()));
    }
    command.env("TIPPECANOE_MAX_THREADS", "2");
    let output = run_child(&mut command, cancel).await?;
    if !output.status.success() {
        anyhow::bail!(
            "Tippecanoe failed (exit {:?}): {}",
            output.status.code(),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let output_name = output_path
        .file_stem()
        .and_then(|value| value.to_str())
        .context("tippecanoe output has no portable name")?;
    normalize_mbtiles_identity(output_path, output_name, geojsonl_files.keys(), options)?;
    Ok(())
}

// Tippecanoe records the literal -o and -L filesystem paths in MBTiles
// metadata. Those paths include a random temporary directory in cloud and a
// machine-specific workspace root on desktop, making otherwise identical
// archives hash differently. Preserve the meaningful engine/policy/layer
// identity while removing placement paths before the shared PMTiles convert.
fn normalize_mbtiles_identity<'a>(
    mbtiles_path: &Path,
    output_name: &str,
    layer_names: impl Iterator<Item = &'a String>,
    options: &TippecanoeOptions,
) -> Result<()> {
    let connection = Connection::open(mbtiles_path)?;
    let layer_names = layer_names.cloned().collect::<Vec<_>>();
    let generator_options = json!({
        "engine": format!("ds-tile-runtime@{}", env!("CARGO_PKG_VERSION")),
        "layers": layer_names,
        "policy": {
            "minzoom": options.min_zoom,
            "maxzoom": options.max_zoom,
            "basezoom": options.base_zoom,
            "full_detail": options.full_detail,
            "drop_densest_as_needed": options.drop_densest_as_needed,
            "no_feature_limit": options.no_feature_limit,
            "no_tile_size_limit": options.no_tile_size_limit,
        }
    })
    .to_string();
    set_mbtiles_metadata(&connection, "name", output_name)?;
    set_mbtiles_metadata(
        &connection,
        "description",
        &format!("Data Solutions vector tiles: {output_name}"),
    )?;
    set_mbtiles_metadata(&connection, "generator_options", &generator_options)?;
    Ok(())
}

fn set_mbtiles_metadata(connection: &Connection, name: &str, value: &str) -> Result<()> {
    let updated = connection.execute(
        "UPDATE metadata SET value = ?1 WHERE name = ?2",
        params![value, name],
    )?;
    if updated == 0 {
        connection.execute(
            "INSERT INTO metadata(name, value) VALUES (?1, ?2)",
            params![name, value],
        )?;
    }
    Ok(())
}

pub(super) fn inspect_mbtiles_zoom_tile_counts(mbtiles_path: &Path) -> Map<String, Value> {
    let Ok(connection) =
        Connection::open_with_flags(mbtiles_path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
    else {
        return Map::new();
    };
    let Ok(mut statement) = connection
        .prepare("SELECT zoom_level, COUNT(*) FROM tiles GROUP BY zoom_level ORDER BY zoom_level")
    else {
        return Map::new();
    };
    let Ok(rows) =
        statement.query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, u64>(1)?)))
    else {
        return Map::new();
    };
    let mut counts = Map::new();
    for row in rows.flatten() {
        counts.insert(row.0.to_string(), json!(row.1));
    }
    counts
}

/// Count rows in the MBTiles `tiles` table through the embedded SQLite
/// library. `Some(0)` is a valid archive with no tiles; `None` means the
/// archive is not readable as MBTiles.
pub(super) fn mbtiles_tile_count(mbtiles_path: &Path) -> Option<u64> {
    let connection =
        Connection::open_with_flags(mbtiles_path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .ok()?;
    connection
        .query_row("SELECT COUNT(*) FROM tiles", [], |row| row.get(0))
        .ok()
}

pub(super) async fn convert_to_pmtiles_with(
    pmtiles: &Path,
    mbtiles_path: &Path,
    pmtiles_path: &Path,
    cancel: &Cancel,
) -> Result<()> {
    if !mbtiles_path.exists() {
        anyhow::bail!(
            "pmtiles convert failed: mbtiles file not found at {}",
            mbtiles_path.display()
        );
    }
    if mbtiles_path.metadata()?.len() == 0 {
        anyhow::bail!("pmtiles convert failed: mbtiles file is empty");
    }
    fix_zero_area_bounds(mbtiles_path);
    let output = run_child(
        Command::new(pmtiles)
            .arg("convert")
            .arg(mbtiles_path)
            .arg(pmtiles_path),
        cancel,
    )
    .await?;
    if !output.status.success() {
        anyhow::bail!(
            "pmtiles convert failed (exit {:?}): stderr={} stdout={}",
            output.status.code(),
            String::from_utf8_lossy(&output.stderr),
            String::from_utf8_lossy(&output.stdout)
        );
    }
    verify_pmtiles_with(pmtiles, pmtiles_path, cancel).await
}

pub(super) async fn verify_pmtiles_with(
    pmtiles: &Path,
    pmtiles_path: &Path,
    cancel: &Cancel,
) -> Result<()> {
    if !pmtiles_path.is_file() || pmtiles_path.metadata()?.len() == 0 {
        anyhow::bail!(
            "pmtiles verify failed: archive is missing or empty at {}",
            pmtiles_path.display()
        );
    }
    let output = run_child(
        Command::new(pmtiles).arg("verify").arg(pmtiles_path),
        cancel,
    )
    .await?;
    if !output.status.success() {
        anyhow::bail!(
            "pmtiles verify failed (exit {:?}): stderr={} stdout={}",
            output.status.code(),
            String::from_utf8_lossy(&output.stderr),
            String::from_utf8_lossy(&output.stdout)
        );
    }
    Ok(())
}

fn fix_zero_area_bounds(mbtiles_path: &Path) {
    let Ok(connection) = Connection::open(mbtiles_path) else {
        return;
    };
    let Ok(bounds) = connection.query_row(
        "SELECT value FROM metadata WHERE name='bounds' LIMIT 1",
        [],
        |row| row.get::<_, String>(0),
    ) else {
        return;
    };
    let parts = bounds
        .split(',')
        .filter_map(|part| part.trim().parse::<f64>().ok())
        .collect::<Vec<_>>();
    if parts.len() != 4 {
        return;
    }
    let (min_lon, min_lat, max_lon, max_lat) = (parts[0], parts[1], parts[2], parts[3]);
    if max_lon - min_lon >= 1e-6 && max_lat - min_lat >= 1e-6 {
        return;
    }
    let buffer = 0.001;
    let new_bounds = format!(
        "{},{},{},{}",
        min_lon - buffer,
        min_lat - buffer,
        max_lon + buffer,
        max_lat + buffer
    );
    let _ = connection.execute(
        "UPDATE metadata SET value = ?1 WHERE name = 'bounds'",
        params![new_bounds],
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    fn test_executable(path: &Path, body: &str) {
        use std::os::unix::fs::PermissionsExt;

        std::fs::write(path, format!("#!/bin/sh\n{body}\n")).unwrap();
        let mut permissions = std::fs::metadata(path).unwrap().permissions();
        permissions.set_mode(0o700);
        std::fs::set_permissions(path, permissions).unwrap();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn pmtiles_archive_is_structurally_verified_before_acceptance() {
        let work = tempfile::tempdir().unwrap();
        let archive = work.path().join("preview.pmtiles");
        let verifier = work.path().join("pmtiles-test");
        std::fs::write(&archive, b"non-empty test archive").unwrap();
        test_executable(
            &verifier,
            "if [ \"$1\" = \"verify\" ] && [ -f \"$2\" ]; then exit 0; fi; exit 9",
        );

        verify_pmtiles_with(&verifier, &archive, &Cancel::default())
            .await
            .unwrap();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn pmtiles_verification_failure_refuses_the_archive() {
        let work = tempfile::tempdir().unwrap();
        let archive = work.path().join("preview.pmtiles");
        let verifier = work.path().join("pmtiles-test");
        std::fs::write(&archive, b"non-empty test archive").unwrap();
        test_executable(&verifier, "echo corrupt-archive >&2; exit 17");

        let error = verify_pmtiles_with(&verifier, &archive, &Cancel::default())
            .await
            .unwrap_err();
        assert!(error.to_string().contains("pmtiles verify failed"));
        assert!(error.to_string().contains("corrupt-archive"));
    }
}
