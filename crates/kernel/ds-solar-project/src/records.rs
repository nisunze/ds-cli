//! One city row holds an editable draft or a legacy complete input.
use crate::{MAX_INPUT_BYTES, Workspace, identifier, timestamp};
use anyhow::{Result, ensure};
use ds_solar_contracts::{
    CityIdentity, SolarCityInput,
    city_draft::{self, CityDraft},
};
use rusqlite::{OptionalExtension, TransactionBehavior, params};
use serde_json::{Value, json};

pub(crate) enum Record {
    Draft(Box<CityDraft>),
    Input(Box<SolarCityInput>),
}
impl Record {
    pub(crate) fn decode(value: Value) -> Result<Self> {
        if value["schema"] == city_draft::SCHEMA {
            let draft: CityDraft = serde_json::from_value(value)?;
            draft.validate()?;
            Ok(Self::Draft(Box::new(draft)))
        } else {
            let input: SolarCityInput = serde_json::from_value(value)?;
            input.validate()?;
            Ok(Self::Input(Box::new(input)))
        }
    }
    pub(crate) fn identity(&self) -> &CityIdentity {
        match self {
            Self::Draft(v) => &v.identity,
            Self::Input(v) => &v.identity,
        }
    }
    pub(crate) fn digest(&self) -> &str {
        match self {
            Self::Draft(v) => &v.content_digest,
            Self::Input(v) => &v.content_digest,
        }
    }
    pub(crate) fn snapshot(&self) -> Value {
        match self {
            Self::Draft(v) => v.snapshot.clone(),
            Self::Input(v) => crate::sync::snapshot(v),
        }
    }
    pub(crate) fn input(&self) -> Result<SolarCityInput> {
        Ok(match self {
            Self::Draft(v) => v.to_input(&timestamp())?,
            Self::Input(v) => v.as_ref().clone(),
        })
    }
    pub(crate) fn initial_revision(&self) -> Option<&str> {
        match self {
            Self::Draft(v) => v.revision.as_ref().and_then(|r| r.revision.as_deref()),
            Self::Input(v) => v.revision.revision.as_deref(),
        }
    }
    fn receipt(&self, created: bool) -> Value {
        let readiness = match self {
            Self::Draft(v) => v.readiness(),
            Self::Input(_) => json!({"input_complete":true}),
        };
        json!({"project_id":self.identity().project_id,"city_id":self.identity().city_id,"content_digest":self.digest(),"created":created,"editable":true,"readiness":readiness,"publication":if created {"pending"} else {"unchanged"}})
    }
}

impl Workspace {
    pub(crate) fn record(&self, city: &str) -> Result<Record> {
        ensure!(identifier(city), "invalid Solar city id");
        let (digest, body): (String, String) =
            self.db
                .query_row("SELECT digest,input FROM cities WHERE id=?1", [city], |r| {
                    Ok((r.get(0)?, r.get(1)?))
                })?;
        ensure!(body.len() <= MAX_INPUT_BYTES, "stored city exceeds 16 MiB");
        let record = Record::decode(serde_json::from_str(&body)?)?;
        ensure!(
            record.identity().city_id == city
                && record.identity().project_id == self.identity.project_id
                && record.identity().root == self.identity.root
                && record.digest() == digest,
            "stored city identity mismatch"
        );
        Ok(record)
    }
    pub fn city_snapshot(&self, city: &str) -> Result<Value> {
        let record = self.record(city)?;
        Ok(json!({"city_id":city,"content_digest":record.digest(),"snapshot":record.snapshot()}))
    }
    pub fn create_city(
        &mut self,
        city: &str,
        display_name: &str,
        snapshot: Option<Value>,
    ) -> Result<Value> {
        let draft = city_draft::create(
            CityIdentity {
                project_id: self.identity.project_id.clone(),
                root: self.identity.root.clone(),
                city_id: city.into(),
                display_name: display_name.into(),
            },
            snapshot,
        )?;
        let body = serde_json::to_string(&draft)?;
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let prior: Option<String> = tx
            .query_row("SELECT input FROM cities WHERE id=?1", [city], |r| r.get(0))
            .optional()?;
        if let Some(prior) = prior {
            let record = Record::decode(serde_json::from_str(&prior)?)?;
            ensure!(
                record.identity().city_id == city
                    && record.identity().project_id == self.identity.project_id
                    && record.identity().root == self.identity.root,
                "stored city identity mismatch"
            );
            return Ok(record.receipt(false));
        }
        tx.execute(
            "INSERT INTO cities(id,digest,input) VALUES(?1,?2,?3)",
            params![city, draft.content_digest, body],
        )?;
        tx.execute(
            "INSERT INTO outbox(kind,identity,digest,payload) VALUES('city',?1,?2,?3)",
            params![city, draft.content_digest, body],
        )?;
        tx.commit()?;
        Ok(Record::Draft(Box::new(draft)).receipt(true))
    }
    pub fn write_city_snapshot(
        &mut self,
        city: &str,
        snapshot: Value,
        expected: Option<&str>,
    ) -> Result<Value> {
        let name = snapshot["01_city_inputs"]["city"]["city"]
            .as_str()
            .or_else(|| snapshot["_root"]["city"].as_str())
            .ok_or_else(|| anyhow::anyhow!("city snapshot needs its display name: set 01_city_inputs.city.city (or _root.city) to a text value"))?
            .to_string();
        let mut draft = city_draft::create(
            CityIdentity {
                project_id: self.identity.project_id.clone(),
                root: self.identity.root.clone(),
                city_id: city.into(),
                display_name: name,
            },
            Some(snapshot),
        )?;
        if expected.is_some() {
            let record = self.record(city)?;
            ensure!(
                Some(record.digest()) == expected,
                "Solar city revision conflict: {city}"
            );
            match record {
                Record::Draft(old) => {
                    draft.run_options = old.run_options;
                    draft.revision = old.revision;
                }
                Record::Input(old) => {
                    draft.run_options = old.run_options;
                    draft.revision = Some(old.revision);
                }
            }
            draft.content_digest = draft.compute_digest()?;
        }
        draft.validate()?;
        let body = serde_json::to_string(&draft)?;
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let prior: Option<String> = tx
            .query_row("SELECT digest FROM cities WHERE id=?1", [city], |r| {
                r.get(0)
            })
            .optional()?;
        ensure!(
            prior.as_deref() == expected,
            "Solar city revision conflict: {city}"
        );
        if prior.as_deref() == Some(&draft.content_digest) {
            return Ok(Record::Draft(Box::new(draft)).receipt(false));
        }
        tx.execute("INSERT INTO cities(id,digest,input) VALUES(?1,?2,?3) ON CONFLICT(id) DO UPDATE SET digest=excluded.digest,input=excluded.input",params![city,draft.content_digest,body])?;
        tx.execute(
            "INSERT INTO outbox(kind,identity,digest,payload) VALUES('city',?1,?2,?3)",
            params![city, draft.content_digest, body],
        )?;
        tx.commit()?;
        Ok(Record::Draft(Box::new(draft)).receipt(true))
    }
}
