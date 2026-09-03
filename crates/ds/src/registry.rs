//! The command registry: the one place a command becomes reachable.
//!
//! Registering a command is a single act that makes it dispatchable,
//! discoverable, documented and completable at once. There is no second list
//! to keep in step — `ds capabilities` and `ds <domain> --help` walk exactly
//! this table, so a command cannot exist without being described, and cannot
//! be described without existing.
//!
//! Domains are listed in the order the root help prints them. That order is
//! part of the interface: an agent that has seen root help once should find
//! the same domain in the same place next time.

use ds_cli_contract::spec::{Command, Domain};
use ds_cli_contract::{Context, Failure, Handler, Inputs};
use serde_json::Value;

use crate::meta;

/// One registered command: its contract, how to run it, and how to show it
/// to a person. The renderer is separate from the handler so human output is
/// always a projection of the machine result, never a parallel computation.
pub struct Entry {
    pub command: &'static Command,
    pub handler: Handler,
    pub render: fn(&Value) -> String,
}

pub struct Registered {
    pub domain: &'static Domain,
    pub entries: &'static [Entry],
}

static AUTH_ENTRIES: &[Entry] = &[
    Entry {
        command: &ds_cli_auth::STATUS_COMMAND,
        handler: ds_cli_auth::run_status,
        render: ds_cli_auth::render_status,
    },
    Entry {
        command: &ds_cli_auth::LOGIN_COMMAND,
        handler: ds_cli_auth::run_login,
        render: ds_cli_auth::render_login,
    },
    Entry {
        command: &ds_cli_auth::LOGOUT_COMMAND,
        handler: ds_cli_auth::run_logout,
        render: ds_cli_auth::render_logout,
    },
    Entry {
        command: &ds_cli_auth::device::BEGIN_COMMAND,
        handler: ds_cli_auth::device::run_begin,
        render: ds_cli_auth::device::render,
    },
    Entry {
        command: &ds_cli_auth::device::STATUS_COMMAND,
        handler: ds_cli_auth::device::run_status,
        render: ds_cli_auth::device::render,
    },
    Entry {
        command: &ds_cli_auth::device::COMPLETE_COMMAND,
        handler: ds_cli_auth::device::run_complete,
        render: ds_cli_auth::device::render,
    },
    Entry {
        command: &ds_cli_auth::link_approval::COMMAND,
        handler: ds_cli_auth::link_approval::run,
        render: ds_cli_auth::link_approval::render,
    },
    Entry {
        command: &ds_cli_auth::device::LIST_COMMAND,
        handler: ds_cli_auth::device::run_list,
        render: ds_cli_auth::device::render,
    },
    Entry {
        command: &ds_cli_auth::device::READ_COMMAND,
        handler: ds_cli_auth::device::run_read,
        render: ds_cli_auth::device::render,
    },
    Entry {
        command: &ds_cli_auth::device::REVOKE_COMMAND,
        handler: ds_cli_auth::device::run_revoke,
        render: ds_cli_auth::device::render,
    },
    Entry {
        command: &ds_cli_auth::PROJECT_LIST_COMMAND,
        handler: ds_cli_auth::run_project_list,
        render: ds_cli_auth::render_project_list,
    },
    Entry {
        command: &ds_cli_auth::PROJECT_USE_COMMAND,
        handler: ds_cli_auth::run_project_use,
        render: ds_cli_auth::render_project,
    },
    Entry {
        command: &ds_cli_auth::PROJECT_STATUS_COMMAND,
        handler: ds_cli_auth::run_project_status,
        render: ds_cli_auth::render_project,
    },
];

/// Every domain, in root-help order. Static because the table is the
/// interface: it is walked by dispatch, by help and by the contract tests,
/// and all three must be looking at the same thing.
static DSGRID_ENTRIES: &[Entry] = &[
    Entry {
        command: &ds_cli_dsgrid::inspect::COMMAND,
        handler: ds_cli_dsgrid::inspect::run,
        render: ds_cli_dsgrid::inspect::render,
    },
    Entry {
        command: &ds_cli_dsgrid::validate::COMMAND,
        handler: ds_cli_dsgrid::validate::run,
        render: ds_cli_dsgrid::validate::render,
    },
    Entry {
        command: &ds_cli_dsgrid::describe::COMMAND,
        handler: ds_cli_dsgrid::describe::run,
        render: ds_cli_dsgrid::describe::render,
    },
    Entry {
        command: &ds_cli_dsgrid::run::COMMAND,
        handler: ds_cli_dsgrid::run::run,
        render: ds_cli_dsgrid::run::render,
    },
    Entry {
        command: &ds_cli_dsgrid::apply::COMMAND,
        handler: ds_cli_dsgrid::apply::run,
        render: ds_cli_dsgrid::apply::render,
    },
    // The application-local model family. Registered after the file commands
    // and before the one project act, so domain help reads in the order the
    // work happens: know a package, hold a model, publish a revision.
    Entry {
        command: &ds_cli_dsgrid::model::list::COMMAND,
        handler: ds_cli_dsgrid::model::list::run,
        render: ds_cli_dsgrid::model::list::render,
    },
    Entry {
        command: &ds_cli_dsgrid::model::create_local::COMMAND,
        handler: ds_cli_dsgrid::model::create_local::run,
        render: ds_cli_dsgrid::model::create_local::render,
    },
    Entry {
        command: &ds_cli_dsgrid::model::import_external::COMMAND,
        handler: ds_cli_dsgrid::model::import_external::run,
        render: ds_cli_dsgrid::model::import_external::render,
    },
    Entry {
        command: &ds_cli_dsgrid::model::set_active::COMMAND,
        handler: ds_cli_dsgrid::model::set_active::run,
        render: ds_cli_dsgrid::model::set_active::render,
    },
    Entry {
        command: &ds_cli_dsgrid::model::publish_version::COMMAND,
        handler: ds_cli_dsgrid::model::publish_version::run,
        render: ds_cli_dsgrid::model::publish_version::render,
    },
];

/// The exchange domain lists its commands in the order they are meant to be
/// called: classify, then plan, then convert. Domain help prints this order
/// verbatim, so the index doubles as the procedure — a reader who works down
/// the list is following the safe sequence rather than reconstructing it.
static DSGRID_EXCHANGE_ENTRIES: &[Entry] = &[
    Entry {
        command: &ds_cli_dsgrid_exchange::inspect::COMMAND,
        handler: ds_cli_dsgrid_exchange::inspect::run,
        render: ds_cli_dsgrid_exchange::inspect::render,
    },
    Entry {
        command: &ds_cli_dsgrid_exchange::plan::COMMAND,
        handler: ds_cli_dsgrid_exchange::plan::run,
        render: ds_cli_dsgrid_exchange::plan::render,
    },
    Entry {
        command: &ds_cli_dsgrid_exchange::convert::COMMAND,
        handler: ds_cli_dsgrid_exchange::convert::run,
        render: ds_cli_dsgrid_exchange::convert::render,
    },
];

