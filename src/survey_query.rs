use serde::{Deserialize, Serialize};

use crate::survey_entry_create::Refusal;

const MAX_PROJECT_CHARS: usize = 200;
const MAX_IDENTIFIER_CHARS: usize = 128;
const MAX_GROUP_BY: usize = 2;
const MAX_FILTERS: usize = 8;
const MAX_IN_VALUES: usize = 20;
const MAX_ROWS: usize = 200;
const MAX_VALUE_CHARS: usize = 2_048;
const MAX_ENVELOPE_TEXT_CHARS: usize = 512;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SurveyQueryMetric {
    Count,
    CountDistinct,
}

impl SurveyQueryMetric {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Count => "count",
            Self::CountDistinct => "count_distinct",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SurveyQueryOrder {
    Asc,
    Desc,
}

impl SurveyQueryOrder {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Asc => "asc",
            Self::Desc => "desc",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SurveyQueryFilterOp {
    Eq,
    Neq,
    In,
    Gte,
    Lte,
    Between,
    IsNull,
    NotNull,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SurveyQueryFilter {
    field: String,
    op: SurveyQueryFilterOp,
    #[serde(skip_serializing_if = "Option::is_none")]
    value: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    values: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    from: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    to: Option<String>,
}

impl SurveyQueryFilter {
    fn with_value(
        field: impl Into<String>,
        op: SurveyQueryFilterOp,
        value: impl Into<String>,
    ) -> Self {
        Self {
            field: field.into(),
            op,
            value: Some(value.into()),
            values: Vec::new(),
            from: None,
            to: None,
        }
    }

    pub fn eq(field: impl Into<String>, value: impl Into<String>) -> Self {
        Self::with_value(field, SurveyQueryFilterOp::Eq, value)
    }
    pub fn neq(field: impl Into<String>, value: impl Into<String>) -> Self {
        Self::with_value(field, SurveyQueryFilterOp::Neq, value)
    }
    pub fn gte(field: impl Into<String>, value: impl Into<String>) -> Self {
        Self::with_value(field, SurveyQueryFilterOp::Gte, value)
    }
    pub fn lte(field: impl Into<String>, value: impl Into<String>) -> Self {
        Self::with_value(field, SurveyQueryFilterOp::Lte, value)
    }
    pub fn in_values(field: impl Into<String>, values: Vec<String>) -> Self {
        Self {
            field: field.into(),
            op: SurveyQueryFilterOp::In,
            value: None,
            values,
            from: None,
            to: None,
        }
    }
    pub fn between(
        field: impl Into<String>,
        from: impl Into<String>,
        to: impl Into<String>,
    ) -> Self {
        Self {
            field: field.into(),
            op: SurveyQueryFilterOp::Between,
            value: None,
            values: Vec::new(),
            from: Some(from.into()),
            to: Some(to.into()),
        }
    }
    pub fn is_null(field: impl Into<String>) -> Self {
        Self::without_value(field, SurveyQueryFilterOp::IsNull)
    }
    pub fn not_null(field: impl Into<String>) -> Self {
        Self::without_value(field, SurveyQueryFilterOp::NotNull)
    }
    fn without_value(field: impl Into<String>, op: SurveyQueryFilterOp) -> Self {
        Self {
            field: field.into(),
            op,
            value: None,
            values: Vec::new(),
            from: None,
            to: None,
        }
    }
    pub fn field(&self) -> &str {
        &self.field
    }
    pub const fn op(&self) -> SurveyQueryFilterOp {
        self.op
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SurveyQueryRequest {
    form: String,
    metric: SurveyQueryMetric,
    distinct_field: Option<String>,
    group_by: Vec<String>,
    filters: Vec<SurveyQueryFilter>,
    order: SurveyQueryOrder,
    limit: u16,
}

impl SurveyQueryRequest {
    pub fn new(
        form: impl Into<String>,
        metric: SurveyQueryMetric,
        distinct_field: Option<String>,
        group_by: Vec<String>,
        filters: Vec<SurveyQueryFilter>,
        order: SurveyQueryOrder,
        limit: u16,
    ) -> Result<Self, Refusal> {
        let request = Self {
            form: form.into(),
            metric,
            distinct_field,
            group_by,
            filters,
            order,
            limit,
        };
        request.validate()?;
        Ok(request)
    }

    pub fn form(&self) -> &str {
        &self.form
    }
    pub const fn metric(&self) -> SurveyQueryMetric {
        self.metric
    }
    pub fn distinct_field(&self) -> Option<&str> {
        self.distinct_field.as_deref()
    }
    pub fn group_by(&self) -> &[String] {
        &self.group_by
    }
    pub fn filters(&self) -> &[SurveyQueryFilter] {
        &self.filters
    }
    pub const fn order(&self) -> SurveyQueryOrder {
        self.order
    }
    pub const fn limit(&self) -> u16 {
        self.limit
    }

    fn validate(&self) -> Result<(), Refusal> {
        if !identifier(&self.form)
            || self.group_by.len() > MAX_GROUP_BY
            || self.filters.len() > MAX_FILTERS
            || self.limit == 0
            || usize::from(self.limit) > MAX_ROWS
            || self.group_by.iter().any(|field| !identifier(field))
        {
            return Err(invalid());
        }
        match (self.metric, self.distinct_field.as_deref()) {
            (SurveyQueryMetric::Count, None) => {}
            (SurveyQueryMetric::CountDistinct, Some(field)) if identifier(field) => {}
            _ => return Err(invalid()),
        }
        for filter in &self.filters {
            if !identifier(&filter.field) {
                return Err(invalid());
            }
            let valid = match filter.op {
                SurveyQueryFilterOp::Eq
                | SurveyQueryFilterOp::Neq
                | SurveyQueryFilterOp::Gte
                | SurveyQueryFilterOp::Lte => filter.value.as_deref().is_some_and(value),
                SurveyQueryFilterOp::In => {
                    (1..=MAX_IN_VALUES).contains(&filter.values.len())
                        && filter.values.iter().all(|item| value(item))
                }
                SurveyQueryFilterOp::Between => {
                    filter.from.as_deref().is_some_and(nonempty_value)
                        && filter.to.as_deref().is_some_and(nonempty_value)
                }
                SurveyQueryFilterOp::IsNull | SurveyQueryFilterOp::NotNull => true,
            };
            if !valid {
                return Err(invalid());
            }
        }
        Ok(())
    }

    pub fn body_bytes(&self, ds_project: &str) -> Vec<u8> {
        #[derive(Serialize)]
        struct Body<'a> {
            ds_project: &'a str,
            form: &'a str,
            metric: SurveyQueryMetric,
            #[serde(skip_serializing_if = "Option::is_none")]
            distinct_field: Option<&'a str>,
            #[serde(skip_serializing_if = "Option::is_none")]
            group_by: Option<&'a [String]>,
            #[serde(skip_serializing_if = "Option::is_none")]
            filters: Option<&'a [SurveyQueryFilter]>,
            order: SurveyQueryOrder,
            limit: u16,
        }
        serde_json::to_vec(&Body {
            ds_project,
            form: &self.form,
            metric: self.metric,
            distinct_field: self.distinct_field.as_deref(),
            group_by: (!self.group_by.is_empty()).then_some(self.group_by.as_slice()),
            filters: (!self.filters.is_empty()).then_some(self.filters.as_slice()),
            order: self.order,
            limit: self.limit,
        })
        .expect("the closed survey query request always serializes")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SurveyQueryResult {
    form: String,
    metric: SurveyQueryMetric,
    group_by: Vec<String>,
    rows: Vec<SurveyQueryRow>,
    truncated: bool,
    bytes_processed: u64,
}

impl SurveyQueryResult {
    pub fn form(&self) -> &str {
        &self.form
    }
    pub const fn metric(&self) -> SurveyQueryMetric {
        self.metric
    }
    pub fn group_by(&self) -> &[String] {
        &self.group_by
    }
    pub fn rows(&self) -> &[SurveyQueryRow] {
        &self.rows
    }
    pub const fn truncated(&self) -> bool {
        self.truncated
    }
    pub const fn bytes_processed(&self) -> u64 {
        self.bytes_processed
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SurveyQueryRow {
    groups: Vec<String>,
    value: u64,
}

impl SurveyQueryRow {
    pub fn groups(&self) -> &[String] {
        &self.groups
    }
    pub const fn value(&self) -> u64 {
        self.value
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    success: bool,
    message: String,
    data: ResultWire,
    timestamp: String,
    service: String,
    version: String,
    #[serde(default)]
    request_id: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ResultWire {
    form: String,
    metric: String,
    group_by: Vec<String>,
    rows: Vec<RowWire>,
    truncated: bool,
    bytes_processed: i64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RowWire {
    groups: Vec<String>,
    value: i64,
}

pub fn validate_project(ds_project: &str) -> Result<(), Refusal> {
    if ds_project.is_empty()
        || ds_project.trim() != ds_project
        || ds_project.chars().count() > MAX_PROJECT_CHARS
        || ds_project.chars().any(char::is_control)
    {
        return Err(invalid());
    }
    Ok(())
}

pub fn decode(
    body: &[u8],
    response_limit: usize,
    request: &SurveyQueryRequest,
) -> Result<SurveyQueryResult, Refusal> {
    if body.len() > response_limit {
        return Err(unreadable());
    }
    let envelope: Envelope = serde_json::from_slice(&body).map_err(|_| unreadable())?;
    if !envelope.success
        || envelope.message != "Survey query"
        || envelope.service != "data-solutions-backend"
        || envelope.version != "1.0.0"
        || !bounded_text(&envelope.timestamp, MAX_ENVELOPE_TEXT_CHARS)
        || (!envelope.request_id.is_empty()
            && !bounded_text(&envelope.request_id, MAX_ENVELOPE_TEXT_CHARS))
        || envelope.data.form != request.form
        || envelope.data.metric != request.metric.as_str()
        || envelope.data.group_by != request.group_by
        || envelope.data.rows.len() > usize::from(request.limit)
        || envelope.data.rows.len() > MAX_ROWS
        || envelope.data.bytes_processed < 0
    {
        return Err(unreadable());
    }
    let mut rows = Vec::with_capacity(envelope.data.rows.len());
    for row in envelope.data.rows {
        if row.value < 0
            || row.groups.len() != request.group_by.len()
            || row.groups.iter().any(|group| !value(group))
        {
            return Err(unreadable());
        }
        rows.push(SurveyQueryRow {
            groups: row.groups,
            value: row.value as u64,
        });
    }
    Ok(SurveyQueryResult {
        form: envelope.data.form,
        metric: request.metric,
        group_by: envelope.data.group_by,
        rows,
        truncated: envelope.data.truncated,
        bytes_processed: envelope.data.bytes_processed as u64,
    })
}

fn identifier(input: &str) -> bool {
    !input.is_empty()
        && input.len() <= MAX_IDENTIFIER_CHARS
        && input
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
}

fn value(input: &str) -> bool {
    input.chars().count() <= MAX_VALUE_CHARS && !input.chars().any(char::is_control)
}

fn nonempty_value(input: &str) -> bool {
    !input.is_empty() && value(input)
}

fn bounded_text(input: &str, max: usize) -> bool {
    !input.is_empty()
        && input.trim() == input
        && input.chars().count() <= max
        && !input.chars().any(char::is_control)
}

fn invalid() -> Refusal {
    Refusal::InvalidInput
}

fn unreadable() -> Refusal {
    Refusal::UnreadableResponse
}
