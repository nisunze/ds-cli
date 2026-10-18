//! Mechanical executor for shared Rust SQL effects. No domain decisions.
use ds_command_kernel::sql_effect::{ResultRows, Statement};
use rusqlite::{
    types::{Value as SqlValue, ValueRef},
    Connection,
};
use serde_json::Value;

pub fn execute(connection: &Connection, effect: &Statement) -> Result<ResultRows, String> {
    effect.validate()?;
    let values: Result<Vec<_>, String> = effect
        .bind
        .iter()
        .map(|value| {
            Ok(match value {
                Value::Null => SqlValue::Null,
                Value::String(s) => SqlValue::Text(s.clone()),
                Value::Number(n) => {
                    if let Some(n) = n.as_i64() {
                        SqlValue::Integer(n)
                    } else {
                        SqlValue::Real(n.as_f64().ok_or("invalid SQL number")?)
                    }
                }
                _ => return Err("invalid SQL scalar".into()),
            })
        })
        .collect();
    let values = values?;
    let mut statement = connection.prepare(&effect.sql).map_err(|e| e.to_string())?;
    let columns = statement.column_count();
    let mut cursor = statement
        .query(rusqlite::params_from_iter(values))
        .map_err(|e| e.to_string())?;
    let mut rows = Vec::new();
    let mut bytes = 2usize;
    while let Some(row) = cursor.next().map_err(|e| e.to_string())? {
        if rows.len() >= effect.max_rows as usize {
            return Err("SQLite row bound exceeded".into());
        }
        let mut values = Vec::with_capacity(columns);
        for column in 0..columns {
            let value = match row.get_ref(column).map_err(|e| e.to_string())? {
                ValueRef::Null => Value::Null,
                ValueRef::Integer(n) => Value::from(n),
                ValueRef::Real(n) => Value::from(n),
                ValueRef::Text(s) => {
                    if s.len() > effect.max_bytes as usize {
                        return Err("SQLite byte bound exceeded".into());
                    }
                    Value::String(std::str::from_utf8(s).map_err(|e| e.to_string())?.into())
                }
                ValueRef::Blob(_) => return Err("SQLite blob needs a byte-handle effect".into()),
            };
            bytes += serde_json::to_vec(&value).map_err(|e| e.to_string())?.len() + 1;
            if bytes > effect.max_bytes as usize {
                return Err("SQLite byte bound exceeded".into());
            }
            values.push(value);
        }
        bytes += 2;
        rows.push(values);
    }
    let result = ResultRows {
        rows,
        changes: connection.changes(),
    };
    result.validate(effect)?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ds_command_kernel::transformer_status_store as status;
    use serde_json::json;
    #[test]
    fn shared_status_schema_roundtrips_multiple_fenced_scopes_in_one_database() {
        let db = Connection::open_in_memory().unwrap();
        execute(&db, &status::schema()).unwrap();
        let scope = status::Scope {
            principal: "u".into(),
            lane: "canary".into(),
            audience: "a".into(),
            project: "p".into(),
        };
        let other = status::Scope {
            principal: "v".into(),
            ..scope.clone()
        };
        for (s, name) in [(&scope, "TX-1"), (&other, "TX-2")] {
            execute(
                &db,
                &status::write(s, &json!({"transformers":[{"name":name}]}), 123).unwrap(),
            )
            .unwrap();
        }
        let a = status::decode(&execute(&db, &status::read(&scope).unwrap()).unwrap())
            .unwrap()
            .unwrap();
        let b = status::decode(&execute(&db, &status::read(&other).unwrap()).unwrap())
            .unwrap()
            .unwrap();
        assert_eq!(a.document["transformers"][0]["name"], "TX-1");
        assert_eq!(b.document["transformers"][0]["name"], "TX-2");
        execute(&db, &status::invalidate(&scope, true).unwrap()).unwrap();
        let failed = status::decode(&execute(&db, &status::read(&scope).unwrap()).unwrap())
            .unwrap()
            .unwrap();
        assert_eq!(failed.document, a.document);
        assert_eq!(failed.sha256, a.sha256);
        assert_eq!(failed.observed_at_ms, a.observed_at_ms);
        assert_eq!(failed.revision, a.revision + 1);
    }
    #[test]
    fn cold_refresh_cannot_cross_invalidation_of_absent_observation() {
        use ds_command_kernel::transformer_status_store as status;
        let db = Connection::open_in_memory().unwrap();
        execute(&db, &status::schema()).unwrap();
        let scope = status::Scope {
            principal: "u".into(),
            lane: "canary".into(),
            audience: "a".into(),
            project: "p".into(),
        };
        let before =
            status::admit_size(&execute(&db, &status::size(&scope).unwrap()).unwrap()).unwrap();
        assert_eq!(before, (None, false));
        execute(&db, &status::invalidate(&scope, false).unwrap()).unwrap();
        let after =
            status::admit_size(&execute(&db, &status::size(&scope).unwrap()).unwrap()).unwrap();
        assert_eq!(after, (Some(1), false));
        let status::Step::Sql {
            statement,
            continuation,
        } = status::start(status::Operation::Commit {
            scope: scope.clone(),
            document: json!({"transformers":[]}),
            now: 1,
            expected: before.0,
        })
        .unwrap()
        else {
            panic!()
        };
        assert_eq!(
            status::resume(continuation, execute(&db, &statement).unwrap()).unwrap_err(),
            "status_refresh_superseded"
        );
        assert!(
            status::decode(&execute(&db, &status::read(&scope).unwrap()).unwrap())
                .unwrap()
                .is_none()
        );
    }
    #[test]
    fn scalar_and_result_bounds_refuse_without_silent_truncation() {
        let db = Connection::open_in_memory().unwrap();
        for effect in [
            Statement::new("SELECT 1 UNION ALL SELECT 2", vec![], 1, 100),
            Statement::new("SELECT 'too many bytes'", vec![], 1, 5),
            Statement::new("SELECT x'0102'", vec![], 1, 100),
            Statement::new("SELECT ?1", vec![json!({})], 1, 100),
        ] {
            assert!(execute(&db, &effect).is_err());
        }
    }
}