static LIBRARY_ENTRIES: &[Entry] = &[
    Entry {
        command: &ds_cli_library::verify::COMMAND,
        handler: ds_cli_library::verify::run,
        render: ds_cli_library::verify::render,
    },
    Entry {
        command: &ds_cli_library::open::COMMAND,
        handler: ds_cli_library::open::run,
        render: ds_cli_library::open::render,
    },
    Entry {
        command: &ds_cli_library::catalog::COMMAND,
        handler: ds_cli_library::catalog::run,
        render: ds_cli_library::catalog::render,
    },
    Entry {
        command: &ds_cli_library::global_catalog::READ_COMMAND,
        handler: ds_cli_library::global_catalog::run_read,
        render: ds_cli_library::global_catalog::render,
    },
    Entry {
        command: &ds_cli_library::global_catalog::WRITE_COMMAND,
        handler: ds_cli_library::global_catalog::run_write,
        render: ds_cli_library::global_catalog::render,
    },
    Entry {
        command: &ds_cli_library::global_catalog::FORK_COMMAND,
        handler: ds_cli_library::global_catalog::run_fork,
        render: ds_cli_library::global_catalog::render,
    },
    Entry {
        command: &ds_cli_library::global_catalog::UPLOAD_COMMAND,
        handler: ds_cli_library::global_catalog::run_upload,
        render: ds_cli_library::global_catalog::render,
    },
    Entry {
        command: &ds_cli_library::global_catalog::PUBLISH_LIBRARY_COMMAND,
        handler: ds_cli_library::global_catalog::run_publish_library,
        render: ds_cli_library::global_catalog::render,
    },
    Entry {
        command: &ds_cli_library::global_catalog::PUBLISH_EXAMPLE_COMMAND,
        handler: ds_cli_library::global_catalog::run_publish_example,
        render: ds_cli_library::global_catalog::render,
    },
    Entry {
        command: &ds_cli_library::global_catalog::LIBRARY_LIFECYCLE_COMMAND,
        handler: ds_cli_library::global_catalog::run_library_lifecycle,
        render: ds_cli_library::global_catalog::render,
    },
    Entry {
        command: &ds_cli_library::global_catalog::EXAMPLE_LIFECYCLE_COMMAND,
        handler: ds_cli_library::global_catalog::run_example_lifecycle,
        render: ds_cli_library::global_catalog::render,
    },
    Entry {
        command: &ds_cli_library::pack::COMMAND,
        handler: ds_cli_library::pack::run,
        render: ds_cli_library::pack::render,
    },
    Entry {
        command: &ds_cli_library::unpack::COMMAND,
        handler: ds_cli_library::unpack::run,
        render: ds_cli_library::unpack::render,
    },
    Entry {
        command: &ds_cli_library::seed::COMMAND,
        handler: ds_cli_library::seed::run,
        render: ds_cli_library::seed::render,
    },
    Entry {
        command: &ds_cli_library::resolve_native::COMMAND,
        handler: ds_cli_library::resolve_native::run,
        render: ds_cli_library::resolve_native::render,
    },
];

static PLS_ENTRIES: &[Entry] = &[
    Entry {
        command: &ds_cli_pls::backup_create::COMMAND,
        handler: ds_cli_pls::backup_create::run,
        render: ds_cli_pls::backup_create::render,
    },
    Entry {
        command: &ds_cli_pls::pole_capacity::COMMAND,
        handler: ds_cli_pls::pole_capacity::run,
        render: ds_cli_pls::pole_capacity::render,
    },
    Entry {
        command: &ds_cli_pls::reference_closure::COMMAND,
        handler: ds_cli_pls::reference_closure::run,
        render: ds_cli_pls::reference_closure::render,
    },
    Entry {
        command: &ds_cli_pls::section_orientation::COMMAND,
        handler: ds_cli_pls::section_orientation::run,
        render: ds_cli_pls::section_orientation::render,
    },
    Entry {
        command: &ds_cli_pls::compare_don::COMMAND,
        handler: ds_cli_pls::compare_don::run,
        render: ds_cli_pls::compare_don::render,
    },
    Entry {
        command: &ds_cli_pls::terrain_reconcile::COMMAND,
        handler: ds_cli_pls::terrain_reconcile::run,
        render: ds_cli_pls::terrain_reconcile::render,
    },
    Entry {
        command: &ds_cli_pls::deviation_labels::COMMAND,
        handler: ds_cli_pls::deviation_labels::run,
        render: ds_cli_pls::deviation_labels::render,
    },
    Entry {
        command: &ds_cli_pls::delivery_verify::COMMAND,
        handler: ds_cli_pls::delivery_verify::run,
        render: ds_cli_pls::delivery_verify::render,
    },
];

static REPORT_ENTRIES: &[Entry] = &[
    Entry {
        command: &ds_cli_report::engine::COMMAND,
        handler: ds_cli_report::engine::run,
        render: ds_cli_report::engine::render,
    },
    Entry {
        command: &ds_cli_report::tasks::COMMAND,
        handler: ds_cli_report::tasks::run,
        render: ds_cli_report::tasks::render,
    },
    Entry {
        command: &ds_cli_report::export::COMMAND,
        handler: ds_cli_report::export::run,
        render: ds_cli_report::export::render,
    },
    Entry {
        command: &ds_cli_report::bundle::COMMAND,
        handler: ds_cli_report::bundle::run,
        render: ds_cli_report::bundle::render,
    },
    Entry {
        command: &ds_cli_report::project::scope::COMMAND,
        handler: ds_cli_report::project::scope::run,
        render: ds_cli_report::project::scope::render,
    },
    Entry {
        command: &ds_cli_report::project::compounded::COMMAND,
        handler: ds_cli_report::project::compounded::run,
        render: ds_cli_report::project::compounded::render,
    },
    Entry {
        command: &ds_cli_report::project::archives::COMMAND,
        handler: ds_cli_report::project::archives::run,
        render: ds_cli_report::project::archives::render,
    },
];

