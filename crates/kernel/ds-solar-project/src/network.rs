//! Independent Solar input persistence with explicit source refresh and manual edits.
use crate::Workspace;
use anyhow::{Result, ensure};
use ds_solar_contracts::network_form::{self, ResolvedInformation};
use serde_json::{Value, json};

impl Workspace {
    pub fn network_form(&self, city: &str) -> Result<Value> {
        let record = self.record(city)?;
        let snapshot = record.snapshot();
        let network = snapshot["01_city_inputs"]
            .get("network_data")
            .cloned()
            .unwrap_or(json!({}));
        let document = network_form::open(&self.identity.project_id, &network)?;
        Ok(
            json!({"project_id":self.identity.project_id,"city_id":city,"expected":record.digest(),"document":document,"legacy":!network_form::is_shared(&snapshot["01_city_inputs"]["network_data"]) && snapshot["01_city_inputs"]["network_data"].get("_solar_seed").is_none()}),
        )
    }
    pub fn save_network_form(
        &mut self,
        city: &str,
        expected: &str,
        document: Value,
        information: Option<&ResolvedInformation>,
    ) -> Result<Value> {
        let record = self.record(city)?;
        ensure!(
            record.digest() == expected,
            "network form changed; reload before saving"
        );
        let frozen = network_form::snapshot(&self.identity.project_id, &document, information)?;
        let document = frozen["document"].clone();
        let preview = frozen["values"].clone();
        let mut snapshot = record.snapshot();
        snapshot["01_city_inputs"]["network_data"] = frozen["stored"].clone();
        let receipt = self.write_city_snapshot(city, snapshot, Some(expected))?;
        Ok(
            json!({"project_id":self.identity.project_id,"city_id":city,"document":document,"expected":receipt["content_digest"],"values":preview,"manual_entry_allowed":true,"storage_copied":true,"source_refresh":"explicit","receipt":receipt}),
        )
    }

    /// Install a verified Solar-owned map without touching unrelated inputs.
    pub fn save_network_map(
        &mut self,
        city: &str,
        expected: &str,
        reference: Value,
        file: Option<&std::path::Path>,
    ) -> Result<Value> {
        let held = self.network_form(city)?;
        ensure!(
            held["expected"] == expected,
            "network form changed; reload before saving map"
        );
        let role = reference["logical_role"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("map role missing"))?;
        let (kind, key) = role
            .split_once(':')
            .ok_or_else(|| anyhow::anyhow!("map role invalid"))?;
        ensure!(
            !key.is_empty()
                && key.len() <= 128
                && key
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b)),
            "invalid map identity"
        );
        ensure!(
            reference["kind"] == "solar_network_media"
                && reference["schema"] == 1
                && reference["object"]
                    .as_str()
                    .is_some_and(|p| p.starts_with(&format!(
                        "{}/{}/network_plots/_immutable/",
                        self.identity.root, city
                    )))
                && reference["generation"].as_u64().is_some_and(|n| n > 0),
            "map must be a verified copy owned by this Solar city"
        );
        if let Some(file) = file {
            let bytes = crate::files::read(file, 16 * 1024 * 1024)?;
            self.cache_network_media(&reference, &bytes)?;
        }
        let mut document = held["document"].clone();
        match kind {
            "network.map" => document["overrides"]["maps"][key] = reference.clone(),
            "network.transformer" => {
                document["overrides"]["maps"]["transformers"][key] = reference.clone()
            }
            _ => anyhow::bail!("map role invalid"),
        }
        self.save_network_form(city, expected, document, None)
    }

    pub fn network_sources(&self, cities: &[String]) -> Result<Value> {
        let mut sources = serde_json::Map::new();
        for city in cities {
            let snapshot = self.record(city)?.snapshot();
            let Some(document) = snapshot["01_city_inputs"].get("network_data") else {
                continue;
            };
            if !network_form::is_shared(document) {
                continue;
            }
            let form = network_form::parse(&self.identity.project_id, document)?;
            if let Some(reference) = form.sources.information {
                sources.insert(city.clone(), serde_json::to_value(reference)?);
            }
        }
        Ok(json!({"project_id":self.identity.project_id,"sources":sources}))
    }
}

impl Workspace {
    pub fn cache_network_media_encoded(&self, reference: &Value, encoded: &str) -> Result<()> {
        use base64::Engine;
        let bytes = base64::engine::general_purpose::STANDARD.decode(encoded)?;
        self.cache_network_media(reference, &bytes)
    }

    /// Keep verified input bytes locally so later runs need no map service.
    pub fn cache_network_media(&self, reference: &Value, bytes: &[u8]) -> Result<()> {
        use sha2::{Digest, Sha256};
        let hash = format!("{:x}", Sha256::digest(bytes));
        ensure!(
            reference["sha256"] == hash
                && reference["size_bytes"] == bytes.len() as u64
                && bytes.len() <= 16 * 1024 * 1024,
            "Solar map bytes do not match their immutable reference"
        );
        ensure!(
            reference["kind"] == "solar_network_media",
            "Expected Solar-owned media"
        );
        let path = self.path.join("input-media").join(hash);
        if path.exists() {
            ensure!(
                crate::files::read(&path, 16 * 1024 * 1024)? == bytes,
                "Cached map hash mismatch"
            );
        } else {
            crate::files::write(&path, bytes)?;
        }
        Ok(())
    }

    pub(crate) fn input_maps(
        &self,
        city: &ds_solar_contracts::SolarCityInput,
    ) -> Result<std::collections::BTreeMap<String, ds_solar_io::output::GeneratedDocument>> {
        fn collect(value: &Value, refs: &mut Vec<Value>) {
            if value["kind"] == "solar_network_media" {
                refs.push(value.clone());
            } else if let Some(object) = value.as_object() {
                for v in object.values() {
                    collect(v, refs);
                }
            }
        }
        let mut refs = Vec::new();
        collect(&city.seeded.city_inputs["network_data"]["maps"], &mut refs);
        let mut result = std::collections::BTreeMap::new();
        for reference in refs {
            let hash = reference["sha256"].as_str().unwrap_or_default();
            ensure!(
                hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit()),
                "Invalid input map hash"
            );
            let path = self.path.join("input-media").join(hash);
            // An online-only image keeps its immutable reference until explicitly copied locally.
            if !path.exists() {
                continue;
            }
            let bytes = crate::files::read(&path, 16 * 1024 * 1024)?;
            self.cache_network_media(&reference, &bytes)?;
            let media = reference["media_type"].as_str().unwrap_or_default();
            let ext = match media {
                "image/png" => "png",
                "image/jpeg" => "jpg",
                "image/webp" => "webp",
                _ => anyhow::bail!("Unsupported input map type"),
            };
            let bucket = reference["bucket"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Map bucket missing"))?;
            let object = reference["object"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Map object missing"))?;
            let generation = reference["generation"]
                .as_u64()
                .ok_or_else(|| anyhow::anyhow!("Map generation missing"))?;
            let uri = format!("gs://{bucket}/{object}?generation={generation}");
            result.insert(
                uri,
                ds_solar_io::output::GeneratedDocument::new(
                    format!("maps/{hash}.{ext}"),
                    media,
                    bytes,
                ),
            );
        }
        Ok(result)
    }
}
