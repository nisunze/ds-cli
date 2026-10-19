//! The same closed project-head list through a held authenticated session.
//! A consuming catalogue read walks metadata only; it never downloads a model.
use super::*;
use crate::SecretRequestBody;
use ds_command_kernel::grid_project_catalog as catalog;

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
        let page = catalog::page_from_data(&self.project, &data, usize::from(catalog::PAGE_SIZE))
            .map_err(|_| bad())?;
        Ok(Receipt {
            data: json!({"project":self.project,"models":page.models,"more":page.more,"next_cursor":page.next_cursor}),
            bytes: None,
        })
    }
}

/// One complete bounded metadata walk through the Kernel's single page walk:
/// empty pages with `more:true` continue; repeated cursors, crossed projects
/// and duplicate logical identities are refused.
pub fn collect(
    project: &str,
    mut page: impl FnMut(Option<String>) -> Result<Receipt, ClientError>,
) -> Result<Value, ClientError> {
    Read::new(project, None)?;
    let mut walk = catalog::Walk::new(project).map_err(|_| bad())?;
    loop {
        let data = page(walk.cursor().map(str::to_owned))?.data;
        if data["project"] != project {
            return Err(bad());
        }
        let models = data["models"].as_array().ok_or_else(bad)?.clone();
        let next = catalog::Page {
            models,
            more: data["more"].as_bool().ok_or_else(bad)?,
            next_cursor: data["next_cursor"].as_str().unwrap_or("").to_owned(),
        };
        match walk.accept(next).map_err(|_| bad())? {
            catalog::Walked::More(more) => walk = more,
            catalog::Walked::Complete(listing) => return Ok(listing),
        }
    }
}

#[cfg(test)]
#[path = "catalogue_tests.rs"]
mod tests;
