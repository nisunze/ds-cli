//! `ds pm party` — the outside organisations and people a project
//! corresponds with (correspondence.md §Party).
//!
//! A record names its counterparties by party id, never only by free text,
//! and the party that owes the next answer is one of these. Parties are
//! per project; there is no global directory.

pub mod create;
pub mod list;
pub mod update;

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::Arg;
use ds_client_core::project_correspondence::{Action, PartyFilters};
use serde_json::Value;

pub const PARTY_ARG: Arg = Arg::value(
    "party",
    "<party-id>",
    "The party, by the id `ds pm party list` reports.",
)
.required();

/// One party row by id, found by walking the list — the door has no read
/// by id, and a page of 100 name-ordered rows is what it answers.
pub fn find(lane: &str, party_id: &str) -> Result<Option<(String, Value)>, Failure> {
    let mut page = 1;
    loop {
        let report = crate::correspondence(
            lane,
            &Action::PartyList(PartyFilters {
                include_archived: true,
                limit: Some(100),
                page: Some(page),
                ..PartyFilters::default()
            }),
        )?;
        let project = report.project_id().to_owned();
        let answer = report.into_result();
        let rows = answer["parties"].as_array().cloned().unwrap_or_default();
        if let Some(row) = rows.iter().find(|row| row["id"] == party_id) {
            return Ok(Some((project, row.clone())));
        }
        let total = answer["total"].as_i64().unwrap_or(0);
        if rows.is_empty() || (page * 100) as i64 >= total || page >= 10 {
            return Ok(None);
        }
        page += 1;
    }
}

/// Render one party line the same way in every projection of this domain.
pub fn party_line(row: &Value) -> String {
    format!(
        "  {:<14} {:<13} {:<11} {:<36} {}{}\n",
        crate::truncate(row["id"].as_str().unwrap_or("?"), 14),
        row["kind"].as_str().unwrap_or("—"),
        row["role"].as_str().unwrap_or("—"),
        crate::truncate(row["name"].as_str().unwrap_or("(unnamed)"), 36),
        row["emails"]
            .as_array()
            .and_then(|emails| emails.first())
            .and_then(Value::as_str)
            .unwrap_or(""),
        if row["archived"].as_bool().unwrap_or(false) {
            "  · archived"
        } else {
            ""
        },
    )
}
