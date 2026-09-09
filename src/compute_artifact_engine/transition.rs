use super::*;
fn phase(row: &Value, expected: &str) -> Result<()> {
    if text(row, "phase") != expected {
        return Err(failed("Sync response does not match the durable phase."));
    }
    Ok(())
}
pub(super) fn apply(row: &mut Value, event: Event, reopens: &mut u8, now: u64) -> Result<Effect> {
    match event {
        Event::Start => unreachable!(),
        Event::Bytes {
            output_id,
            sha256,
            size_bytes,
            offset,
        } => {
            phase(
                row,
                if offset.is_some() {
                    "uploading"
                } else {
                    "preparing"
                },
            )?;
            let output = output_mut(row, &output_id)?;
            if size_bytes == 0 || size_bytes > 64 * 1024 * 1024 || !hex(&sha256) {
                return Err(conflict(format!(
                    "{output_id}: the final local artifact is empty or exceeds its byte bound."
                )));
            }
            if offset.is_none() {
                if text(output, "state") != "preparing" {
                    return Err(failed("Unexpected preparation response."));
                }
                if !text(output, "expected_sha256").is_empty()
                    && text(output, "expected_sha256") != sha256
                {
                    return Err(conflict(format!(
                        "{output_id}: engine-reported SHA-256 does not match the final file."
                    )));
                }
                if output["expected_size_bytes"]
                    .as_u64()
                    .is_some_and(|v| v != size_bytes)
                {
                    return Err(conflict(format!(
                        "{output_id}: engine-reported size does not match the final file."
                    )));
                }
                output["sha256"] = json!(sha256);
                output["size_bytes"] = json!(size_bytes);
                output["state"] = json!("pending");
                next(row)
            } else {
                if size_bytes != output["size_bytes"].as_u64().unwrap_or(0)
                    || text(output, "sha256") != sha256
                {
                    return Err(conflict(format!(
                        "{output_id}: local artifact bytes changed after it was queued."
                    )));
                }
                let offset = offset.unwrap();
                if offset > size_bytes || complete(output) || text(output, "session_uri").is_empty()
                {
                    return Err(conflict("Resumable upload offset or state is invalid."));
                }
                output["state"] = json!("uploading");
                Ok(Effect::Upload { output_id, offset })
            }
        }
        Event::Opened { response } => {
            phase(row, "opening")?;
            if text(&response, "work_id").is_empty() || response["head_revision"].as_u64().is_none()
            {
                return Err(failed("Compute artifact work identity is missing."));
            }
            if !text(row, "work_id").is_empty()
                && text(row, "work_id") != text(&response, "work_id")
            {
                return Err(conflict(
                    "Idempotent reopen returned a different work identity.",
                ));
            }
            let targets = response["outputs"]
                .as_array()
                .ok_or_else(|| failed("Server output targets are missing."))?;
            let mut ids = std::collections::BTreeSet::new();
            if targets.len() != outputs(row)?.len() {
                return Err(failed(
                    "Server output inventory does not match the publication.",
                ));
            }
            for target in targets {
                let id = text(target, "output_id");
                if !ids.insert(id) {
                    return Err(failed("Server returned duplicate upload targets."));
                }
                let output = output_mut(row, id)?;
                match text(target, "status") {
                    "already_exists" => {
                        output["state"] = json!("already_exists");
                        remove(output, "session_uri");
                    }
                    "upload" if !text(target, "session_uri").is_empty() => {
                        output["state"] = json!("pending");
                        output["session_uri"] = target["session_uri"].clone();
                    }
                    _ => return Err(failed("Server returned an invalid upload target.")),
                }
                if let Some(generation) = target.get("object_generation") {
                    output["object_generation"] = generation.clone();
                }
            }
            row["work_id"] = response["work_id"].clone();
            row["head_revision"] = response["head_revision"].clone();
            next(row)
        }
        Event::Probed {
            output_id,
            status,
            offset,
        } => {
            phase(row, "uploading")?;
            let output = output_mut(row, &output_id)?;
            if complete(output) || text(output, "session_uri").is_empty() {
                return Err(failed("Probe does not match a pending upload."));
            }
            match status.as_str() {
                "expired" => {
                    output["state"] = json!("pending");
                    remove(output, "session_uri");
                    if *reopens >= 1 {
                        return Err(retry(
                            "A newly minted resumable session expired before use.",
                        ));
                    }
                    *reopens += 1;
                    opening(row)
                }
                "completed" => {
                    output["state"] = json!("uploaded");
                    remove(output, "session_uri");
                    next(row)
                }
                "active" => {
                    if offset > output["size_bytes"].as_u64().unwrap_or(0) {
                        return Err(conflict(
                            "Resumable upload offset exceeds the declared bytes.",
                        ));
                    }
                    Ok(Effect::VerifyUpload { output_id, offset })
                }
                _ => Err(retry(format!(
                    "{output_id}: resumable session could not be checked."
                ))),
            }
        }
        Event::Uploaded { output_id } => {
            phase(row, "uploading")?;
            let output = output_mut(row, &output_id)?;
            if text(output, "state") != "uploading" {
                return Err(failed("Upload response does not match an active output."));
            }
            output["state"] = json!("uploaded");
            remove(output, "session_uri");
            next(row)
        }
        Event::Finalized { response } => {
            phase(row, "finalizing")?;
            if !outputs(row)?.iter().all(complete)
                || (!text(&response, "work_id").is_empty()
                    && text(row, "work_id") != text(&response, "work_id"))
            {
                return Err(conflict(
                    "Finalize response does not match the uploaded work.",
                ));
            }
            if response["head_revision"].as_u64().is_none() {
                return Err(failed("Finalize response is missing its head revision."));
            }
            row["head_revision"] = response["head_revision"].clone();
            match text(&response, "state") {
                "stored_stale" => {
                    row["status"] = json!("conflict");
                    row["phase"] = json!("stored_stale");
                    row["error"] = response.get("conflict").cloned().unwrap_or(json!(
                        "The cloud head moved; local artifacts remain as stale history."
                    ));
                    Ok(Effect::Done {
                        state: "stored_stale",
                    })
                }
                "published" => {
                    row["status"] = json!("synced");
                    row["phase"] = json!("published");
                    row["synced_at"] = json!(now);
                    row["error"] = Value::Null;
                    for output in row["outputs"].as_array_mut().unwrap() {
                        remove(output, "session_uri");
                    }
                    Ok(Effect::Done { state: "published" })
                }
                _ => Err(retry("Compute artifact finalize remains pending.")),
            }
        }
    }
}
