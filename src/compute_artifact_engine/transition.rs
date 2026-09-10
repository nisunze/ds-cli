use super::*;
use crate::transfer::{Cause, Phase};

/// A short, safe echo of a host-supplied token. Bounded so a malformed host
/// cannot write an unbounded string into a durable row's error field.
fn bounded_token(value: &str) -> String {
    let clean: String = value
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-'))
        .take(32)
        .collect();
    if clean.is_empty() {
        "unnamed".into()
    } else {
        clean
    }
}

/// Turn a typed transport cause into one operator-readable sentence.
///
/// This exists because the previous text — "resumable session could not be
/// checked" — was the same sentence for an offline device, a refused
/// credential, a name-resolution failure and an unreadable response header. An
/// operator could not act on it and a reviewer could not diagnose from it, so
/// it produced a silent 29-attempt loop instead of a decision. Every sentence
/// here is safe by construction: `Cause` cannot carry a URL, a token, or a
/// response body.
fn describe_probe_cause(cause: Option<&Cause>, retrying: bool) -> String {
    let tail = if retrying {
        " Delivery will try again; the upload keeps its place."
    } else {
        ""
    };
    let head = match cause {
        Some(Cause::HttpStatus { status: 401 }) | Some(Cause::HttpStatus { status: 403 }) => {
            "file storage refused this upload's credentials".to_string()
        }
        Some(Cause::HttpStatus { status }) => {
            format!("file storage answered {status} while checking this upload")
        }
        Some(Cause::Dns) => "this device could not resolve the file-storage address".into(),
        Some(Cause::Connect) => "this device could not reach file storage".into(),
        Some(Cause::Tls) => "the secure connection to file storage could not be established".into(),
        Some(Cause::Timeout { phase }) => match phase {
            Phase::Connect => "the connection to file storage timed out".into(),
            Phase::Request => "sending to file storage timed out".into(),
            Phase::Response => "file storage did not answer in time".into(),
            Phase::Idle => "the upload stopped making progress".into(),
        },
        Some(Cause::ConnectionClosed) => "the connection to file storage closed early".into(),
        Some(Cause::Offline) => "this device is in offline mode".into(),
        Some(Cause::Cancelled) => "the upload check was cancelled".into(),
        Some(Cause::Truncated) => "file storage kept fewer bytes than were sent".into(),
        Some(Cause::DigestMismatch) => "the uploaded bytes do not match this result".into(),
        Some(Cause::LengthMismatch) => "the uploaded size does not match this result".into(),
        Some(Cause::RangeMalformed) => {
            "file storage reported a progress range this app could not read".into()
        }
        Some(Cause::SessionExpired) => "the upload session is no longer available".into(),
        Some(Cause::BlockedByHostPolicy) => {
            "this app was blocked from reaching file storage and was given no reason".into()
        }
        Some(Cause::Inconclusive) | None => {
            "the upload's progress could not be established".to_string()
        }
    };
    format!("{}{}{}", capitalize(&head), ".", tail)
}

fn capitalize(value: &str) -> String {
    let mut chars = value.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

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
            cause,
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
                // Nothing was established. The session is deliberately KEPT —
                // restarting on "I do not know" is how committed bytes get
                // re-sent or abandoned. The operator gets the cause instead of
                // the bare fact that a check did not conclude.
                "inconclusive" => Err(retry(format!(
                    "{output_id}: {}",
                    describe_probe_cause(cause.as_ref(), true)
                ))),
                // The host established that this will not succeed by retrying.
                "failed" => Err(failed(format!(
                    "{output_id}: {}",
                    describe_probe_cause(cause.as_ref(), false)
                ))),
                other => Err(failed(format!(
                    "{output_id}: the upload check reported an unknown state \
                     ({}).",
                    bounded_token(other)
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
