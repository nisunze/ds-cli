use serde::Serialize;

use crate::{ClientError, ErrorKind, TransportResponse};
use ds_command_kernel::survey_query as kernel;

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
    ) -> Result<Self, ClientError> {
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

    fn validate(&self) -> Result<(), ClientError> {
        kernel_request(self).map(|_| ()).map_err(|_| invalid())
    }

    pub(crate) fn body_bytes(&self, ds_project: &str) -> Vec<u8> {
        kernel_request(self)
            .expect("the closed survey query request remains validated")
            .body_bytes(ds_project)
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

pub(crate) fn validate_project(ds_project: &str) -> Result<(), ClientError> {
    kernel::validate_project(ds_project).map_err(|_| invalid())
}

pub(crate) fn decode(
    response: TransportResponse,
    response_limit: usize,
    request: &SurveyQueryRequest,
) -> Result<SurveyQueryResult, ClientError> {
    let kernel_request = kernel_request(request).map_err(|_| invalid())?;
    let result = kernel::decode(&response.body, response_limit, &kernel_request)
        .map_err(|_| unreadable())?;
    Ok(SurveyQueryResult {
        form: result.form().into(),
        metric: match result.metric() {
            kernel::SurveyQueryMetric::Count => SurveyQueryMetric::Count,
            kernel::SurveyQueryMetric::CountDistinct => SurveyQueryMetric::CountDistinct,
        },
        group_by: result.group_by().to_vec(),
        rows: result
            .rows()
            .iter()
            .map(|row| SurveyQueryRow {
                groups: row.groups().to_vec(),
                value: row.value(),
            })
            .collect(),
        truncated: result.truncated(),
        bytes_processed: result.bytes_processed(),
    })
}

fn invalid() -> ClientError {
    ClientError::new(
        ErrorKind::InvalidInput,
        "the survey aggregate request is outside its closed grammar or bounds",
    )
}

fn unreadable() -> ClientError {
    ClientError::new(
        ErrorKind::UnreadableResponse,
        "the survey aggregate response did not match the bounded server contract",
    )
}

fn kernel_request(
    request: &SurveyQueryRequest,
) -> Result<kernel::SurveyQueryRequest, ds_command_kernel::survey_entry_create::Refusal> {
    let filters = request
        .filters
        .iter()
        .map(kernel_filter)
        .collect::<Vec<_>>();
    kernel::SurveyQueryRequest::new(
        request.form.clone(),
        match request.metric {
            SurveyQueryMetric::Count => kernel::SurveyQueryMetric::Count,
            SurveyQueryMetric::CountDistinct => kernel::SurveyQueryMetric::CountDistinct,
        },
        request.distinct_field.clone(),
        request.group_by.clone(),
        filters,
        match request.order {
            SurveyQueryOrder::Asc => kernel::SurveyQueryOrder::Asc,
            SurveyQueryOrder::Desc => kernel::SurveyQueryOrder::Desc,
        },
        request.limit,
    )
}

fn kernel_filter(filter: &SurveyQueryFilter) -> kernel::SurveyQueryFilter {
    match filter.op {
        SurveyQueryFilterOp::Eq => {
            kernel::SurveyQueryFilter::eq(&filter.field, filter.value.clone().unwrap_or_default())
        }
        SurveyQueryFilterOp::Neq => {
            kernel::SurveyQueryFilter::neq(&filter.field, filter.value.clone().unwrap_or_default())
        }
        SurveyQueryFilterOp::Gte => {
            kernel::SurveyQueryFilter::gte(&filter.field, filter.value.clone().unwrap_or_default())
        }
        SurveyQueryFilterOp::Lte => {
            kernel::SurveyQueryFilter::lte(&filter.field, filter.value.clone().unwrap_or_default())
        }
        SurveyQueryFilterOp::In => {
            kernel::SurveyQueryFilter::in_values(&filter.field, filter.values.clone())
        }
        SurveyQueryFilterOp::Between => kernel::SurveyQueryFilter::between(
            &filter.field,
            filter.from.clone().unwrap_or_default(),
            filter.to.clone().unwrap_or_default(),
        ),
        SurveyQueryFilterOp::IsNull => kernel::SurveyQueryFilter::is_null(&filter.field),
        SurveyQueryFilterOp::NotNull => kernel::SurveyQueryFilter::not_null(&filter.field),
    }
}

pub(crate) fn decode_service_error(
    response: &TransportResponse,
    response_limit: usize,
) -> Option<ClientError> {
    use crate::SurveyQueryServiceCode;
    let code = kernel::decode_service_code(response.status, &response.body, response_limit)?;
    let (kind, message) = match code {
        SurveyQueryServiceCode::FormUnknown => (
            ErrorKind::ResourceNotFound,
            "the bound survey form has no queryable survey view",
        ),
        SurveyQueryServiceCode::FieldUnknown => (
            ErrorKind::InvalidInput,
            "the requested field is not available to this survey aggregate",
        ),
        SurveyQueryServiceCode::ViewStale => (
            ErrorKind::InvalidInput,
            "the survey view is out of date with its governed schema",
        ),
        SurveyQueryServiceCode::TooExpensive => (
            ErrorKind::InvalidInput,
            "the aggregate exceeds the server scan limit",
        ),
        SurveyQueryServiceCode::SyncFailed => (
            ErrorKind::Transient,
            "survey synchronization failed before the query",
        ),
        SurveyQueryServiceCode::Unavailable => (
            ErrorKind::Transient,
            "survey queries are unavailable on this deployment",
        ),
        SurveyQueryServiceCode::ScopeNotFound => (
            ErrorKind::ResourceNotFound,
            "the service refused the project or survey form scope",
        ),
    };
    Some(ClientError::with_survey_query_service_code(
        kind, message, code,
    ))
}