static SOLAR_ENTRIES: &[Entry] = &[
    Entry {
        command: &ds_cli_solar::engine::COMMAND,
        handler: ds_cli_solar::engine::run,
        render: ds_cli_solar::engine::render,
    },
    Entry {
        command: &ds_cli_solar::compare::COMMAND,
        handler: ds_cli_solar::compare::run,
        render: ds_cli_solar::compare::render,
    },
    Entry {
        command: &ds_cli_solar::input_capture::COMMAND,
        handler: ds_cli_solar::input_capture::run,
        render: ds_cli_solar::input_capture::render,
    },
    Entry {
        command: &ds_cli_solar::input_prepare::COMMAND,
        handler: ds_cli_solar::input_prepare::run,
        render: ds_cli_solar::input_prepare::render,
    },
    Entry {
        command: &ds_cli_solar::prepare::COMMAND,
        handler: ds_cli_solar::prepare::run,
        render: ds_cli_solar::prepare::render,
    },
    Entry {
        command: &ds_cli_solar::run::COMMAND,
        handler: ds_cli_solar::run::run,
        render: ds_cli_solar::run::render,
    },
    Entry {
        command: &ds_cli_solar::paired_run::START_COMMAND,
        handler: ds_cli_solar::paired_run::start,
        render: ds_cli_solar::paired_run::render_start,
    },
    Entry {
        command: &ds_cli_solar::paired_run::PROGRESS_COMMAND,
        handler: ds_cli_solar::paired_run::progress,
        render: ds_cli_solar::paired_run::render_receipt,
    },
    Entry {
        command: &ds_cli_solar::paired_run::RESULT_COMMAND,
        handler: ds_cli_solar::paired_run::result,
        render: ds_cli_solar::paired_run::render_receipt,
    },
    Entry {
        command: &ds_cli_solar::paired_run::CANCEL_COMMAND,
        handler: ds_cli_solar::paired_run::cancel,
        render: ds_cli_solar::paired_run::render_receipt,
    },
    Entry {
        command: &ds_cli_solar::paired_run::READ_COMMAND,
        handler: ds_cli_solar::paired_run::read,
        render: ds_cli_solar::paired_run::render_receipt,
    },
    Entry {
        command: &ds_cli_solar::workflow::RESULTS_READ_COMMAND,
        handler: ds_cli_solar::workflow::results_read,
        render: ds_cli_solar::workflow::render,
    },
    Entry {
        command: &ds_cli_solar::workflow::SYNC_STATUS_COMMAND,
        handler: ds_cli_solar::workflow::sync_status,
        render: ds_cli_solar::workflow::render,
    },
    Entry {
        command: &ds_cli_solar::workflow::PORTFOLIO_LIST_COMMAND,
        handler: ds_cli_solar::workflow::portfolio_list,
        render: ds_cli_solar::workflow::render,
    },
    Entry {
        command: &ds_cli_solar::workflow::PORTFOLIO_READ_COMMAND,
        handler: ds_cli_solar::workflow::portfolio_read,
        render: ds_cli_solar::workflow::render,
    },
    Entry {
        command: &ds_cli_solar::workflow::PORTFOLIO_ANALYSIS_COMMAND,
        handler: ds_cli_solar::workflow::portfolio_analysis,
        render: ds_cli_solar::workflow::render,
    },
    Entry {
        command: &ds_cli_solar::workflow::FINAL_IMPORT_COMMAND,
        handler: ds_cli_solar::workflow::final_import,
        render: ds_cli_solar::workflow::render,
    },
    Entry {
        command: &ds_cli_solar::workflow::FINAL_SUBMIT_COMMAND,
        handler: ds_cli_solar::workflow::final_submit,
        render: ds_cli_solar::workflow::render,
    },
    Entry {
        command: &ds_cli_solar::exports::REPORT_EXPORT_COMMAND,
        handler: ds_cli_solar::exports::export_report,
        render: ds_cli_solar::exports::render,
    },
    Entry {
        command: &ds_cli_solar::exports::PORTFOLIO_EXPORT_COMMAND,
        handler: ds_cli_solar::exports::export_portfolio,
        render: ds_cli_solar::exports::render,
    },
    Entry {
        command: &ds_cli_solar::seed::PREVIEW_COMMAND,
        handler: ds_cli_solar::seed::preview,
        render: ds_cli_solar::seed::render,
    },
    Entry {
        command: &ds_cli_solar::seed::APPLY_COMMAND,
        handler: ds_cli_solar::seed::apply,
        render: ds_cli_solar::seed::render,
    },
    Entry {
        command: &ds_cli_solar::weather::COMMAND,
        handler: ds_cli_solar::weather::run,
        render: ds_cli_solar::weather::render,
    },
];

static MAP_ENTRIES: &[Entry] = &[
    Entry {
        command: &ds_cli_map::view::COMMAND,
        handler: ds_cli_map::view::run,
        render: ds_cli_map::view::render,
    },
    Entry {
        command: &ds_cli_map::draw::COMMAND,
        handler: ds_cli_map::draw::run,
        render: ds_cli_map::draw::render,
    },
    Entry {
        command: &ds_cli_map::remove::COMMAND,
        handler: ds_cli_map::remove::run,
        render: ds_cli_map::remove::render,
    },
    Entry {
        command: &ds_cli_map::zoom::COMMAND,
        handler: ds_cli_map::zoom::run,
        render: ds_cli_map::zoom::render,
    },
    Entry {
        command: &ds_cli_map::layer::list::COMMAND,
        handler: ds_cli_map::layer::list::run,
        render: ds_cli_map::layer::list::render,
    },
    Entry {
        command: &ds_cli_map::layer::reorder::COMMAND,
        handler: ds_cli_map::layer::reorder::run,
        render: ds_cli_map::layer::reorder::render,
    },
    Entry {
        command: &ds_cli_map::layer::remote_list::COMMAND,
        handler: ds_cli_map::layer::remote_list::run,
        render: ds_cli_map::layer::remote_list::render,
    },
    Entry {
        command: &ds_cli_map::layer::add::COMMAND,
        handler: ds_cli_map::layer::add::run,
        render: ds_cli_map::layer::add::render,
    },
    Entry {
        command: &ds_cli_map::layer::remove::COMMAND,
        handler: ds_cli_map::layer::remove::run,
        render: ds_cli_map::layer::remove::render,
    },
    Entry {
        command: &ds_cli_map::layer::visibility::COMMAND,
        handler: ds_cli_map::layer::visibility::run,
        render: ds_cli_map::layer::visibility::render,
    },
    Entry {
        command: &ds_cli_map::ui::open::COMMAND,
        handler: ds_cli_map::ui::open::run,
        render: ds_cli_map::ui::open::render,
    },
    Entry {
        command: &ds_cli_map::evidence::capture::COMMAND,
        handler: ds_cli_map::evidence::capture::run,
        render: ds_cli_map::evidence::capture::render,
    },
    Entry {
        command: &ds_cli_map::points_along::COMMAND,
        handler: ds_cli_map::points_along::run,
        render: ds_cli_map::points_along::render,
    },
    Entry {
        command: &ds_cli_map::random_points::COMMAND,
        handler: ds_cli_map::random_points::run,
        render: ds_cli_map::random_points::render,
    },
    Entry {
        command: &ds_cli_map::outliers::COMMAND,
        handler: ds_cli_map::outliers::run,
        render: ds_cli_map::outliers::render,
    },
    Entry {
        command: &ds_cli_map::line_difference::COMMAND,
        handler: ds_cli_map::line_difference::run,
        render: ds_cli_map::line_difference::render,
    },
    Entry {
        command: &ds_cli_map::survey::download::COMMAND,
        handler: ds_cli_map::survey::download::run,
        render: ds_cli_map::survey::download::render,
    },
    Entry {
        command: &ds_cli_map::survey::plan::COMMAND,
        handler: ds_cli_map::survey::plan::run,
        render: ds_cli_map::survey::plan::render,
    },
    Entry {
        command: &ds_cli_map::survey::apply::COMMAND,
        handler: ds_cli_map::survey::apply::run,
        render: ds_cli_map::survey::apply::render,
    },
    Entry {
        command: &ds_cli_map::design::open::COMMAND,
        handler: ds_cli_map::design::open::run,
        render: ds_cli_map::design::open::render,
    },
    Entry {
        command: &ds_cli_map::design::read::COMMAND,
        handler: ds_cli_map::design::read::run,
        render: ds_cli_map::design::read::render,
    },
    Entry {
        command: &ds_cli_map::design::discard::COMMAND,
        handler: ds_cli_map::design::discard::run,
        render: ds_cli_map::design::discard::render,
    },
    Entry {
        command: &ds_cli_map::design::layer_to_local::COMMAND,
        handler: ds_cli_map::design::layer_to_local::run,
        render: ds_cli_map::design::layer_to_local::render,
    },
    Entry {
        command: &ds_cli_map::design::upload_to_local::COMMAND,
        handler: ds_cli_map::design::upload_to_local::run,
        render: ds_cli_map::design::upload_to_local::render,
    },
    Entry {
        command: &ds_cli_map::design::select::COMMAND,
        handler: ds_cli_map::design::select::run,
        render: ds_cli_map::design::select::render,
    },
    Entry {
        command: &ds_cli_map::design::set::COMMAND,
        handler: ds_cli_map::design::set::run,
        render: ds_cli_map::design::set::render,
    },
    Entry {
        command: &ds_cli_map::design::create::COMMAND,
        handler: ds_cli_map::design::create::run,
        render: ds_cli_map::design::create::render,
    },
    Entry {
        command: &ds_cli_map::design::delete::COMMAND,
        handler: ds_cli_map::design::delete::run,
        render: ds_cli_map::design::delete::render,
    },
    Entry {
        command: &ds_cli_map::design::geometry::COMMAND,
        handler: ds_cli_map::design::geometry::run,
        render: ds_cli_map::design::geometry::render,
    },
    Entry {
        command: &ds_cli_map::design::process_setup::COMMAND,
        handler: ds_cli_map::design::process_setup::run,
        render: ds_cli_map::design::process_setup::render,
    },
    Entry {
        command: &ds_cli_map::design::version_create::COMMAND,
        handler: ds_cli_map::design::version_create::run,
        render: ds_cli_map::design::version_create::render,
    },
    Entry {
        command: &ds_cli_map::design::version_list::COMMAND,
        handler: ds_cli_map::design::version_list::run,
        render: ds_cli_map::design::version_list::render,
    },
    Entry {
        command: &ds_cli_map::design::version_play::COMMAND,
        handler: ds_cli_map::design::version_play::run,
        render: ds_cli_map::design::version_play::render,
    },
    Entry {
        command: &ds_cli_map::design::version_compare::COMMAND,
        handler: ds_cli_map::design::version_compare::run,
        render: ds_cli_map::design::version_compare::render,
    },
    Entry {
        command: &ds_cli_map::design::process::COMMAND,
        handler: ds_cli_map::design::process::run,
        render: ds_cli_map::design::process::render,
    },
    Entry {
        command: &ds_cli_map::design::batch_process::COMMAND,
        handler: ds_cli_map::design::batch_process::run,
        render: ds_cli_map::design::batch_process::render,
    },
    Entry {
        command: &ds_cli_map::design::batch_report::COMMAND,
        handler: ds_cli_map::design::batch_report::run,
        render: ds_cli_map::design::batch_report::render,
    },
    Entry {
        command: &ds_cli_map::design::batch_save::COMMAND,
        handler: ds_cli_map::design::batch_save::run,
        render: ds_cli_map::design::batch_save::render,
    },
    Entry {
        command: &ds_cli_map::design::save::COMMAND,
        handler: ds_cli_map::design::save::run,
        render: ds_cli_map::design::save::render,
    },
    Entry {
        command: &ds_cli_map::design::list::COMMAND,
        handler: ds_cli_map::design::list::run,
        render: ds_cli_map::design::list::render,
    },
    Entry {
        command: &ds_cli_map::design::pin::COMMAND,
        handler: ds_cli_map::design::pin::run,
        render: ds_cli_map::design::pin::render,
    },
    Entry {
        command: &ds_cli_map::design::report::COMMAND,
        handler: ds_cli_map::design::report::run,
        render: ds_cli_map::design::report::render,
    },
    Entry {
        command: &ds_cli_map::design::attach_print::COMMAND,
        handler: ds_cli_map::design::attach_print::run,
        render: ds_cli_map::design::attach_print::render,
    },
    Entry {
        command: &ds_cli_map::design::upload::COMMAND,
        handler: ds_cli_map::design::upload::run,
        render: ds_cli_map::design::upload::render,
    },
    Entry {
        command: &ds_cli_map::design::upload_stage::COMMAND,
        handler: ds_cli_map::design::upload_stage::run,
        render: ds_cli_map::design::upload_stage::render,
    },
];

