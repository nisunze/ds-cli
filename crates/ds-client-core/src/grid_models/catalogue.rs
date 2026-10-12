//! The same closed project-head list through a held authenticated session.
//! A consuming catalogue read walks metadata only; it never downloads a model.
use super::*;
use crate::SecretRequestBody;

pub struct Read {
    project: String,
    cursor: Option<String>,
}
impl Read {
    pub fn new(project: &str, cursor: Option<String>) -> Result<Self, ClientError> {
        Command::List {
            limit: 100,
            cursor: cursor.clone(),
            include_deleted: false,
        }
        .validate(project)?;
        Ok(Self {
            project: project.into(),
            cursor,
        })
    }
    pub const fn path(&self) -> &'static str {
        PATH
    }
    pub const fn timeout_seconds(&self) -> u64 {
        120
    }
    pub const fn response_limit(&self) -> usize {
        8 * 1024 * 1024
    }
    pub fn body(&self) -> SecretRequestBody {
        SecretRequestBody::new(
            serde_json::to_vec(&list_payload(
                &self.project,
                100,
                self.cursor.as_deref(),
                false,
            ))
            .expect("closed project model list"),
        )
    }
    pub fn unreachable(&self) -> ClientError {
        ClientError::new(
            ErrorKind::Transient,
            "the project model catalogue could not be reached",
        )
    }
    pub fn decode(&self, response: TransportResponse) -> Result<Receipt, ClientError> {
        let data = decode_response(response, "list_models", false)?;
        list_receipt(&self.project, 100, &data)
    }
}

/// One complete bounded metadata walk. Empty pages with `more:true` continue;
/// repeated cursors and duplicate logical identities are refused.
pub fn collect(
    project: &str,
    mut page: impl FnMut(Option<String>) -> Result<Receipt, ClientError>,
) -> Result<Value, ClientError> {
    Read::new(project, None)?;
    let mut cursor = None;
    let mut seen_cursors = std::collections::BTreeSet::new();
    let mut ids = std::collections::BTreeSet::new();
    let mut rows = Vec::new();
    for _ in 0..64 {
        let result = page(cursor.clone())?.data;
        if result["project"] != project {
            return Err(bad());
        }
        let batch = result["models"].as_array().ok_or_else(bad)?;
        for row in batch {
            if !row["model_id"]
                .as_str()
                .is_some_and(|v| ids.insert(v.to_owned()))
            {
                return Err(bad());
            }
            rows.push(row.clone());
        }
        if rows.len() > ds_command_kernel::printing::mv_models::MAX_MODELS {
            return Err(bad());
        }
        match result["more"].as_bool() {
            Some(false) => {
                return Ok(json!({"project":project,"models":rows,"more":false,"next_cursor":""}))
            }
            Some(true) => {
                let next = result["next_cursor"]
                    .as_str()
                    .filter(|v| {
                        !v.is_empty() && v.len() <= 4096 && seen_cursors.insert(v.to_string())
                    })
                    .ok_or_else(bad)?;
                cursor = Some(next.into());
            }
            None => return Err(bad()),
        }
    }
    Err(bad())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn paired_read_uses_the_same_native_list_decoder_and_rejects_crossed_projects() {
        let read = Read::new("template", None).unwrap();
        let body: Value = serde_json::from_slice(read.body().as_bytes()).unwrap();
        assert_eq!(
            body,
            json!({"action":"list_models","project_id":"template","page_size":100,"cursor":"","include_deleted":false})
        );
        let answer = |project: &str| {
            TransportResponse::new(200,serde_json::to_vec(&json!({"success":true,"data":{"models":[{"model_id":"model-1","project_id":project}],"has_more":false,"next_cursor":""}})).unwrap())
        };
        assert_eq!(
            read.decode(answer("template")).unwrap().data["models"][0]["model_id"],
            "model-1"
        );
        assert!(read.decode(answer("foreign")).is_err());
        assert!(Read::new("template", Some("bad\ncursor".into())).is_err());
    }
    #[test]
    fn a_consuming_walk_follows_empty_pages_without_downloading_and_refuses_cycles() {
        let mut calls = Vec::new();
        let complete=collect("template",|cursor| { calls.push(cursor.clone()); Ok(Receipt{data:if cursor.is_none() {json!({"project":"template","models":[],"more":true,"next_cursor":"next"})} else {json!({"project":"template","models":[{"model_id":"model-1"}],"more":false,"next_cursor":""})},bytes:None}) }).unwrap();
        assert_eq!(calls, vec![None, Some("next".into())]);
        assert_eq!(complete["models"].as_array().unwrap().len(), 1);
        assert!(collect("template", |_| Ok(Receipt {
            data: json!({"project":"template","models":[],"more":true,"next_cursor":"same"}),
            bytes: None
        }))
        .is_err());
    }
}
