//! One approved Solar workspace selection per captured native owner/project.
//! Metadata only in the existing Store; never a path approval or a compute job.
use crate::{Error, Result, Store};
use ds_command_kernel::report_export::sha256_hex;
use rusqlite::{params, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};

pub const MAX_SOLAR_WORKSPACE_SELECTIONS: usize = 64;
const MAX_ROW_BYTES: usize = 8 * 1024;
const ROW_SCHEMA: &str = "ds.core.solar-workspace-selection/v1";
const SCHEMA: &str = "CREATE TABLE IF NOT EXISTS solar_workspace_selections(
 owner TEXT NOT NULL, lane TEXT NOT NULL, project TEXT NOT NULL,
 generation INTEGER NOT NULL CHECK(generation > 0),
 payload TEXT NOT NULL, payload_sha256 TEXT NOT NULL,
 PRIMARY KEY(owner,lane,project));";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SolarWorkspaceScope {
    pub owner: String,
    pub lane: String,
    pub project: String,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SolarWorkspaceOrigin {
    Approved,
    Default,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SolarWorkspaceChoice {
    pub workspace_handle: String,
    pub dir: String,
    pub origin: SolarWorkspaceOrigin,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SolarWorkspaceSelection {
    pub schema: String,
    pub scope: SolarWorkspaceScope,
    pub generation: u64,
    pub workspace: SolarWorkspaceChoice,
}
fn refused(message: &str) -> Error {
    Error::Kernel(format!("solar_workspace_selection_refused: {message}"))
}
fn valid_scope(scope: &SolarWorkspaceScope) -> bool {
    scope.owner.len() == 64
        && scope
            .owner
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        && matches!(scope.lane.as_str(), "stable" | "canary")
        && ds_command_kernel::execution_context::valid_project(&scope.project)
}
fn valid_choice(choice: &SolarWorkspaceChoice) -> bool {
    let handle = &choice.workspace_handle;
    handle.len() == 48
        && handle.starts_with("solar-workspace-")
        && handle[16..]
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        && !choice.dir.is_empty()
        && choice.dir.len() <= 4096
        && choice.dir.trim() == choice.dir
        && !choice.dir.chars().any(char::is_control)
}
fn encode(value: &SolarWorkspaceSelection) -> Result<Vec<u8>> {
    if value.schema != ROW_SCHEMA
        || !valid_scope(&value.scope)
        || !valid_choice(&value.workspace)
        || value.generation == 0
        || value.generation > i64::MAX as u64
    {
        return Err(refused("invalid closed selection metadata"));
    }
    let raw = serde_json::to_vec(value)?;
    if raw.len() > MAX_ROW_BYTES {
        return Err(refused("selection metadata exceeds 8 KiB"));
    }
    Ok(raw)
}
fn has_table(connection: &rusqlite::Connection) -> Result<bool> {
    let kind: Option<String> = connection
        .query_row(
            "SELECT type FROM sqlite_master WHERE name='solar_workspace_selections'",
            [],
            |row| row.get(0),
        )
        .optional()?;
    match kind.as_deref() {
        None => Ok(false),
        Some("table") => Ok(true),
        _ => Err(refused(
            "selection storage has an unknown object type; left untouched",
        )),
    }
}
fn read_on(
    connection: &rusqlite::Connection,
    scope: &SolarWorkspaceScope,
) -> Result<Option<SolarWorkspaceSelection>> {
    if !has_table(connection)? {
        return Ok(None);
    }
    // SQLite bounds bytes before Vec allocation, including corrupt oversized rows.
    let row: Option<(Vec<u8>, i64, Vec<u8>, i64)> = connection.query_row(
        "SELECT substr(CAST(payload AS BLOB),1,?4),length(CAST(payload AS BLOB)),substr(CAST(payload_sha256 AS BLOB),1,65),generation FROM solar_workspace_selections WHERE owner=?1 AND lane=?2 AND project=?3",
        params![scope.owner,scope.lane,scope.project,MAX_ROW_BYTES+1],
        |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?)),
    ).optional()?;
    let Some((raw, length, digest, generation)) = row else {
        return Ok(None);
    };
    if length <= 0
        || length as usize > MAX_ROW_BYTES
        || raw.len() != length as usize
        || digest != sha256_hex(&raw).as_bytes()
    {
        return Err(refused(
            "stored selection is corrupt or excessive; left untouched",
        ));
    }
    let value: SolarWorkspaceSelection = serde_json::from_slice(&raw)?;
    encode(&value)?;
    if &value.scope != scope || value.generation != generation as u64 {
        return Err(refused(
            "stored selection crosses its exact scope/generation",
        ));
    }
    Ok(Some(value))
}
impl Store {
    /// Read metadata only. This never creates the optional table or repairs a row.
    pub fn solar_workspace_selection(
        &self,
        scope: &SolarWorkspaceScope,
    ) -> Result<Option<SolarWorkspaceSelection>> {
        if !valid_scope(scope) {
            return Err(refused("invalid native owner/project scope"));
        }
        read_on(&self.connection, scope)
    }
    /// The Solar owner must validate its approved receipt before this CAS write.
    /// None means create-only; another selection can never be silently replaced.
    pub fn set_solar_workspace_selection(
        &mut self,
        scope: &SolarWorkspaceScope,
        choice: &SolarWorkspaceChoice,
        expected_generation: Option<u64>,
    ) -> Result<SolarWorkspaceSelection> {
        if !valid_scope(scope) || !valid_choice(choice) {
            return Err(refused("invalid native owner/project or approved choice"));
        }
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        transaction.execute_batch(SCHEMA)?;
        let previous = read_on(&transaction, scope)?;
        if previous.as_ref().map(|row| row.generation) != expected_generation {
            return Err(refused("selection changed; read it before choosing again"));
        }
        if previous.is_none() {
            let count: i64 = transaction.query_row(
                "SELECT COUNT(*) FROM solar_workspace_selections WHERE owner=?1",
                [&scope.owner],
                |row| row.get(0),
            )?;
            if count >= MAX_SOLAR_WORKSPACE_SELECTIONS as i64 {
                return Err(refused(
                    "the native owner already retains 64 project selections",
                ));
            }
        }
        let generation = previous.map_or(Ok(1), |row| {
            row.generation
                .checked_add(1)
                .filter(|n| *n <= i64::MAX as u64)
                .ok_or_else(|| refused("selection generation is exhausted"))
        })?;
        let value = SolarWorkspaceSelection {
            schema: ROW_SCHEMA.into(),
            scope: scope.clone(),
            generation,
            workspace: choice.clone(),
        };
        let raw = encode(&value)?;
        transaction.execute(
            "INSERT INTO solar_workspace_selections(owner,lane,project,generation,payload,payload_sha256) VALUES(?1,?2,?3,?4,?5,?6) ON CONFLICT(owner,lane,project) DO UPDATE SET generation=excluded.generation,payload=excluded.payload,payload_sha256=excluded.payload_sha256",
            params![scope.owner,scope.lane,scope.project,generation as i64,String::from_utf8(raw.clone()).map_err(|_|refused("selection is not UTF-8"))?,sha256_hex(&raw)],
        )?;
        transaction.commit()?;
        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn scope() -> SolarWorkspaceScope {
        SolarWorkspaceScope {
            owner: "a".repeat(64),
            lane: "canary".into(),
            project: "project-a".into(),
        }
    }
    fn choice() -> SolarWorkspaceChoice {
        SolarWorkspaceChoice {
            workspace_handle: format!("solar-workspace-{}", "b".repeat(32)),
            dir: "/approved/custom/solar".into(),
            origin: SolarWorkspaceOrigin::Approved,
        }
    }
    #[test]
    fn selection_survives_real_sqlite_reopen_without_a_second_store_or_schema_change() {
        let dir = crate::test_directory::tempdir().unwrap();
        let path = dir.path().join("core.sqlite");
        assert!(Store::open_read_only(&path).unwrap().is_none());
        let mut store = Store::open(&path).unwrap();
        assert!(store.solar_workspace_selection(&scope()).unwrap().is_none());
        assert!(!has_table(&store.connection).unwrap());
        let stored = store
            .set_solar_workspace_selection(&scope(), &choice(), None)
            .unwrap();
        assert_eq!(stored.generation, 1);
        drop(store);
        let reopened = Store::open_read_only(&path).unwrap().unwrap();
        assert_eq!(
            reopened.solar_workspace_selection(&scope()).unwrap(),
            Some(stored)
        );
        assert_eq!(
            crate::schema_version(&reopened.connection).unwrap(),
            Some(crate::SCHEMA_VERSION)
        );
    }
    #[test]
    fn account_lane_and_project_never_share_a_selection() {
        let mut store = Store::in_memory().unwrap();
        store
            .set_solar_workspace_selection(&scope(), &choice(), None)
            .unwrap();
        for other in [
            SolarWorkspaceScope {
                owner: "c".repeat(64),
                ..scope()
            },
            SolarWorkspaceScope {
                lane: "stable".into(),
                ..scope()
            },
            SolarWorkspaceScope {
                project: "project-b".into(),
                ..scope()
            },
        ] {
            assert!(store.solar_workspace_selection(&other).unwrap().is_none());
        }
        assert!(store
            .solar_workspace_selection(&SolarWorkspaceScope {
                owner: "forged".into(),
                ..scope()
            })
            .is_err());
    }
    #[test]
    fn two_connections_cas_refuses_late_default_without_losing_custom_selection() {
        let dir = crate::test_directory::tempdir().unwrap();
        let path = dir.path().join("core.sqlite");
        let mut first = Store::open(&path).unwrap();
        let mut second = Store::open(&path).unwrap();
        let custom = first
            .set_solar_workspace_selection(&scope(), &choice(), None)
            .unwrap();
        let default = SolarWorkspaceChoice {
            origin: SolarWorkspaceOrigin::Default,
            dir: "/default/solar".into(),
            ..choice()
        };
        assert!(second
            .set_solar_workspace_selection(&scope(), &default, None)
            .is_err());
        assert_eq!(
            second.solar_workspace_selection(&scope()).unwrap(),
            Some(custom.clone())
        );
        let successor = second
            .set_solar_workspace_selection(&scope(), &default, Some(custom.generation))
            .unwrap();
        assert_eq!(successor.generation, 2);
        assert!(first
            .set_solar_workspace_selection(&scope(), &choice(), Some(1))
            .is_err());
        assert_eq!(
            first.solar_workspace_selection(&scope()).unwrap(),
            Some(successor)
        );
    }
    #[test]
    fn corrupt_or_oversized_rows_refuse_read_and_replacement_without_repair() {
        let mut store = Store::in_memory().unwrap();
        let stored = store
            .set_solar_workspace_selection(&scope(), &choice(), None)
            .unwrap();
        store
            .connection
            .execute(
                "UPDATE solar_workspace_selections SET payload=zeroblob(?1)",
                [MAX_ROW_BYTES + 1],
            )
            .unwrap();
        assert!(store.solar_workspace_selection(&scope()).is_err());
        assert!(store
            .set_solar_workspace_selection(&scope(), &choice(), Some(stored.generation))
            .is_err());
        let size: i64 = store
            .connection
            .query_row(
                "SELECT length(payload) FROM solar_workspace_selections",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(size, (MAX_ROW_BYTES + 1) as i64);
    }
    #[test]
    fn stale_digest_crossed_scope_and_unknown_schema_are_not_adopted() {
        for kind in ["digest", "scope", "schema", "generation"] {
            let mut store = Store::in_memory().unwrap();
            let mut row = store
                .set_solar_workspace_selection(&scope(), &choice(), None)
                .unwrap();
            match kind {
                "scope" => row.scope.project = "project-b".into(),
                "schema" => row.schema = "future".into(),
                "generation" => row.generation = 2,
                _ => (),
            }
            let raw = serde_json::to_vec(&row).unwrap();
            let hash = if kind == "digest" {
                "0".repeat(64)
            } else {
                sha256_hex(&raw)
            };
            store
                .connection
                .execute(
                    "UPDATE solar_workspace_selections SET payload=?1,payload_sha256=?2",
                    params![String::from_utf8(raw).unwrap(), hash],
                )
                .unwrap();
            assert!(store.solar_workspace_selection(&scope()).is_err(), "{kind}");
            assert!(
                store
                    .set_solar_workspace_selection(&scope(), &choice(), Some(1))
                    .is_err(),
                "{kind}"
            );
        }
    }
    #[test]
    fn sixty_four_project_bound_refuses_new_rows_but_allows_explicit_existing_successor() {
        let mut store = Store::in_memory().unwrap();
        for n in 0..MAX_SOLAR_WORKSPACE_SELECTIONS {
            store
                .set_solar_workspace_selection(
                    &SolarWorkspaceScope {
                        project: format!("project-{n}"),
                        ..scope()
                    },
                    &choice(),
                    None,
                )
                .unwrap();
        }
        assert!(store
            .set_solar_workspace_selection(&scope(), &choice(), None)
            .is_err());
        let first = SolarWorkspaceScope {
            project: "project-0".into(),
            ..scope()
        };
        assert_eq!(
            store
                .set_solar_workspace_selection(&first, &choice(), Some(1))
                .unwrap()
                .generation,
            2
        );
    }
    #[test]
    fn closed_choice_bounds_refuse_before_table_creation() {
        for candidate in [
            SolarWorkspaceChoice {
                workspace_handle: "../receipt".into(),
                ..choice()
            },
            SolarWorkspaceChoice {
                dir: "bad\npath".into(),
                ..choice()
            },
            SolarWorkspaceChoice {
                dir: "x".repeat(4097),
                ..choice()
            },
        ] {
            let mut store = Store::in_memory().unwrap();
            assert!(store
                .set_solar_workspace_selection(&scope(), &candidate, None)
                .is_err());
            assert!(!has_table(&store.connection).unwrap());
        }
        assert!(serde_json::from_value::<SolarWorkspaceChoice>(serde_json::json!({"workspace_handle":choice().workspace_handle,"dir":"/safe","origin":"approved","credentials":"no"})).is_err());
    }

    #[test]
    fn nul_suffixed_cached_digest_is_refused_as_complete_bytes() {
        let mut store = Store::in_memory().unwrap();
        let row = store
            .set_solar_workspace_selection(&scope(), &choice(), None)
            .unwrap();
        let raw = serde_json::to_vec(&row).unwrap();
        let digest = format!("{}\0hidden suffix", sha256_hex(&raw));
        store
            .connection
            .execute(
                "UPDATE solar_workspace_selections SET payload_sha256=?1",
                [digest],
            )
            .unwrap();
        assert!(store.solar_workspace_selection(&scope()).is_err());
        assert!(store
            .set_solar_workspace_selection(&scope(), &choice(), Some(1))
            .is_err());
        let length: i64 = store
            .connection
            .query_row(
                "SELECT length(CAST(payload_sha256 AS BLOB)) FROM solar_workspace_selections",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(length > 64);
    }
    #[test]
    fn unexpected_optional_object_is_refused_instead_of_selection_absence() {
        let mut store = Store::in_memory().unwrap();
        store
            .connection
            .execute_batch("CREATE VIEW solar_workspace_selections AS SELECT 1 AS future_layout;")
            .unwrap();
        assert!(store.solar_workspace_selection(&scope()).is_err());
        assert!(store
            .set_solar_workspace_selection(&scope(), &choice(), None)
            .is_err());
        let kind: String = store
            .connection
            .query_row(
                "SELECT type FROM sqlite_master WHERE name='solar_workspace_selections'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(kind, "view");
    }
}