static SURVEY_ENTRIES: &[Entry] = &[
    Entry {
        command: &ds_cli_survey::forms::LIST_COMMAND,
        handler: ds_cli_survey::forms::list,
        render: ds_cli_survey::forms::render_list,
    },
    Entry {
        command: &ds_cli_survey::forms::READ_COMMAND,
        handler: ds_cli_survey::forms::read,
        render: ds_cli_survey::forms::render_form,
    },
    Entry {
        command: &ds_cli_survey::forms::TYPES_COMMAND,
        handler: ds_cli_survey::forms::types,
        render: ds_cli_survey::forms::render_types,
    },
    Entry {
        command: &ds_cli_survey::forms::CREATE_COMMAND,
        handler: ds_cli_survey::forms::create,
        render: ds_cli_survey::forms::render_form,
    },
    Entry {
        command: &ds_cli_survey::forms::UPDATE_COMMAND,
        handler: ds_cli_survey::forms::update,
        render: ds_cli_survey::forms::render_form,
    },
    Entry {
        command: &ds_cli_survey::forms::LIFECYCLE_COMMAND,
        handler: ds_cli_survey::forms::lifecycle,
        render: ds_cli_survey::forms::render_lifecycle,
    },
    Entry {
        command: &ds_cli_survey::project_forms::READ_COMMAND,
        handler: ds_cli_survey::project_forms::read,
        render: ds_cli_survey::project_forms::render_read,
    },
    Entry {
        command: &ds_cli_survey::project_forms::LIST_COMMAND,
        handler: ds_cli_survey::project_forms::list,
        render: ds_cli_survey::project_forms::render_list,
    },
    Entry {
        command: &ds_cli_survey::project_forms::SETTINGS_COMMAND,
        handler: ds_cli_survey::project_forms::settings,
        render: ds_cli_survey::project_forms::render_settings,
    },
    Entry {
        command: &ds_cli_survey::project_forms::EDITOR_COMMAND,
        handler: ds_cli_survey::project_forms::editor,
        render: ds_cli_survey::project_forms::render_editor,
    },
    Entry {
        command: &ds_cli_survey::project_forms::PLAN_COMMAND,
        handler: ds_cli_survey::project_forms::plan,
        render: ds_cli_survey::project_forms::render_plan,
    },
    Entry {
        command: &ds_cli_survey::project_forms::APPLY_COMMAND,
        handler: ds_cli_survey::project_forms::apply,
        render: ds_cli_survey::project_forms::render_apply,
    },
    Entry {
        command: &ds_cli_survey::query::COMMAND,
        handler: ds_cli_survey::query::run,
        render: ds_cli_survey::query::render,
    },
    Entry {
        command: &ds_cli_survey::entries::COMMAND,
        handler: ds_cli_survey::entries::run,
        render: ds_cli_survey::entries::render,
    },
    Entry {
        command: &ds_cli_survey::changes::COMMAND,
        handler: ds_cli_survey::changes::run,
        render: ds_cli_survey::changes::render,
    },
    Entry {
        command: &ds_cli_survey::create::COMMAND,
        handler: ds_cli_survey::create::run,
        render: ds_cli_survey::create::render,
    },
    Entry {
        command: &ds_cli_survey::import::COMMAND,
        handler: ds_cli_survey::import::run,
        render: ds_cli_survey::import::render,
    },
    Entry {
        command: &ds_cli_survey::templates::LIST_COMMAND,
        handler: ds_cli_survey::templates::list,
        render: ds_cli_survey::templates::render_list,
    },
    Entry {
        command: &ds_cli_survey::templates::READ_COMMAND,
        handler: ds_cli_survey::templates::read,
        render: ds_cli_survey::templates::render_template,
    },
    Entry {
        command: &ds_cli_survey::templates::CREATE_COMMAND,
        handler: ds_cli_survey::templates::create,
        render: ds_cli_survey::templates::render_template,
    },
    Entry {
        command: &ds_cli_survey::templates::APPLY_COMMAND,
        handler: ds_cli_survey::templates::apply,
        render: ds_cli_survey::templates::render_mutation,
    },
    Entry {
        command: &ds_cli_survey::templates::LIFECYCLE_COMMAND,
        handler: ds_cli_survey::templates::lifecycle,
        render: ds_cli_survey::templates::render_mutation,
    },
    Entry {
        command: &ds_cli_survey::templates::CREATE_PROJECT_COMMAND,
        handler: ds_cli_survey::templates::create_project,
        render: ds_cli_survey::templates::render_project,
    },
];

