//! One authenticated linked-generation commit through the existing publication
//! path. Every package is planned before upload; no single-version commit loop.
use super::*;
use ds_command_kernel::grid_publication::{identifier, Artifact, Intent, Plan};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LinkedIntent {
    pub project: String,
    pub graph_id: String,
    pub expected_generation: Option<u64>,
    pub generation: u64,
    pub composite: String,
    pub parts: Vec<String>,
    pub canonical_digest: String,
    pub retire: Option<Retirement>,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Retirement {
    pub model_id: String,
    pub expected_head_revision_id: String,
    pub expected_model_digest: String,
    pub reason: String,
}
pub struct Participant {
    pub intent: Intent,
    pub bytes: Vec<u8>,
}
pub struct LinkedPlan {
    pub versions: Vec<Plan>,
    pub checkpoint: Artifact,
    pub request: Value,
}
impl LinkedIntent {
    pub fn plan(
        &self,
        project: &str,
        participants: &[Participant],
        checkpoint: &[u8],
    ) -> Result<LinkedPlan, ClientError> {
        let invalid = || {
            ClientError::new(
                ErrorKind::InvalidInput,
                "invalid linked publication graph, vector or exact bytes",
            )
        };
        let members: BTreeSet<_> = self
            .parts
            .iter()
            .chain(std::iter::once(&self.composite))
            .cloned()
            .collect();
        let next = match self.expected_generation {
            None => self.generation == 0,
            Some(previous) => previous.checked_add(1) == Some(self.generation),
        };
        if self.project != project
            || !identifier(project)
            || !identifier(&self.graph_id)
            || !next
            || self.generation > i64::MAX as u64
            || !(2..=100).contains(&self.parts.len())
            || self.parts.windows(2).any(|p| p[0] >= p[1])
            || members.len() != self.parts.len() + 1
            || members.iter().any(|m| !identifier(m))
            || participants.len() != members.len()
            || !self
                .canonical_digest
                .strip_prefix("sha256:")
                .is_some_and(sha256_hex)
            || checkpoint.is_empty()
            || checkpoint.len() > MAX_BYTES
        {
            return Err(invalid());
        }
        if let Some(retire) = &self.retire {
            if self.expected_generation.is_some()
                || members.contains(&retire.model_id)
                || !identifier(&retire.model_id)
                || !identifier(&retire.expected_head_revision_id)
                || !sha256_hex(&retire.expected_model_digest)
                || retire.reason.is_empty()
                || retire.reason.trim() != retire.reason
                || retire.reason.len() > 2000
                || retire.reason.chars().any(char::is_control)
            {
                return Err(invalid());
            }
        }
        let mut versions = Vec::new();
        for participant in participants {
            if !participant.intent.composition_sources.is_empty() {
                return Err(invalid());
            }
            versions.push(
                participant
                    .intent
                    .plan(project, &participant.bytes)
                    .map_err(|e| ClientError::new(ErrorKind::InvalidInput, e))?,
            );
        }
        versions.sort_by(|a, b| a.intent.model_id.cmp(&b.intent.model_id));
        if versions
            .iter()
            .map(|p| p.intent.model_id.clone())
            .collect::<BTreeSet<_>>()
            != members
            || versions
                .iter()
                .any(|p| p.intent.model_kind != versions[0].intent.model_kind)
        {
            return Err(invalid());
        }
        let digest = format!("{:x}", Sha256::digest(checkpoint));
        let artifact = Artifact {
            object: format!("ds-grid/objects/sha256/{}/{digest}", &digest[..2]),
            digest,
            byte_length: checkpoint.len() as u64,
        };
        let bodies: Vec<_> = versions
            .iter()
            .map(|p| {
                let mut body = p.version_request()["version"].clone();
                body["project_id"] = json!(project);
                body
            })
            .collect();
        let retirement = self.retire.as_ref().map(|r| {
            let mut body = json!(r);
            body["project_id"] = json!(project);
            body
        });
        let request = json!({"action":"publish_linked","project_id":project,"linked":{
            "project_id":project,"graph_id":self.graph_id,"expected_generation":self.expected_generation,
            "generation":self.generation,"composite_model_id":self.composite,"part_model_ids":self.parts,
            "canonical_digest":self.canonical_digest,"checkpoint":artifact,"versions":bodies,"retire":retirement}});
        Ok(LinkedPlan {
            versions,
            checkpoint: artifact,
            request,
        })
    }
}

fn upload<T: AuthorizedTransport>(
    profile: &ClientProfile,
    transport: &mut T,
    authority: Authority<'_>,
    request_body: Value,
    artifact: &Artifact,
    bytes: &[u8],
) -> Result<(), ClientError> {
    let grant = request(profile, transport, authority, request_body.clone())?;
    if let Some(call) = publication::session_upload(&grant, artifact, bytes)? {
        transport.grid_model_upload(call)?;
        let stored = request(profile, transport, authority, request_body)?;
        publication::stored_artifact(&stored, artifact)?;
    }
    Ok(())
}
fn matches(intent: &LinkedIntent, plan: &LinkedPlan, value: &Value) -> bool {
    value["schema"] == 1
        && value["project_id"] == intent.project
        && value["graph_id"] == intent.graph_id
        && value["generation"] == intent.generation
        && value["composite_model_id"] == intent.composite
        && value["part_model_ids"] == json!(intent.parts)
        && value["canonical_digest"] == intent.canonical_digest
        && value["request_digest"].as_str().is_some_and(sha256_hex)
        && serde_json::from_value::<Artifact>(value["checkpoint"].clone())
            .ok()
            .as_ref()
            == Some(&plan.checkpoint)
        && value["versions"].as_array().is_some_and(|pins| {
            pins.len() == plan.versions.len()
                && pins.iter().zip(&plan.versions).all(|(pin, version)| {
                    pin["model_id"] == version.intent.model_id
                        && pin["revision_id"] == version.revision_id
                        && serde_json::from_value::<Artifact>(pin["model"].clone())
                            .ok()
                            .as_ref()
                            == Some(&version.artifact)
                })
        })
}
pub(super) fn run<T: AuthorizedTransport>(
    profile: &ClientProfile,
    transport: &mut T,
    authority: Authority<'_>,
    project: &str,
    intent: &LinkedIntent,
    participants: &[Participant],
    checkpoint: &[u8],
) -> Result<Receipt, ClientError> {
    let plan = intent.plan(project, participants, checkpoint)?;
    // All source bytes and expected heads have been captured/validated first.
    for version in &plan.versions {
        let participant = participants
            .iter()
            .find(|p| p.intent.model_id == version.intent.model_id)
            .expect("validated exact members");
        upload(
            profile,
            transport,
            authority,
            version.upload_request(),
            &version.artifact,
            &participant.bytes,
        )?;
    }
    upload(
        profile,
        transport,
        authority,
        json!({"action":"start_upload","project_id":project,"upload":{
        "model_id":intent.composite,"digest":plan.checkpoint.digest,"byte_length":plan.checkpoint.byte_length,
        "purpose":"export","content_type":"application/vnd.ds-grid.linked-models"}}),
        &plan.checkpoint,
        checkpoint,
    )?;
    let committed = request(profile, transport, authority, plan.request.clone())?;
    if !matches(intent, &plan, &committed["linked"]) {
        return Err(bad());
    }
    let readback = request(
        profile,
        transport,
        authority,
        json!({"action":"get_linked_generation","project_id":project,"graph_id":intent.graph_id,"generation":intent.generation}),
    )?;
    if !matches(intent, &plan, &readback["linked"])
        || committed["linked"]["request_digest"] != readback["linked"]["request_digest"]
    {
        return Err(bad());
    }
    Ok(Receipt {
        data: json!({"status":"published","project":project,"verified":true,"linked":readback["linked"]}),
        bytes: None,
    })
}

fn valid_artifact(value: &Value) -> bool {
    serde_json::from_value::<Artifact>(value.clone()).is_ok_and(|a| {
        sha256_hex(&a.digest)
            && a.byte_length > 0
            && a.byte_length <= MAX_BYTES as u64
            && a.object == format!("ds-grid/objects/sha256/{}/{}", &a.digest[..2], a.digest)
    })
}
fn valid_vector(linked: &Value) -> bool {
    let Some(composite) = linked["composite_model_id"]
        .as_str()
        .filter(|id| identifier(id))
    else {
        return false;
    };
    let Some(parts) = linked["part_model_ids"]
        .as_array()
        .filter(|p| (2..=100).contains(&p.len()))
    else {
        return false;
    };
    let Some(parts) = parts.iter().map(Value::as_str).collect::<Option<Vec<_>>>() else {
        return false;
    };
    if parts.windows(2).any(|p| p[0] >= p[1])
        || parts.iter().any(|p| !identifier(p) || *p == composite)
    {
        return false;
    }
    let members: BTreeSet<_> = parts
        .into_iter()
        .chain(std::iter::once(composite))
        .collect();
    let Some(pins) = linked["versions"]
        .as_array()
        .filter(|p| p.len() == members.len())
    else {
        return false;
    };
    let mut previous = "";
    for pin in pins {
        let Some(id) = pin["model_id"].as_str() else {
            return false;
        };
        if !members.contains(id)
            || id <= previous
            || !pin["revision_id"].as_str().is_some_and(identifier)
            || !valid_artifact(&pin["model"])
        {
            return false;
        }
        previous = id;
    }
    linked["canonical_digest"]
        .as_str()
        .and_then(|s| s.strip_prefix("sha256:"))
        .is_some_and(sha256_hex)
        && valid_artifact(&linked["checkpoint"])
}

pub(super) fn read<T: AuthorizedTransport>(
    profile: &ClientProfile,
    transport: &mut T,
    authority: Authority<'_>,
    project: &str,
    graph: &str,
    generation: u64,
) -> Result<Receipt, ClientError> {
    let data = request(
        profile,
        transport,
        authority,
        json!({"action":"get_linked_generation","project_id":project,"graph_id":graph,"generation":generation}),
    )?;
    let linked = &data["linked"];
    if linked["schema"] != 1
        || linked["project_id"] != project
        || linked["graph_id"] != graph
        || linked["generation"] != generation
        || !linked["request_digest"].as_str().is_some_and(sha256_hex)
        || !valid_vector(linked)
    {
        return Err(bad());
    }
    Ok(Receipt {
        data: linked.clone(),
        bytes: None,
    })
}
