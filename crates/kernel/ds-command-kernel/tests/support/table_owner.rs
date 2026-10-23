//! The Network unit's attribute-table handlers (`selectionFieldInfo`,
//! `tableFilter`, `tableProfiles`, `tableEdit`), reproduced over ds-geo as
//! the reply oracle of the Kernel's admission tests. ds-geo is a
//! dev-dependency only; the unit's own tests run the same work over IPC.
#![allow(dead_code)]
use ds_columnar::schema::infer_row_kinds;
use ds_command_kernel::selection_field::{
    SelectionFieldError, SelectionFieldProfile, SelectionFieldQuery,
};
use ds_command_kernel::table_edit::{
    validate_request, AddColumnEditResult, TableEditError, TableEditRequest, TableEditResponse,
};
use ds_command_kernel::table_query::{
    TableFilterMode, TableFilterQuery, TableFilterResult, TableProfilesQuery, TableProfilesResult,
    TableQueryError,
};
use ds_geo::table::{self, filter_sort_groups, selection_field_values, value_stats};
use ds_geo::table::{FilterSortInput, ValueStatsInput};
use serde_json::{Map, Value};

pub fn selection_field_info(
    query: &SelectionFieldQuery,
    rows: Vec<Map<String, Value>>,
) -> Result<SelectionFieldProfile, SelectionFieldError> {
    query.validate()?;
    let columns = vec![query.column.clone()];
    let kind = infer_row_kinds(&rows, &columns)[&query.column];
    let token_values = selection_field_values(&rows, &query.column, true);
    let mut result = value_stats(ValueStatsInput {
        rows,
        columns,
        options: Default::default(),
    })
    .map_err(SelectionFieldError::InvalidRequest)?;
    let stats = result.stats.remove(0);
    Ok(SelectionFieldProfile {
        stats,
        kind,
        token_values,
    })
}

pub fn filter_rows(
    query: &TableFilterQuery,
    rows: Vec<Map<String, Value>>,
) -> Result<TableFilterResult, TableQueryError> {
    let total_rows = rows.len();
    let indices = filter_sort_groups(
        FilterSortInput {
            rows,
            filters: query.filters.clone(),
            sort: query.sort.clone(),
        },
        query.groups.clone(),
        matches!(query.mode, TableFilterMode::Any),
    )
    .map_err(TableQueryError::InvalidRequest)?;
    Ok(TableFilterResult {
        indices,
        total_rows,
    })
}

pub fn profile_rows(
    query: &TableProfilesQuery,
    rows: Vec<Map<String, Value>>,
) -> Result<TableProfilesResult, TableQueryError> {
    for column in &query.columns {
        SelectionFieldQuery {
            column: column.clone(),
        }
        .validate()
        .map_err(TableQueryError::from)?;
    }
    let total_rows = rows.len();
    let kinds = infer_row_kinds(&rows, &query.columns);
    let tokens = query
        .columns
        .iter()
        .map(|column| selection_field_values(&rows, column, true))
        .collect::<Vec<_>>();
    let result = value_stats(ValueStatsInput {
        rows,
        columns: query.columns.clone(),
        options: query.options.clone().unwrap_or_default(),
    })
    .map_err(TableQueryError::InvalidRequest)?;
    let stats = result
        .stats
        .into_iter()
        .zip(tokens)
        .map(|(stats, token_values)| SelectionFieldProfile {
            kind: kinds[&stats.column],
            stats,
            token_values,
        })
        .collect();
    Ok(TableProfilesResult { stats, total_rows })
}

pub fn execute(request: &TableEditRequest) -> Result<TableEditResponse, TableEditError> {
    use TableEditRequest as Request;
    use TableEditResponse as Response;
    validate_request(request)?;
    match request {
        Request::CountReplace(input) => {
            table::count_replace_matches(input.clone()).map(Response::CountReplace)
        }
        Request::CollectReplace(input) => {
            table::collect_replace_updates(input.clone()).map(Response::CollectReplace)
        }
        Request::AddColumn(input) => table::add_column(input.clone()).and_then(|plan| {
            Ok(Response::AddColumn(AddColumnEditResult {
                plan,
                default_value: table::coerce_value(&input.default_value, input.data_type)?,
            }))
        }),
        Request::CalculateField(input) => {
            table::calculate_field(input.clone()).map(Response::CalculateField)
        }
        Request::GeometryProperties(input) => {
            table::calculate_geometry_properties(input.clone()).map(Response::GeometryProperties)
        }
    }
    .map_err(TableEditError::InvalidRequest)
}