/// Map styling. Ordered as a session uses it: list the refs, read one, author
/// guided base appearance, then plan, publish or clear a second dimension, and
/// last the cartography — line type, direction, casing and hatching — which
/// needs no field and so is reached once the field-driven axes are settled.
static STYLE_ENTRIES: &[Entry] = &[
    Entry {
        command: &ds_cli_style::list::COMMAND,
        handler: ds_cli_style::list::run,
        render: ds_cli_style::list::render,
    },
    Entry {
        command: &ds_cli_style::read::COMMAND,
        handler: ds_cli_style::read::run,
        render: ds_cli_style::read::render,
    },
    Entry {
        command: &ds_cli_style::appearance::plan::COMMAND,
        handler: ds_cli_style::appearance::plan::run,
        render: ds_cli_style::appearance::plan::render,
    },
    Entry {
        command: &ds_cli_style::appearance::set::COMMAND,
        handler: ds_cli_style::appearance::set::run,
        render: ds_cli_style::appearance::set::render,
    },
    Entry {
        command: &ds_cli_style::dimension::plan::COMMAND,
        handler: ds_cli_style::dimension::plan::run,
        render: ds_cli_style::dimension::plan::render,
    },
    Entry {
        command: &ds_cli_style::dimension::set::COMMAND,
        handler: ds_cli_style::dimension::set::run,
        render: ds_cli_style::dimension::set::render,
    },
    Entry {
        command: &ds_cli_style::dimension::clear::COMMAND,
        handler: ds_cli_style::dimension::clear::run,
        render: ds_cli_style::dimension::clear::render,
    },
    Entry {
        command: &ds_cli_style::cartography::plan::COMMAND,
        handler: ds_cli_style::cartography::plan::run,
        render: ds_cli_style::cartography::plan::render,
    },
    Entry {
        command: &ds_cli_style::cartography::set::COMMAND,
        handler: ds_cli_style::cartography::set::run,
        render: ds_cli_style::cartography::set::render,
    },
];

/// Local data preparation. Ordered as a session uses it: look at the source
/// first, then convert with what you saw.
static DATA_ENTRIES: &[Entry] = &[
    Entry {
        command: &ds_cli_data::inspect::COMMAND,
        handler: ds_cli_data::inspect::run,
        render: ds_cli_data::inspect::render,
    },
    Entry {
        command: &ds_cli_data::convert::COMMAND,
        handler: ds_cli_data::convert::run,
        render: ds_cli_data::convert::render,
    },
    Entry {
        command: &ds_cli_data::conversion_matrix::COMMAND,
        handler: ds_cli_data::conversion_matrix::run,
        render: ds_cli_data::conversion_matrix::render,
    },
    Entry {
        command: &ds_cli_data::elevation::COMMAND,
        handler: ds_cli_data::elevation::run,
        render: ds_cli_data::elevation::render,
    },
    Entry {
        command: &ds_cli_data::point_cloud::PLAN_COMMAND,
        handler: ds_cli_data::point_cloud::run_plan,
        render: ds_cli_data::point_cloud::render_plan,
    },
    Entry {
        command: &ds_cli_data::point_cloud::EXTRACT_COMMAND,
        handler: ds_cli_data::point_cloud::run_extract,
        render: ds_cli_data::point_cloud::render_extract,
    },
    Entry {
        command: &ds_cli_data::admin_bounds::COMMAND,
        handler: ds_cli_data::admin_bounds::run,
        render: ds_cli_data::admin_bounds::render,
    },
    Entry {
        command: &ds_cli_data::admin_bounds::LIST_COMMAND,
        handler: ds_cli_data::admin_bounds::run_list,
        render: ds_cli_data::admin_bounds::render_list,
    },
    Entry {
        command: &ds_cli_data::admin_bounds::READ_COMMAND,
        handler: ds_cli_data::admin_bounds::run_read,
        render: ds_cli_data::admin_bounds::render_read,
    },
];

/// Tiling. Ordered as a session uses it: read the state, look at the
/// sources, decide, run; then the catalogue.
static TILE_ENTRIES: &[Entry] = &[
    Entry {
        command: &ds_cli_tile::status::COMMAND,
        handler: ds_cli_tile::status::run,
        render: ds_cli_tile::status::render,
    },
    Entry {
        command: &ds_cli_tile::preflight::COMMAND,
        handler: ds_cli_tile::preflight::run,
        render: ds_cli_tile::preflight::render,
    },
    Entry {
        command: &ds_cli_tile::plan::COMMAND,
        handler: ds_cli_tile::plan::run,
        render: ds_cli_tile::plan::render,
    },
    Entry {
        command: &ds_cli_tile::generate::COMMAND,
        handler: ds_cli_tile::generate::run,
        render: ds_cli_tile::generate::render,
    },
    Entry {
        command: &ds_cli_tile::list::COMMAND,
        handler: ds_cli_tile::list::run,
        render: ds_cli_tile::list::render,
    },
    Entry {
        command: &ds_cli_tile::add::COMMAND,
        handler: ds_cli_tile::add::run,
        render: ds_cli_tile::add::render,
    },
    Entry {
        command: &ds_cli_tile::remove::COMMAND,
        handler: ds_cli_tile::remove::run,
        render: ds_cli_tile::remove::render,
    },
];

/// The Project Work domain lists its commands in the order a session uses
/// them: look at the plan, find the item, read it, then act on it. Domain
/// help prints this order verbatim, so the index doubles as the procedure.
static WORK_ENTRIES: &[Entry] = &[
    Entry {
        command: &ds_cli_work::plan::COMMAND,
        handler: ds_cli_work::plan::run,
        render: ds_cli_work::plan::render,
    },
    Entry {
        command: &ds_cli_work::task::list::COMMAND,
        handler: ds_cli_work::task::list::run,
        render: ds_cli_work::task::list::render,
    },
    Entry {
        command: &ds_cli_work::task::read::COMMAND,
        handler: ds_cli_work::task::read::run,
        render: ds_cli_work::task::read::render,
    },
    Entry {
        command: &ds_cli_work::task::create::COMMAND,
        handler: ds_cli_work::task::create::run,
        render: ds_cli_work::task::create::render,
    },
    Entry {
        command: &ds_cli_work::task::update::COMMAND,
        handler: ds_cli_work::task::update::run,
        render: ds_cli_work::task::update::render,
    },
    Entry {
        command: &ds_cli_work::task::assign::COMMAND,
        handler: ds_cli_work::task::assign::run,
        render: ds_cli_work::task::assign::render,
    },
    Entry {
        command: &ds_cli_work::task::respond::COMMAND,
        handler: ds_cli_work::task::respond::run,
        render: ds_cli_work::task::respond::render,
    },
    Entry {
        command: &ds_cli_work::record::list::COMMAND,
        handler: ds_cli_work::record::list::run,
        render: ds_cli_work::record::list::render,
    },
    Entry {
        command: &ds_cli_work::record::read::COMMAND,
        handler: ds_cli_work::record::read::run,
        render: ds_cli_work::record::read::render,
    },
];

/// Design collaboration is durable project metadata, not map-owned local
/// state. It is available through a paired desktop without an open map.
static DESIGN_ENTRIES: &[Entry] = &[
    Entry {
        command: &ds_cli_design::features::COMMAND,
        handler: ds_cli_design::features::run,
        render: ds_cli_design::features::render,
    },
    Entry {
        command: &ds_cli_design::selection::list::COMMAND,
        handler: ds_cli_design::selection::list::run,
        render: ds_cli_design::selection::list::render,
    },
    Entry {
        command: &ds_cli_design::selection::read::COMMAND,
        handler: ds_cli_design::selection::read::run,
        render: ds_cli_design::selection::read::render,
    },
    Entry {
        command: &ds_cli_design::selection::save::COMMAND,
        handler: ds_cli_design::selection::save::run,
        render: ds_cli_design::selection::save::render,
    },
    Entry {
        command: &ds_cli_design::selection::archive::COMMAND,
        handler: ds_cli_design::selection::archive::run,
        render: ds_cli_design::selection::archive::render,
    },
    Entry {
        command: &ds_cli_design::selection::assign::COMMAND,
        handler: ds_cli_design::selection::assign::run,
        render: ds_cli_design::selection::assign::render,
    },
    Entry {
        command: &ds_cli_design::attachment::list::COMMAND,
        handler: ds_cli_design::attachment::list::run,
        render: ds_cli_design::attachment::list::render,
    },
    Entry {
        command: &ds_cli_design::attachment::publish::COMMAND,
        handler: ds_cli_design::attachment::publish::run,
        render: ds_cli_design::attachment::publish::render,
    },
    Entry {
        command: &ds_cli_design::attachment::download::COMMAND,
        handler: ds_cli_design::attachment::download::run,
        render: ds_cli_design::attachment::download::render,
    },
    Entry {
        command: &ds_cli_design::attachment::retire::COMMAND,
        handler: ds_cli_design::attachment::retire::run,
        render: ds_cli_design::attachment::retire::render,
    },
    Entry {
        command: &ds_cli_design::tag::list::COMMAND,
        handler: ds_cli_design::tag::list::run,
        render: ds_cli_design::tag::list::render,
    },
    Entry {
        command: &ds_cli_design::tag::query::COMMAND,
        handler: ds_cli_design::tag::query::run,
        render: ds_cli_design::tag::query::render,
    },
    Entry {
        command: &ds_cli_design::tag::define::COMMAND,
        handler: ds_cli_design::tag::define::run,
        render: ds_cli_design::tag::define::render,
    },
    Entry {
        command: &ds_cli_design::tag::set::COMMAND,
        handler: ds_cli_design::tag::set::run,
        render: ds_cli_design::tag::set::render,
    },
    Entry {
        command: &ds_cli_design::tag::enrich::PREVIEW_COMMAND,
        handler: ds_cli_design::tag::enrich::run_preview,
        render: ds_cli_design::tag::enrich::render,
    },
    Entry {
        command: &ds_cli_design::tag::enrich::APPLY_COMMAND,
        handler: ds_cli_design::tag::enrich::run_apply,
        render: ds_cli_design::tag::enrich::render,
    },
    Entry {
        command: &ds_cli_design::known_columns::list::COMMAND,
        handler: ds_cli_design::known_columns::list::run,
        render: ds_cli_design::known_columns::list::render,
    },
    Entry {
        command: &ds_cli_design::known_columns::set::COMMAND,
        handler: ds_cli_design::known_columns::set::run,
        render: ds_cli_design::known_columns::set::render,
    },
    Entry {
        command: &ds_cli_design::group::list::COMMAND,
        handler: ds_cli_design::group::list::run,
        render: ds_cli_design::group::list::render,
    },
    Entry {
        command: &ds_cli_design::group::preview::COMMAND,
        handler: ds_cli_design::group::preview::run,
        render: ds_cli_design::group::preview::render,
    },
    Entry {
        command: &ds_cli_design::group::apply::COMMAND,
        handler: ds_cli_design::group::apply::run,
        render: ds_cli_design::group::apply::render,
    },
    Entry {
        command: &ds_cli_design::group::unassign::COMMAND,
        handler: ds_cli_design::group::unassign::run,
        render: ds_cli_design::group::unassign::render,
    },
    Entry {
        command: &ds_cli_design::group::export::COMMAND,
        handler: ds_cli_design::group::export::run,
        render: ds_cli_design::group::export::render,
    },
    Entry {
        command: &ds_cli_design::grouping::preview::COMMAND,
        handler: ds_cli_design::grouping::preview::run,
        render: ds_cli_design::grouping::preview::render,
    },
    Entry {
        command: &ds_cli_design::grouping::apply::COMMAND,
        handler: ds_cli_design::grouping::apply::run,
        render: ds_cli_design::grouping::apply::render,
    },
    Entry {
        command: &ds_cli_design::grouping::read::READ_COMMAND,
        handler: ds_cli_design::grouping::read::run_read,
        render: ds_cli_design::grouping::read::render,
    },
    Entry {
        command: &ds_cli_design::grouping::read::ARCHIVE_COMMAND,
        handler: ds_cli_design::grouping::read::run_archive,
        render: ds_cli_design::grouping::read::render,
    },
    Entry {
        command: &ds_cli_design::comment::list::COMMAND,
        handler: ds_cli_design::comment::list::run,
        render: ds_cli_design::comment::list::render,
    },
    Entry {
        command: &ds_cli_design::comment::read::COMMAND,
        handler: ds_cli_design::comment::read::run,
        render: ds_cli_design::comment::read::render,
    },
    Entry {
        command: &ds_cli_design::comment::post::COMMAND,
        handler: ds_cli_design::comment::post::run,
        render: ds_cli_design::comment::post::render,
    },
    Entry {
        command: &ds_cli_design::comment::resolve::COMMAND,
        handler: ds_cli_design::comment::resolve::run,
        render: ds_cli_design::comment::resolve::render,
    },
    Entry {
        command: &ds_cli_design::comment::promote::COMMAND,
        handler: ds_cli_design::comment::promote::run,
        render: ds_cli_design::comment::promote::render,
    },
    Entry {
        command: &ds_cli_design::lv::project_export::COMMAND,
        handler: ds_cli_design::lv::project_export::run,
        render: ds_cli_design::lv::project_export::render,
    },
    Entry {
        command: &ds_cli_design::lv::process::COMMAND,
        handler: ds_cli_design::lv::process::run,
        render: ds_cli_design::lv::process::render,
    },
    Entry {
        command: &ds_cli_design::transformer::inventory::COMMAND,
        handler: ds_cli_design::transformer::inventory::run,
        render: ds_cli_design::transformer::inventory::render,
    },
    Entry {
        command: &ds_cli_design::transformer::retire::COMMAND,
        handler: ds_cli_design::transformer::retire::run,
        render: ds_cli_design::transformer::retire::render,
    },
    Entry {
        command: &ds_cli_design::transformer::restore::COMMAND,
        handler: ds_cli_design::transformer::restore::run,
        render: ds_cli_design::transformer::restore::render,
    },
];

static SRE_ENTRIES: &[Entry] = &[
    Entry {
        command: &ds_cli_sre::overview::COMMAND,
        handler: ds_cli_sre::overview::run,
        render: ds_cli_sre::overview::render,
    },
    Entry {
        command: &ds_cli_sre::events::COMMAND,
        handler: ds_cli_sre::events::run,
        render: ds_cli_sre::events::render,
    },
];

static DESKTOP_ENTRIES: &[Entry] = &[
    Entry {
        command: &ds_cli_desktop::status::COMMAND,
        handler: ds_cli_desktop::status::run,
        render: ds_cli_desktop::status::render,
    },
    Entry {
        command: &ds_cli_desktop::project::LIST_COMMAND,
        handler: ds_cli_desktop::project::list,
        render: ds_cli_desktop::project::render_list,
    },
    Entry {
        command: &ds_cli_desktop::project::SWITCH_COMMAND,
        handler: ds_cli_desktop::project::switch,
        render: ds_cli_desktop::project::render_switch,
    },
];

/// Feedback is a loop: report a gap, find it again once a session has fixed
/// it, and close it with what changed. The order is the loop.
static FEEDBACK_ENTRIES: &[Entry] = &[
    Entry {
        command: &ds_cli_feedback::submit::COMMAND,
        handler: ds_cli_feedback::submit::run,
        render: ds_cli_feedback::submit::render,
    },
    Entry {
        command: &ds_cli_feedback::list::COMMAND,
        handler: ds_cli_feedback::list::run,
        render: ds_cli_feedback::list::render,
    },
    Entry {
        command: &ds_cli_feedback::close::COMMAND,
        handler: ds_cli_feedback::close::run,
        render: ds_cli_feedback::close::render,
    },
];

/// The shell domain lists its commands in the order a person needs them:
/// look, register, and — rarely — undo.
static SHELL_ENTRIES: &[Entry] = &[
    Entry {
        command: &ds_cli_shell::status::COMMAND,
        handler: ds_cli_shell::status::run,
        render: ds_cli_shell::status::render,
    },
    Entry {
        command: &ds_cli_shell::register::COMMAND,
        handler: ds_cli_shell::register::run,
        render: ds_cli_shell::register::render,
    },
    Entry {
        command: &ds_cli_shell::unregister::COMMAND,
        handler: ds_cli_shell::unregister::run,
        render: ds_cli_shell::unregister::render,
    },
];

/// One setup door: inspect and plan before either proven machine mutation.
static WORKSTATION_ENTRIES: &[Entry] = &[
    Entry {
        command: &ds_cli_workstation::status::COMMAND,
        handler: ds_cli_workstation::status::run,
        render: ds_cli_workstation::status::render,
    },
    Entry {
        command: &ds_cli_workstation::plan::COMMAND,
        handler: ds_cli_workstation::plan::run,
        render: ds_cli_workstation::plan::render,
    },
    Entry {
        command: &ds_cli_workstation::install::COMMAND,
        handler: ds_cli_workstation::install::run,
        render: ds_cli_workstation::install::render,
    },
    Entry {
        command: &ds_cli_workstation::configure::COMMAND,
        handler: ds_cli_workstation::configure::run,
        render: ds_cli_workstation::configure::render,
    },
    Entry {
        command: &ds_cli_workstation::verify::COMMAND,
        handler: ds_cli_workstation::verify::run,
        render: ds_cli_workstation::verify::render,
    },
    Entry {
        command: &ds_cli_workstation::components::COMMAND,
        handler: ds_cli_workstation::components::run,
        render: ds_cli_workstation::components::render,
    },
];

static MCP_ENTRIES: &[Entry] = &[
    Entry {
        command: &ds_cli_mcp::serve::COMMAND,
        handler: ds_cli_mcp::serve::run,
        render: ds_cli_mcp::serve::render,
    },
    Entry {
        command: &ds_cli_mcp::install::COMMAND,
        handler: ds_cli_mcp::install::run,
        render: ds_cli_mcp::install::render,
    },
];

static DOMAINS: &[Registered] = &[
    Registered {
        domain: &ds_cli_dsgrid::DOMAIN,
        entries: DSGRID_ENTRIES,
    },
    Registered {
        domain: &ds_cli_dsgrid_exchange::DOMAIN,
        entries: DSGRID_EXCHANGE_ENTRIES,
    },
    Registered {
        domain: &ds_cli_library::DOMAIN,
        entries: LIBRARY_ENTRIES,
    },
    Registered {
        domain: &ds_cli_pls::DOMAIN,
        entries: PLS_ENTRIES,
    },
    Registered {
        domain: &ds_cli_solar::DOMAIN,
        entries: SOLAR_ENTRIES,
    },
    Registered {
        domain: &ds_cli_report::DOMAIN,
        entries: REPORT_ENTRIES,
    },
    Registered {
        domain: &ds_cli_survey::DOMAIN,
        entries: SURVEY_ENTRIES,
    },
    Registered {
        domain: &ds_cli_map::DOMAIN,
        entries: MAP_ENTRIES,
    },
    Registered {
        domain: &ds_cli_work::DOMAIN,
        entries: WORK_ENTRIES,
    },
    Registered {
        domain: &ds_cli_design::DOMAIN,
        entries: DESIGN_ENTRIES,
    },
    Registered {
        domain: &ds_cli_sre::DOMAIN,
        entries: SRE_ENTRIES,
    },
    Registered {
        domain: &ds_cli_style::DOMAIN,
        entries: STYLE_ENTRIES,
    },
    Registered {
        domain: &ds_cli_tile::DOMAIN,
        entries: TILE_ENTRIES,
    },
    Registered {
        domain: &ds_cli_data::DOMAIN,
        entries: DATA_ENTRIES,
    },
    Registered {
        domain: &ds_cli_feedback::DOMAIN,
        entries: FEEDBACK_ENTRIES,
    },
    Registered {
        domain: &ds_cli_desktop::DOMAIN,
        entries: DESKTOP_ENTRIES,
    },
    Registered {
        domain: &ds_cli_shell::DOMAIN,
        entries: SHELL_ENTRIES,
    },
    Registered {
        domain: &ds_cli_workstation::DOMAIN,
        entries: WORKSTATION_ENTRIES,
    },
    Registered {
        domain: &ds_cli_mcp::DOMAIN,
        entries: MCP_ENTRIES,
    },
    Registered {
        domain: &ds_cli_auth::DOMAIN,
        entries: AUTH_ENTRIES,
    },
];

pub fn domains() -> &'static [Registered] {
    DOMAINS
}

/// The root-level commands that describe the CLI itself. They are separate
/// from domains because they are not a subject area — `ds capabilities` is
/// not a kind of engineering work — and because they must stay reachable when
/// every domain is unavailable.
pub fn meta_commands() -> &'static [Entry] {
    meta::ENTRIES
}

pub fn find_domain(id: &str) -> Option<&'static Registered> {
    DOMAINS.iter().find(|registered| registered.domain.id == id)
}

/// Resolve a command from an invocation path, longest match first.
///
/// Returns the entry and how many tokens it consumed, so the caller knows
/// where the command's own arguments begin. Longest-first matters because a
/// domain may hold both `ds solar run` and a future `ds solar run status`:
/// resolving the shorter path first would make the longer one unreachable
/// and silently feed `status` to the wrong command as an operand.
pub fn find_by_path(tokens: &[String]) -> Option<(&'static Entry, usize)> {
    let mut best: Option<(&'static Entry, usize)> = None;
    for registered in DOMAINS {
        for entry in registered.entries {
            let path = entry.command.path;
            if path.len() > tokens.len() {
                continue;
            }
            if !path.iter().zip(tokens).all(|(part, token)| *part == token) {
                continue;
            }
            if best.is_none_or(|(_, consumed)| path.len() > consumed) {
                best = Some((entry, path.len()));
            }
        }
    }
    best
}

/// Resolve a dotted command id such as `dsgrid.inspect`.
pub fn find_by_id(id: &str) -> Option<&'static Entry> {
    DOMAINS
        .iter()
        .flat_map(|registered| registered.entries.iter())
        .chain(meta_commands().iter())
        .find(|entry| entry.command.id == id)
}

/// Every registered command, in registration order. Used by capability search
/// and by the contract tests that hold the whole surface to its rules.
pub fn all_commands() -> Vec<&'static Command> {
    DOMAINS
        .iter()
        .flat_map(|registered| registered.entries.iter())
        .chain(meta_commands().iter())
        .map(|entry| entry.command)
        .collect()
}

/// One central identity decision for commands that borrow paired user/map
/// authority. Production observation stays outside this pure comparison: the
/// registry obtains one non-secret protected-state probe and one paired
/// session snapshot, then every eligible command passes through this gate.
#[cfg(test)]
fn enforce_provider_identity(
    command: &Command,
    headless: Option<(&ds_cli_auth::ProviderIdentity, Option<&str>)>,
    desktop: Option<(&ds_cli_auth::ProviderIdentity, Option<&str>)>,
) -> Result<(), Failure> {
    if command.id == "auth.link.approve"
        || !matches!(
            command.authority,
            ds_cli_contract::Authority::DesktopUser | ds_cli_contract::Authority::Project
        )
    {
        return Ok(());
    }
    let Some((headless_identity, headless_project)) = headless else {
        // An already-authorized map must not require a separate CLI login.
        return Ok(());
    };
    let Some((desktop_identity, desktop_project)) = desktop else {
        // The ordinary paired-authority handler owns the typed not-paired
        // refusal; there is no mismatched identity to classify here.
        return Ok(());
    };
    let projects = if command.authority == ds_cli_contract::Authority::Project {
        (headless_project, desktop_project)
    } else {
        (None, None)
    };
    ds_cli_auth::arbitrate_provider(
        headless_identity,
        desktop_identity,
        ds_cli_auth::ProviderTarget::MapAttached,
        projects.0,
        projects.1,
    )?;
    Ok(())
}

/// Run one entry: parse its declared inputs, enforce the confirmation policy
/// its effect class implies, then hand off. Confirmation is checked here, in
/// one place, so a handler cannot forget it.
pub fn dispatch(entry: &Entry, tokens: &[String], context: &Context) -> Result<Value, Failure> {
    let inputs: Inputs = ds_cli_contract::parse(entry.command, tokens)?;

    // A machine-write command may expose one declared `--write` switch for a
    // read-only preview. The declaration remains machine_write, but the gate
    // is required only when that switch selects the writing path. Parsing
    // still happens first and the policy stays centralized here.
    let confirmation_required = entry.command.confirmation_required_for(&inputs);
    if confirmation_required && !context.confirmed {
        return Err(Failure::invalid(
            "confirmation_required",
            format!(
                "`ds {}` {} and needs explicit confirmation",
                entry.command.path.join(" "),
                entry.command.effect.gloss()
            ),
        )
        .remedy("re-run with --yes once you intend the effect")
        .next(format!("ds {} --help", entry.command.path.join(" "))));
    }

    if let ds_cli_contract::Availability::Unavailable {
        code,
        reason,
        remedy,
    } = (entry.command.availability)()
    {
        return Err(Failure::unavailable(code, reason)
            .remedy(remedy)
            .next(format!("ds {} --help", entry.command.path.join(" "))));
    }

    // Scope the non-network protected-provider observation. The shared
    // Desktop bridge seam consumes it only for an actual map-backed call,
    // snapshots the current map session, and sends the atomic fence beside
    // domain arguments. Pure backend commands remain Desktop-independent.
    let _headless_identity = scope_headless_identity(entry.command, &inputs)?;

    (entry.handler)(&inputs, context)
}

fn scope_headless_identity(
    command: &Command,
    inputs: &Inputs,
) -> Result<ds_cli_desktop::ops::HeadlessIdentityGuard, Failure> {
    // Pure backend commands carry this observation through dispatch without
    // probing or requiring Desktop. If (and only if) a handler reaches the
    // shared typed bridge seam, `ops::invoke` snapshots the map, arbitrates,
    // and sends an atomic invocation fence.
    if !matches!(
        command.authority,
        ds_cli_contract::Authority::DesktopUser | ds_cli_contract::Authority::Project
    ) || command.id == "auth.link.approve"
    {
        return Ok(ds_cli_desktop::ops::scope_headless_identity(None));
    }
    let observed =
        match ds_cli_auth::probe_headless_identity(inputs.value("lane").unwrap_or("stable")) {
            Ok(observed) => observed,
            Err(error) if headless_probe_means_absent(&error) => None,
            Err(error) => return Err(error),
        };
    let observed = observed.map(
        |(identity, project)| ds_cli_desktop::ops::HeadlessIdentity {
            uid: identity.uid().to_owned(),
            lane: identity.lane().to_owned(),
            credential_audience_sha256: identity.credential_audience_sha256().to_owned(),
            project,
            command_authority: command.authority,
        },
    );
    Ok(ds_cli_desktop::ops::scope_headless_identity(observed))
}

fn headless_probe_means_absent(error: &Failure) -> bool {
    error.code() == "native_profile_not_configured"
        || error.code() == "native_state_protection_unavailable"
}

#[cfg(test)]
mod identity_preflight_tests {
    use super::*;

    fn identity(lane: &str, audience: char, uid: &str) -> ds_cli_auth::ProviderIdentity {
        ds_cli_auth::ProviderIdentity::new(lane, &audience.to_string().repeat(64), uid).unwrap()
    }

    #[test]
    fn windows_without_native_store_still_allows_map_only_authority() {
        let missing_adapter = Failure::unavailable(
            "native_state_protection_unavailable",
            "Windows adapter absent",
        );
        assert!(headless_probe_means_absent(&missing_adapter));
        let malformed = Failure::unavailable("native_state_unsafe", "malformed state");
        assert!(!headless_probe_means_absent(&malformed));
        let mismatched_profile =
            Failure::unavailable("native_profile_digest_mismatch", "mismatched profile");
        assert!(!headless_probe_means_absent(&mismatched_profile));
    }

    #[test]
    fn central_preflight_allows_map_only_and_blocks_mismatched_maphead() {
        let desktop_user = all_commands()
            .into_iter()
            .find(|command| {
                command.authority == ds_cli_contract::Authority::DesktopUser
                    && command.id != "auth.link.approve"
            })
            .expect("one DesktopUser command");
        let headless = identity("stable", 'a', "uid-a");
        let exact = identity("stable", 'a', "uid-a");
        let wrong_uid = identity("stable", 'a', "uid-b");

        enforce_provider_identity(desktop_user, None, Some((&exact, Some("map-project"))))
            .expect("map-only authority needs no second login");
        enforce_provider_identity(desktop_user, Some((&headless, None)), Some((&exact, None)))
            .expect("exact identity uses the paired provider first");
        assert_eq!(
            enforce_provider_identity(
                desktop_user,
                Some((&headless, None)),
                Some((&wrong_uid, None)),
            )
            .unwrap_err()
            .code(),
            "auth_context_mismatch"
        );
    }

    #[test]
    fn project_preflight_requires_equality_only_when_both_selections_exist() {
        let project = all_commands()
            .into_iter()
            .find(|command| command.authority == ds_cli_contract::Authority::Project)
            .expect("one legacy Project command");
        let exact = identity("stable", 'a', "uid-a");
        enforce_provider_identity(
            project,
            Some((&exact, None)),
            Some((&exact, Some("map-project"))),
        )
        .expect("the authorized map supplies its project when headless has none");
        assert_eq!(
            enforce_provider_identity(
                project,
                Some((&exact, Some("headless-project"))),
                Some((&exact, Some("map-project"))),
            )
            .unwrap_err()
            .code(),
            "auth_context_mismatch"
        );

        let approval = find_by_id("auth.link.approve").unwrap().command;
        let other = identity("stable", 'a', "uid-b");
        enforce_provider_identity(approval, Some((&exact, None)), Some((&other, None)))
            .expect("device approval is explicitly outside map/headless arbitration");
    }
}
