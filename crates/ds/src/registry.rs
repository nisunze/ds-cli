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

// The one sign-in a person is asked to do. Listed before `auth` because it
// is the step every signed-out refusal names; `auth` keeps the contracts it
// composes and the trusted-terminal commands.
static ACCOUNT_ENTRIES: &[Entry] = &[Entry {
    command: &ds_cli_auth::account::CONNECT_COMMAND,
    handler: ds_cli_auth::account::run_connect,
    render: ds_cli_auth::account::render_connect,
}];

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
    Entry {
        command: &ds_cli_auth::PROJECT_CREATE_COMMAND,
        handler: ds_cli_auth::run_project_create,
        render: ds_cli_auth::render_project_properties,
    },
    Entry {
        command: &ds_cli_auth::PROJECT_UPDATE_COMMAND,
        handler: ds_cli_auth::run_project_update,
        render: ds_cli_auth::render_project_properties,
    },
];

/// Every domain, in root-help order. Static because the table is the
/// interface: it is walked by dispatch, by help and by the contract tests,
/// and all three must be looking at the same thing.

/// The exchange domain lists its commands in the order they are meant to be
/// called: classify, then plan, then convert. Domain help prints this order
/// verbatim, so the index doubles as the procedure — a reader who works down
/// the list is following the safe sequence rather than reconstructing it.



static REPORT_ENTRIES: &[Entry] = &[
    Entry {
        command: &ds_cli_report::planning::TRANSFORMERS,
        handler: ds_cli_report::planning::transformers,
        render: ds_cli_report::layout::render_text,
    },
    Entry {
        command: &ds_cli_report::planning::PLAN,
        handler: ds_cli_report::planning::plan,
        render: ds_cli_report::layout::render_text,
    },
    Entry {
        command: &ds_cli_report::layout::NEW,
        handler: ds_cli_report::layout::new,
        render: ds_cli_report::layout::render_text,
    },
    Entry {
        command: &ds_cli_report::layout::EDIT,
        handler: ds_cli_report::layout::edit,
        render: ds_cli_report::layout::render_text,
    },
    Entry {
        command: &ds_cli_report::layout::CONTEXT,
        handler: ds_cli_report::layout::context,
        render: ds_cli_report::layout::render_text,
    },
    Entry {
        command: &ds_cli_report::layout::SESSION,
        handler: ds_cli_report::layout::session,
        render: ds_cli_report::layout::render_text,
    },
    Entry {
        command: &ds_cli_report::layout::ADD,
        handler: ds_cli_report::layout::add,
        render: ds_cli_report::layout::render_text,
    },
    Entry {
        command: &ds_cli_report::layout::STYLE_REF_COMMAND,
        handler: ds_cli_report::layout::style_ref,
        render: ds_cli_report::layout::render_text,
    },
    Entry {
        command: &ds_cli_report::layout::PENS,
        handler: ds_cli_report::layout::pens,
        render: ds_cli_report::layout::render_text,
    },
    Entry {
        command: &ds_cli_report::layout::SCHEMA,
        handler: ds_cli_report::layout::schema,
        render: ds_cli_report::layout::render_text,
    },
    Entry {
        command: &ds_cli_report::layout::RENDER,
        handler: ds_cli_report::layout::render,
        render: ds_cli_report::layout::render_text,
    },
    Entry {
        command: &ds_cli_report::layout::LIST,
        handler: ds_cli_report::layout::list,
        render: ds_cli_report::layout::render_text,
    },
    Entry {
        command: &ds_cli_report::layout::GET,
        handler: ds_cli_report::layout::get,
        render: ds_cli_report::layout::render_text,
    },
    Entry {
        command: &ds_cli_report::layout::CREATE,
        handler: ds_cli_report::layout::create,
        render: ds_cli_report::layout::render_text,
    },
    Entry {
        command: &ds_cli_report::layout::UPDATE,
        handler: ds_cli_report::layout::update,
        render: ds_cli_report::layout::render_text,
    },
    Entry {
        command: &ds_cli_report::layout::SAVE,
        handler: ds_cli_report::layout::save,
        render: ds_cli_report::layout::render_text,
    },
    Entry {
        command: &ds_cli_report::layout::DELETE,
        handler: ds_cli_report::layout::delete,
        render: ds_cli_report::layout::render_text,
    },
    Entry {
        command: &ds_cli_report::layout::COPY,
        handler: ds_cli_report::layout::copy,
        render: ds_cli_report::layout::render_text,
    },
    Entry {
        command: &ds_cli_report::standard::LIST,
        handler: ds_cli_report::standard::list,
        render: ds_cli_report::standard::render,
    },
    Entry {
        command: &ds_cli_report::standard::GET,
        handler: ds_cli_report::standard::get,
        render: ds_cli_report::standard::render,
    },
    Entry {
        command: &ds_cli_report::style_keys::PLAN,
        handler: ds_cli_report::style_keys::plan,
        render: ds_cli_report::style_keys::render,
    },
    Entry {
        command: &ds_cli_report::style_keys::APPLY,
        handler: ds_cli_report::style_keys::apply,
        render: ds_cli_report::style_keys::render,
    },
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
        command: &ds_cli_report::voltage_drop_combined::COMMAND,
        handler: ds_cli_report::voltage_drop_combined::run,
        render: ds_cli_report::voltage_drop_combined::render,
    },
    Entry {
        command: &ds_cli_report::mv_frontmatter::COMMAND,
        handler: ds_cli_report::mv_frontmatter::run,
        render: ds_cli_report::mv_frontmatter::render,
    },
    Entry {
        command: &ds_cli_report::plan_profile::COMMAND,
        handler: ds_cli_report::plan_profile::run,
        render: ds_cli_report::plan_profile::render,
    },
    Entry {
        command: &ds_cli_report::plan_profile_config::COMMAND,
        handler: ds_cli_report::plan_profile_config::run,
        render: ds_cli_report::plan_profile_config::render,
    },
    Entry {
        command: &ds_cli_report::plan_profile_config::SCHEMA,
        handler: ds_cli_report::plan_profile_config::schema_run,
        render: ds_cli_report::plan_profile_config::schema_render,
    },
    Entry {
        command: &ds_cli_report::spatial_workbook::COMMAND,
        handler: ds_cli_report::spatial_workbook::run,
        render: ds_cli_report::spatial_workbook::render,
    },
    Entry {
        command: &ds_cli_report::project::scope::COMMAND,
        handler: ds_cli_report::project::scope::run,
        render: ds_cli_report::project::scope::render,
    },
    Entry {
        command: &ds_cli_report::project::combined::COMMAND,
        handler: ds_cli_report::project::combined::run,
        render: ds_cli_report::project::combined::render,
    },
    Entry {
        command: &ds_cli_report::outbox::STATUS,
        handler: ds_cli_report::outbox::status,
        render: ds_cli_report::outbox::render,
    },
    Entry {
        command: &ds_cli_report::outbox::INVENTORY,
        handler: ds_cli_report::outbox::inventory,
        render: ds_cli_report::outbox::render_inventory,
    },
    Entry {
        command: &ds_cli_report::outbox::DRAIN,
        handler: ds_cli_report::outbox::drain,
        render: ds_cli_report::outbox::render,
    },
    Entry {
        command: &ds_cli_report::publication::LIST,
        handler: ds_cli_report::publication::list,
        render: ds_cli_report::publication::render,
    },
    Entry {
        command: &ds_cli_report::publication::SHOW,
        handler: ds_cli_report::publication::show,
        render: ds_cli_report::publication::render,
    },
    Entry {
        command: &ds_cli_report::retained_preview::READ,
        handler: ds_cli_report::retained_preview::read,
        render: ds_cli_report::retained_preview::render,
    },
    Entry {
        command: &ds_cli_report::retained_preview::REFRESH,
        handler: ds_cli_report::retained_preview::refresh,
        render: ds_cli_report::retained_preview::render,
    },
    Entry {
        command: &ds_cli_report::project::compute::COMMAND,
        handler: ds_cli_report::project::compute::run,
        render: ds_cli_report::project::compute::render,
    },
    Entry {
        command: &ds_cli_report::project::archives::COMMAND,
        handler: ds_cli_report::project::archives::run,
        render: ds_cli_report::project::archives::render,
    },
    Entry {
        command: &ds_cli_report::project::crs::COMMAND,
        handler: ds_cli_report::project::crs::run,
        render: ds_cli_report::project::crs::render,
    },
    Entry {
        command: &ds_cli_report::project::settings::COMMAND,
        handler: ds_cli_report::project::settings::run,
        render: ds_cli_report::project::settings::render,
    },
    Entry {
        command: &ds_cli_report::project::mv_setup::RESOLVE,
        handler: ds_cli_report::project::mv_setup::resolve,
        render: ds_cli_report::project::mv_setup::render_resolve,
    },
    Entry {
        command: &ds_cli_report::project::mv_setup::SET,
        handler: ds_cli_report::project::mv_setup::set,
        render: ds_cli_report::project::mv_setup::render,
    },
    Entry {
        command: &ds_cli_report::project::settings::OUTPUTS_SET,
        handler: ds_cli_report::project::settings::set,
        render: ds_cli_report::project::settings::render_set,
    },
    Entry {
        command: &ds_cli_report::project::export::COMMAND,
        handler: ds_cli_report::project::export::run,
        render: ds_cli_report::project::export::render,
    },
    Entry {
        command: &ds_cli_report::project::publish::COMMAND,
        handler: ds_cli_report::project::publish::run,
        render: ds_cli_report::project::publish::render,
    },
    Entry {
        command: &ds_cli_report::project::map_inputs::COMMAND,
        handler: ds_cli_report::project::map_inputs::run,
        render: ds_cli_report::layout::render_text,
    },
    Entry {
        command: &ds_cli_report::artifact::REMOVE,
        handler: ds_cli_report::artifact::remove,
        render: ds_cli_report::layout::render_text,
    },
];

static SOLAR_ENTRIES: &[Entry] = &[
    Entry {
        command: &ds_cli_solar::calculate_prepared::COMMAND,
        handler: ds_cli_solar::calculate_prepared::run,
        render: ds_cli_solar::calculate_prepared::render,
    },
    Entry {
        command: &ds_cli_solar::application::SCHEMA,
        handler: ds_cli_solar::application::schema,
        render: ds_cli_solar::application::render,
    },
    Entry {
        command: &ds_cli_solar::application::COMMAND,
        handler: ds_cli_solar::application::execute,
        render: ds_cli_solar::application::render,
    },
    Entry {
        command: &ds_cli_solar::network_map::COMMAND,
        handler: ds_cli_solar::network_map::run,
        render: ds_cli_solar::network_form::render,
    },
    Entry {
        command: &ds_cli_solar::network_form::COMMAND,
        handler: ds_cli_solar::network_form::run,
        render: ds_cli_solar::network_form::render,
    },
    Entry {
        command: &ds_cli_solar::network_form::SAVE,
        handler: ds_cli_solar::network_form::save,
        render: ds_cli_solar::network_form::render,
    },
    Entry {
        command: &ds_cli_solar::reference::PLAN,
        handler: ds_cli_solar::reference::plan,
        render: ds_cli_solar::reference::render,
    },
    Entry {
        command: &ds_cli_solar::reference::IMPORT,
        handler: ds_cli_solar::reference::import,
        render: ds_cli_solar::reference::render,
    },
    Entry {
        command: &ds_cli_solar::cities::COMMAND,
        handler: ds_cli_solar::cities::run,
        render: ds_cli_solar::cities::render,
    },
    Entry {
        command: &ds_cli_solar::project::REBASE,
        handler: ds_cli_solar::project::rebase,
        render: ds_cli_solar::project::render,
    },
    Entry {
        command: &ds_cli_solar::project::CITY_CREATE,
        handler: ds_cli_solar::project::city_create,
        render: ds_cli_solar::project::render,
    },
    Entry {
        command: &ds_cli_solar::project::CITY_READ,
        handler: ds_cli_solar::project::city_read,
        render: ds_cli_solar::project::render,
    },
    Entry {
        command: &ds_cli_solar::project::CITY_WRITE,
        handler: ds_cli_solar::project::city_write,
        render: ds_cli_solar::project::render,
    },
    Entry {
        command: &ds_cli_solar::project_sync::COMMAND,
        handler: ds_cli_solar::project_sync::run,
        render: ds_cli_solar::project_sync::render_sync,
    },
    Entry {
        command: &ds_cli_solar::project::INIT,
        handler: ds_cli_solar::project::init,
        render: ds_cli_solar::project::render,
    },
    Entry {
        command: &ds_cli_solar::project::SEED,
        handler: ds_cli_solar::project::seed,
        render: ds_cli_solar::project::render,
    },
    Entry {
        command: &ds_cli_solar::project::RUN,
        handler: ds_cli_solar::project::run,
        render: ds_cli_solar::project::render,
    },
    Entry {
        command: &ds_cli_solar::project::STATUS,
        handler: ds_cli_solar::project::status,
        render: ds_cli_solar::project::render,
    },
    Entry {
        command: &ds_cli_solar::project::RESULT,
        handler: ds_cli_solar::project::result,
        render: ds_cli_solar::project::render,
    },
    Entry {
        command: &ds_cli_solar::project::OUTBOX,
        handler: ds_cli_solar::project::outbox,
        render: ds_cli_solar::project::render,
    },
    Entry {
        command: &ds_cli_solar::project::CLEAN,
        handler: ds_cli_solar::project::clean,
        render: ds_cli_solar::project::render,
    },
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
        command: &ds_cli_solar::dashboard::COMMAND,
        handler: ds_cli_solar::dashboard::execute,
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
        command: &ds_cli_solar::portfolio_batch::START_COMMAND,
        handler: ds_cli_solar::portfolio_batch::start,
        render: ds_cli_solar::workflow::render,
    },
    Entry {
        command: &ds_cli_solar::portfolio_batch::STATUS_COMMAND,
        handler: ds_cli_solar::portfolio_batch::status,
        render: ds_cli_solar::workflow::render,
    },
    Entry {
        command: &ds_cli_solar::portfolio_batch::CANCEL_COMMAND,
        handler: ds_cli_solar::portfolio_batch::cancel,
        render: ds_cli_solar::workflow::render,
    },
    Entry {
        command: &ds_cli_solar::portfolio_headless::CALCULATE,
        handler: ds_cli_solar::portfolio_headless::calculate,
        render: ds_cli_solar::workflow::render,
    },
    Entry {
        command: &ds_cli_solar::portfolio_headless::PUBLISHED_READ,
        handler: ds_cli_solar::portfolio_headless::published_read,
        render: ds_cli_solar::portfolio_headless::render_publication,
    },
    Entry {
        command: &ds_cli_solar::portfolio_management::CREATE_COMMAND,
        handler: ds_cli_solar::portfolio_management::create,
        render: ds_cli_solar::workflow::render,
    },
    Entry {
        command: &ds_cli_solar::portfolio_management::UPDATE_COMMAND,
        handler: ds_cli_solar::portfolio_management::update,
        render: ds_cli_solar::workflow::render,
    },
    Entry {
        command: &ds_cli_solar::portfolio_management::DELETE_COMMAND,
        handler: ds_cli_solar::portfolio_management::delete,
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
        command: &ds_cli_solar::exports::REPORT_BUNDLE_COMMAND,
        handler: ds_cli_solar::exports::export_report_bundle,
        render: ds_cli_solar::exports::render,
    },
    Entry {
        command: &ds_cli_solar::exports::PORTFOLIO_EXPORT_COMMAND,
        handler: ds_cli_solar::exports::export_portfolio,
        render: ds_cli_solar::exports::render,
    },
    Entry {
        command: &ds_cli_solar::migrate::PLAN_COMMAND,
        handler: ds_cli_solar::migrate::plan,
        render: ds_cli_solar::migrate::render,
    },
    Entry {
        command: &ds_cli_solar::migrate::APPLY_COMMAND,
        handler: ds_cli_solar::migrate::apply,
        render: ds_cli_solar::migrate::render,
    },
    Entry {
        command: &ds_cli_solar::network_seed::COMMAND,
        handler: ds_cli_solar::network_seed::run,
        render: ds_cli_solar::network_seed::render,
    },
    Entry {
        command: &ds_cli_solar::weather::COMMAND,
        handler: ds_cli_solar::weather::run,
        render: ds_cli_solar::weather::render,
    },
];

static MAP_ENTRIES: &[Entry] = &[
    Entry {
        command: &ds_cli_map::scene::COMMAND,
        handler: ds_cli_map::scene::run,
        render: ds_cli_map::scene::render,
    },
    Entry {
        command: &ds_cli_map::print_schema::COMMAND,
        handler: ds_cli_map::print_schema::run,
        render: ds_cli_map::print_schema::render,
    },
    Entry {
        command: &ds_cli_map::view::COMMAND,
        handler: ds_cli_map::view::run,
        render: ds_cli_map::view::render,
    },
    Entry {
        command: &ds_cli_map::camera_set::COMMAND,
        handler: ds_cli_map::camera_set::run,
        render: ds_cli_map::camera_set::render,
    },
    Entry {
        command: &ds_cli_map::profile::VIEW,
        handler: ds_cli_map::profile::view,
        render: ds_cli_map::profile::render,
    },
    Entry {
        command: &ds_cli_map::profile::SET,
        handler: ds_cli_map::profile::set,
        render: ds_cli_map::profile::render,
    },
    Entry {
        command: &ds_cli_map::profile::SELECT,
        handler: ds_cli_map::profile::select,
        render: ds_cli_map::profile::render_selection,
    },
    Entry {
        command: &ds_cli_map::profile_pick::COMMAND,
        handler: ds_cli_map::profile_pick::run,
        render: ds_cli_map::profile::render,
    },
    Entry {
        command: &ds_cli_map::profile_model::USAGE,
        handler: ds_cli_map::profile_model::usage,
        render: ds_cli_map::profile::render,
    },
    Entry {
        command: &ds_cli_map::profile_results::COMMAND,
        handler: ds_cli_map::profile_results::run,
        render: ds_cli_map::profile::render,
    },
    Entry {
        command: &ds_cli_map::profile_calculation::COMMAND,
        handler: ds_cli_map::profile_calculation::run,
        render: ds_cli_map::profile::render,
    },
    Entry {
        command: &ds_cli_map::profile_edit::COMMAND,
        handler: ds_cli_map::profile_edit::run,
        render: ds_cli_map::profile::render,
    },
    Entry {
        command: &ds_cli_map::profile_save::COMMAND,
        handler: ds_cli_map::profile_save::run,
        render: ds_cli_map::profile::render,
    },
    Entry {
        command: &ds_cli_map::profile_model::FILTER,
        handler: ds_cli_map::profile_model::filter,
        render: ds_cli_map::profile::render,
    },
    Entry {
        command: &ds_cli_map::profile_model::ISSUES,
        handler: ds_cli_map::profile_model::issues,
        render: ds_cli_map::profile::render,
    },
    Entry {
        command: &ds_cli_map::profile_model::RETYPE,
        handler: ds_cli_map::profile_model::retype,
        render: ds_cli_map::profile::render,
    },
    Entry {
        command: &ds_cli_map::grid_lasso::COMMAND,
        handler: ds_cli_map::grid_lasso::run,
        render: ds_cli_map::grid_lasso::render,
    },
    Entry {
        command: &ds_cli_map::renderer_configure::COMMAND,
        handler: ds_cli_map::renderer_configure::run,
        render: ds_cli_map::renderer_configure::render,
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
        command: &ds_cli_map::layer::default_visibility::COMMAND,
        handler: ds_cli_map::layer::default_visibility::run,
        render: ds_cli_map::layer::default_visibility::render,
    },
    Entry {
        command: &ds_cli_map::layer::reorder::COMMAND,
        handler: ds_cli_map::layer::reorder::run,
        render: ds_cli_map::layer::reorder::render,
    },
    Entry {
        command: &ds_cli_map::layer::registry::READ,
        handler: ds_cli_map::layer::registry::read,
        render: ds_cli_map::layer::registry::render,
    },
    Entry {
        command: &ds_cli_map::layer::registry::EDIT,
        handler: ds_cli_map::layer::registry::edit,
        render: ds_cli_map::layer::registry::render,
    },
    Entry {
        command: &ds_cli_map::layer::remote_list::COMMAND,
        handler: ds_cli_map::layer::remote_list::run,
        render: ds_cli_map::layer::remote_list::render,
    },
    Entry {
        command: &ds_cli_map::data::remove::COMMAND,
        handler: ds_cli_map::data::remove::run,
        render: ds_cli_map::data::remove::render,
    },
    Entry {
        command: &ds_cli_map::data::list::COMMAND,
        handler: ds_cli_map::data::list::run,
        render: ds_cli_map::data::list::render,
    },
    Entry {
        command: &ds_cli_map::data::upload::COMMAND,
        handler: ds_cli_map::data::upload::run,
        render: ds_cli_map::data::upload::render,
    },
    Entry {
        command: &ds_cli_map::data::inspect::COMMAND,
        handler: ds_cli_map::data::inspect::run,
        render: ds_cli_map::data::inspect::render,
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
        command: &ds_cli_map::layer::project_visibility::SHOW,
        handler: ds_cli_map::layer::project_visibility::run_show,
        render: ds_cli_map::layer::project_visibility::render,
    },
    Entry {
        command: &ds_cli_map::layer::project_visibility::HIDE,
        handler: ds_cli_map::layer::project_visibility::run_hide,
        render: ds_cli_map::layer::project_visibility::render,
    },
    Entry {
        command: &ds_cli_map::local::list::COMMAND,
        handler: ds_cli_map::local::list::run,
        render: ds_cli_map::local::list::render,
    },
    Entry {
        command: &ds_cli_map::local::register::COMMAND,
        handler: ds_cli_map::local::register::run,
        render: ds_cli_map::local::register::render,
    },
    Entry {
        command: &ds_cli_map::local::rename::COMMAND,
        handler: ds_cli_map::local::rename::run,
        render: ds_cli_map::local::rename::render,
    },
    Entry {
        command: &ds_cli_map::local::remove::COMMAND,
        handler: ds_cli_map::local::remove::run,
        render: ds_cli_map::local::remove::render,
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
        command: &ds_cli_survey::update::PLAN,
        handler: ds_cli_survey::update::plan,
        render: ds_cli_survey::update::render,
    },
    Entry {
        command: &ds_cli_survey::update::COMMAND,
        handler: ds_cli_survey::update::run,
        render: ds_cli_survey::update::render,
    },
    Entry {
        command: &ds_cli_survey::validate::COMMAND,
        handler: ds_cli_survey::validate::run,
        render: ds_cli_survey::validate::render,
    },
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
        command: &ds_cli_survey::purge::PLAN_COMMAND,
        handler: ds_cli_survey::purge::plan,
        render: ds_cli_survey::purge::render,
    },
    Entry {
        command: &ds_cli_survey::purge::CANDIDATES_COMMAND,
        handler: ds_cli_survey::purge::candidates,
        render: ds_cli_survey::purge::render,
    },
    Entry {
        command: &ds_cli_survey::purge::APPLY_COMMAND,
        handler: ds_cli_survey::purge::apply,
        render: ds_cli_survey::purge::render,
    },
    Entry {
        command: &ds_cli_survey::photo::LOCAL_COMMAND,
        handler: ds_cli_survey::photo::rotate_local,
        render: ds_cli_survey::photo::render,
    },
    Entry {
        command: &ds_cli_survey::photo::ROTATE_COMMAND,
        handler: ds_cli_survey::photo::rotate,
        render: ds_cli_survey::photo::render,
    },
    Entry {
        command: &ds_cli_survey::photo::PUBLISH_COMMAND,
        handler: ds_cli_survey::photo::publish,
        render: ds_cli_survey::photo::render,
    },
    Entry {
        command: &ds_cli_survey::photo_fetch::FETCH_COMMAND,
        handler: ds_cli_survey::photo_fetch::fetch,
        render: ds_cli_survey::photo_fetch::render,
    },
    Entry {
        command: &ds_cli_survey::moments::LIST_COMMAND,
        handler: ds_cli_survey::moments::list,
        render: ds_cli_survey::moments::render_list,
    },
    Entry {
        command: &ds_cli_survey::moments::READ_COMMAND,
        handler: ds_cli_survey::moments::read,
        render: ds_cli_survey::moments::render_read,
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
        command: &ds_cli_survey::member_grants::READ_COMMAND,
        handler: ds_cli_survey::member_grants::read,
        render: ds_cli_survey::member_grants::render,
    },
    Entry {
        command: &ds_cli_survey::member_grants::PLAN_COMMAND,
        handler: ds_cli_survey::member_grants::plan,
        render: ds_cli_survey::member_grants::render,
    },
    Entry {
        command: &ds_cli_survey::member_grants::APPLY_COMMAND,
        handler: ds_cli_survey::member_grants::apply,
        render: ds_cli_survey::member_grants::render,
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
        command: &ds_cli_survey::read::COMMAND,
        handler: ds_cli_survey::read::run,
        render: ds_cli_survey::read::render,
    },
    Entry {
        command: &ds_cli_survey::local::STATUS_COMMAND,
        handler: ds_cli_survey::local::status,
        render: ds_cli_survey::local::render,
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
        command: &ds_cli_survey::delete::COMMAND,
        handler: ds_cli_survey::delete::run,
        render: ds_cli_survey::delete::render,
    },
    Entry {
        command: &ds_cli_survey::delete_plan::COMMAND,
        handler: ds_cli_survey::delete_plan::run,
        render: ds_cli_survey::delete_plan::render,
    },
    Entry {
        command: &ds_cli_survey::import::COMMAND,
        handler: ds_cli_survey::import::run,
        render: ds_cli_survey::import::render,
    },
    Entry {
        command: &ds_cli_survey::migrate::plan::COMMAND,
        handler: ds_cli_survey::migrate::plan::run,
        render: ds_cli_survey::migrate::plan::render,
    },
    Entry {
        command: &ds_cli_survey::migrate::apply::COMMAND,
        handler: ds_cli_survey::migrate::apply::run,
        render: ds_cli_survey::migrate::apply::render,
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
    Entry {
        command: &ds_cli_survey::working_area::FORMS_COMMAND,
        handler: ds_cli_survey::working_area::forms,
        render: ds_cli_survey::working_area::render,
    },
    Entry {
        command: &ds_cli_survey::working_area::SELECT_COMMAND,
        handler: ds_cli_survey::working_area::select,
        render: ds_cli_survey::working_area::render,
    },
    Entry {
        command: &ds_cli_survey::working_area::CLEAR_COMMAND,
        handler: ds_cli_survey::working_area::clear,
        render: ds_cli_survey::working_area::render,
    },
];

/// Map styling. Ordered as a session uses it: list the refs, read one, author
/// guided base appearance and labels, create a print variant, then plan, publish or clear a second dimension, and
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
        command: &ds_cli_style::seed::plan::COMMAND,
        handler: ds_cli_style::seed::plan::run,
        render: ds_cli_style::seed::plan::render,
    },
    Entry {
        command: &ds_cli_style::seed::create::COMMAND,
        handler: ds_cli_style::seed::create::run,
        render: ds_cli_style::seed::create::render,
    },
    Entry {
        command: &ds_cli_style::print_variant::plan::COMMAND,
        handler: ds_cli_style::print_variant::plan::run,
        render: ds_cli_style::print_variant::plan::render,
    },
    Entry {
        command: &ds_cli_style::print_variant::create::COMMAND,
        handler: ds_cli_style::print_variant::create::run,
        render: ds_cli_style::print_variant::create::render,
    },
    Entry {
        command: &ds_cli_style::instruction::schema::COMMAND,
        handler: ds_cli_style::instruction::schema::run,
        render: ds_cli_style::instruction::schema::render,
    },
    Entry {
        command: &ds_cli_style::instruction::plan::COMMAND,
        handler: ds_cli_style::instruction::plan::run,
        render: ds_cli_style::instruction::plan::render,
    },
    Entry {
        command: &ds_cli_style::instruction::set::COMMAND,
        handler: ds_cli_style::instruction::set::run,
        render: ds_cli_style::instruction::set::render,
    },
    Entry {
        command: &ds_cli_style::governance::resolve::COMMAND,
        handler: ds_cli_style::governance::resolve::run,
        render: ds_cli_style::governance::resolve::render,
    },
    Entry {
        command: &ds_cli_style::governance::table::COMMAND,
        handler: ds_cli_style::governance::table::run,
        render: ds_cli_style::governance::table::render,
    },
    Entry {
        command: &ds_cli_style::governance::purpose_index::COMMAND,
        handler: ds_cli_style::governance::purpose_index::run,
        render: ds_cli_style::governance::purpose_index::render,
    },
    Entry {
        command: &ds_cli_style::governance::purpose_plan::COMMAND,
        handler: ds_cli_style::governance::purpose_plan::run,
        render: ds_cli_style::governance::purpose_plan::render,
    },
    Entry {
        command: &ds_cli_style::governance::manifest::COMMAND,
        handler: ds_cli_style::governance::manifest::run,
        render: ds_cli_style::governance::manifest::render,
    },
    Entry {
        command: &ds_cli_style::governance::seed_plan::COMMAND,
        handler: ds_cli_style::governance::seed_plan::run,
        render: ds_cli_style::governance::seed_plan::render,
    },
    Entry {
        command: &ds_cli_style::governance::seed_apply::COMMAND,
        handler: ds_cli_style::governance::seed_apply::run,
        render: ds_cli_style::governance::seed_apply::render,
    },
    Entry {
        command: &ds_cli_style::a4_seed::plan::COMMAND,
        handler: ds_cli_style::a4_seed::plan::run,
        render: ds_cli_style::a4_seed::plan::render,
    },
    Entry {
        command: &ds_cli_style::a4_seed::create::COMMAND,
        handler: ds_cli_style::a4_seed::create::run,
        render: ds_cli_style::a4_seed::create::render,
    },
    Entry {
        command: &ds_cli_style::a4_migration::plan::COMMAND,
        handler: ds_cli_style::a4_migration::plan::run,
        render: ds_cli_style::a4_migration::plan::render,
    },
    Entry {
        command: &ds_cli_style::a4_migration::apply::COMMAND,
        handler: ds_cli_style::a4_migration::apply::run,
        render: ds_cli_style::a4_migration::apply::render,
    },
    Entry {
        command: &ds_cli_style::governance::inventory::COMMAND,
        handler: ds_cli_style::governance::inventory::run,
        render: ds_cli_style::governance::inventory::render,
    },
    Entry {
        command: &ds_cli_style::binding::plan::COMMAND,
        handler: ds_cli_style::binding::plan::run,
        render: ds_cli_style::binding::plan::render,
    },
    Entry {
        command: &ds_cli_style::binding::create::COMMAND,
        handler: ds_cli_style::binding::create::run,
        render: ds_cli_style::binding::create::render,
    },
    Entry {
        command: &ds_cli_style::governance::backup_create::COMMAND,
        handler: ds_cli_style::governance::backup_create::run,
        render: ds_cli_style::governance::backup_create::render,
    },
    Entry {
        command: &ds_cli_style::governance::backup_read::COMMAND,
        handler: ds_cli_style::governance::backup_read::run,
        render: ds_cli_style::governance::backup_read::render,
    },
    Entry {
        command: &ds_cli_style::governance::retirement_plan::COMMAND,
        handler: ds_cli_style::governance::retirement_plan::run,
        render: ds_cli_style::governance::retirement_plan::render,
    },
    Entry {
        command: &ds_cli_style::history::list::COMMAND,
        handler: ds_cli_style::history::list::run,
        render: ds_cli_style::history::list::render,
    },
    Entry {
        command: &ds_cli_style::history::read::COMMAND,
        handler: ds_cli_style::history::read::run,
        render: ds_cli_style::history::read::render,
    },
    Entry {
        command: &ds_cli_style::history::compare::COMMAND,
        handler: ds_cli_style::history::compare::run,
        render: ds_cli_style::history::compare::render,
    },
    Entry {
        command: &ds_cli_style::history::restore::COMMAND,
        handler: ds_cli_style::history::restore::run,
        render: ds_cli_style::history::restore::render,
    },
    Entry {
        command: &ds_cli_style::multiscale::categorical::plan::COMMAND,
        handler: ds_cli_style::multiscale::categorical::plan::run,
        render: ds_cli_style::multiscale::categorical::plan::render,
    },
    Entry {
        command: &ds_cli_style::multiscale::categorical::set::COMMAND,
        handler: ds_cli_style::multiscale::categorical::set::run,
        render: ds_cli_style::multiscale::categorical::set::render,
    },
    Entry {
        command: &ds_cli_style::multiscale::color_range::plan::COMMAND,
        handler: ds_cli_style::multiscale::color_range::plan::run,
        render: ds_cli_style::multiscale::color_range::plan::render,
    },
    Entry {
        command: &ds_cli_style::multiscale::color_range::set::COMMAND,
        handler: ds_cli_style::multiscale::color_range::set::run,
        render: ds_cli_style::multiscale::color_range::set::render,
    },
    Entry {
        command: &ds_cli_style::multiscale::zoom::plan::COMMAND,
        handler: ds_cli_style::multiscale::zoom::plan::run,
        render: ds_cli_style::multiscale::zoom::plan::render,
    },
    Entry {
        command: &ds_cli_style::multiscale::zoom::set::COMMAND,
        handler: ds_cli_style::multiscale::zoom::set::run,
        render: ds_cli_style::multiscale::zoom::set::render,
    },
    Entry {
        command: &ds_cli_style::multiscale::preset::plan::COMMAND,
        handler: ds_cli_style::multiscale::preset::plan::run,
        render: ds_cli_style::multiscale::preset::plan::render,
    },
    Entry {
        command: &ds_cli_style::multiscale::preset::set::COMMAND,
        handler: ds_cli_style::multiscale::preset::set::run,
        render: ds_cli_style::multiscale::preset::set::render,
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
        command: &ds_cli_style::label::plan::COMMAND,
        handler: ds_cli_style::label::plan::run,
        render: ds_cli_style::label::plan::render,
    },
    Entry {
        command: &ds_cli_style::label::set::COMMAND,
        handler: ds_cli_style::label::set::run,
        render: ds_cli_style::label::set::render,
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
        command: &ds_cli_data::reference::COMMAND,
        handler: ds_cli_data::reference::run,
        render: ds_cli_data::reference::render,
    },
    Entry {
        command: &ds_cli_data::city_vectors::COMMAND,
        handler: ds_cli_data::city_vectors::run,
        render: ds_cli_data::city_vectors::render,
    },
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
        command: &ds_cli_data::mv_lv_orphans::COMMAND,
        handler: ds_cli_data::mv_lv_orphans::run,
        render: ds_cli_data::mv_lv_orphans::render,
    },
    Entry {
        command: &ds_cli_data::elevation::COMMAND,
        handler: ds_cli_data::elevation::run,
        render: ds_cli_data::elevation::render,
    },
    Entry {
        command: &ds_cli_data::elevation_compare::COMMAND,
        handler: ds_cli_data::elevation_compare::run,
        render: ds_cli_data::elevation_compare::render,
    },
    Entry {
        command: &ds_cli_data::terrain_sampling::DESCRIBE,
        handler: ds_cli_data::terrain_sampling::run_describe,
        render: ds_cli_data::terrain_sampling::render,
    },
    Entry {
        command: &ds_cli_data::terrain_sampling::SAMPLE,
        handler: ds_cli_data::terrain_sampling::run_sample,
        render: ds_cli_data::terrain_sampling::render,
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
    // The project's own extracts of canonical datasets. Reading is free;
    // acquiring is the one command here that reaches a geographic source.
    Entry {
        command: &ds_cli_data::project_cache::STATUS_COMMAND,
        handler: ds_cli_data::project_cache::run_status,
        render: ds_cli_data::project_cache::render_status,
    },
    Entry {
        command: &ds_cli_data::project_cache::QUERY_COMMAND,
        handler: ds_cli_data::project_cache::run_query,
        render: ds_cli_data::project_cache::render_query,
    },
    Entry {
        command: &ds_cli_data::project_cache::SEED_COMMAND,
        handler: ds_cli_data::project_cache::run_seed,
        render: ds_cli_data::project_cache::render_seed,
    },
    // The cloud-resident reads: BigQuery is the national holding of the
    // parcels and customers datasets. A bounded read answers a question;
    // `project-cache seed` keeps a project's extents with the project.
    Entry {
        command: &ds_cli_data::foundation::LOOKUP_COMMAND,
        handler: ds_cli_data::foundation::run_lookup,
        render: ds_cli_data::foundation::render_lookup,
    },
    Entry {
        command: &ds_cli_data::foundation::QUERY_COMMAND,
        handler: ds_cli_data::foundation::run_query,
        render: ds_cli_data::foundation::render_query,
    },
    Entry {
        command: &ds_cli_data::foundation::PARCELS_COMMAND,
        handler: ds_cli_data::foundation::run_parcels,
        render: ds_cli_data::foundation::render_parcels,
    },
    Entry {
        command: &ds_cli_data::spatial::PLAN_COMMAND,
        handler: ds_cli_data::spatial::run_plan,
        render: ds_cli_data::spatial::render_plan,
    },
    Entry {
        command: &ds_cli_data::spatial::EXECUTE_COMMAND,
        handler: ds_cli_data::spatial::run_execute,
        render: ds_cli_data::spatial::render_execute,
    },
];

/// Tiling. Ordered as a session uses it: read the state, look at the
/// sources, decide, run; then the catalogue.
static TILE_ENTRIES: &[Entry] = &[
    Entry {
        command: &ds_cli_tile::global::CATALOG,
        handler: ds_cli_tile::global::catalog,
        render: ds_cli_tile::global::render,
    },
    Entry {
        command: &ds_cli_tile::global::LIST,
        handler: ds_cli_tile::global::list,
        render: ds_cli_tile::global::render,
    },
    Entry {
        command: &ds_cli_tile::global::GENERATE,
        handler: ds_cli_tile::global::generate,
        render: ds_cli_tile::global::render,
    },
    Entry {
        command: &ds_cli_tile::global::STATUS,
        handler: ds_cli_tile::global::status,
        render: ds_cli_tile::global::render,
    },
    Entry {
        command: &ds_cli_tile::global::ACCESS,
        handler: ds_cli_tile::global::access,
        render: ds_cli_tile::global::render,
    },
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
static NOTES_ENTRIES: &[Entry] = &[
    Entry { command: &ds_cli_pm::private_note_backups::BACKUP, handler: ds_cli_pm::private_note_backups::backup, render: ds_cli_pm::private_notes::render },
    Entry { command: &ds_cli_pm::private_note_backups::READ, handler: ds_cli_pm::private_note_backups::read, render: ds_cli_pm::private_notes::render },
    Entry { command: &ds_cli_pm::private_note_backups::RESTORE, handler: ds_cli_pm::private_note_backups::restore, render: ds_cli_pm::private_notes::render },
    Entry { command: &ds_cli_pm::private_notes::CREATE, handler: ds_cli_pm::private_notes::create, render: ds_cli_pm::private_notes::render },
    Entry { command: &ds_cli_pm::private_notes::UPDATE, handler: ds_cli_pm::private_notes::update, render: ds_cli_pm::private_notes::render },
    Entry { command: &ds_cli_pm::private_notes::LIST, handler: ds_cli_pm::private_notes::list, render: ds_cli_pm::private_notes::render },
    Entry { command: &ds_cli_pm::private_notes::READ, handler: ds_cli_pm::private_notes::read, render: ds_cli_pm::private_notes::render },
    Entry { command: &ds_cli_pm::private_notes::RETIRE, handler: ds_cli_pm::private_notes::retire, render: ds_cli_pm::private_notes::render },
    Entry { command: &ds_cli_pm::private_notes::RESTORE, handler: ds_cli_pm::private_notes::restore, render: ds_cli_pm::private_notes::render },
    Entry { command: &ds_cli_pm::private_notes::VERSIONS, handler: ds_cli_pm::private_notes::versions, render: ds_cli_pm::private_notes::render },
    Entry { command: &ds_cli_pm::private_notes::READ_VERSION, handler: ds_cli_pm::private_notes::read_version, render: ds_cli_pm::private_notes::render },
    Entry { command: &ds_cli_pm::private_notes::RESTORE_VERSION, handler: ds_cli_pm::private_notes::restore_version, render: ds_cli_pm::private_notes::render },
    Entry { command: &ds_cli_pm::checklist_notes::PRIVATE_READ, handler: ds_cli_pm::checklist_notes::private_read, render: ds_cli_pm::checklist_notes::render },
    Entry { command: &ds_cli_pm::checklist_notes::PRIVATE_EDIT, handler: ds_cli_pm::checklist_notes::private_edit, render: ds_cli_pm::checklist_notes::render },
];

static WORK_ENTRIES: &[Entry] = &[
    Entry {
        command: &ds_cli_pm::overview::COMMAND,
        handler: ds_cli_pm::overview::run,
        render: ds_cli_pm::overview::render,
    },
    Entry {
        command: &ds_cli_pm::plan::COMMAND,
        handler: ds_cli_pm::plan::run,
        render: ds_cli_pm::plan::render,
    },
    Entry {
        command: &ds_cli_pm::collaboration::PROJECTS,
        handler: ds_cli_pm::collaboration::projects,
        render: ds_cli_pm::collaboration::render,
    },
    Entry {
        command: &ds_cli_pm::collaboration::COMMENT,
        handler: ds_cli_pm::collaboration::comment,
        render: ds_cli_pm::collaboration::render,
    },
    Entry {
        command: &ds_cli_pm::collaboration::COMMENTS,
        handler: ds_cli_pm::collaboration::comments,
        render: ds_cli_pm::collaboration::render,
    },
    Entry {
        command: &ds_cli_pm::member::COMMAND,
        handler: ds_cli_pm::member::run,
        render: ds_cli_pm::member::render,
    },
    Entry {
        command: &ds_cli_pm::deletion::INVENTORY,
        handler: ds_cli_pm::deletion::inventory,
        render: ds_cli_pm::deletion::render_inventory,
    },
    Entry {
        command: &ds_cli_pm::deletion::READ,
        handler: ds_cli_pm::deletion::read,
        render: ds_cli_pm::deletion::render_read,
    },
    Entry {
        command: &ds_cli_pm::deletion::RESTORE,
        handler: ds_cli_pm::deletion::restore,
        render: ds_cli_pm::deletion::render_restore,
    },
    Entry {
        command: &ds_cli_pm::task::list::COMMAND,
        handler: ds_cli_pm::task::list::run,
        render: ds_cli_pm::task::list::render,
    },
    Entry {
        command: &ds_cli_pm::task::read::COMMAND,
        handler: ds_cli_pm::task::read::run,
        render: ds_cli_pm::task::read::render,
    },
    Entry {
        command: &ds_cli_pm::task::checklist::READ,
        handler: ds_cli_pm::task::checklist::read,
        render: ds_cli_pm::task::checklist::render,
    },
    Entry {
        command: &ds_cli_pm::task::checklist::EDIT,
        handler: ds_cli_pm::task::checklist::edit,
        render: ds_cli_pm::task::checklist::render,
    },
    Entry {
        command: &ds_cli_pm::task::checklist::PROMOTE,
        handler: ds_cli_pm::task::checklist::promote,
        render: ds_cli_pm::task::checklist::render,
    },
    Entry {
        command: &ds_cli_pm::task::create::COMMAND,
        handler: ds_cli_pm::task::create::run,
        render: ds_cli_pm::task::create::render,
    },
    Entry {
        command: &ds_cli_pm::task::subdivision::SUBDIVIDE,
        handler: ds_cli_pm::task::subdivision::run_subdivide,
        render: ds_cli_pm::task::subdivision::render,
    },
    Entry {
        command: &ds_cli_pm::task::subdivision::PROGRESS,
        handler: ds_cli_pm::task::subdivision::run_progress,
        render: ds_cli_pm::task::subdivision::render,
    },
    Entry {
        command: &ds_cli_pm::task::update::COMMAND,
        handler: ds_cli_pm::task::update::run,
        render: ds_cli_pm::task::update::render,
    },
    Entry {
        command: &ds_cli_pm::task::delete::COMMAND,
        handler: ds_cli_pm::task::delete::run,
        render: ds_cli_pm::task::delete::render,
    },
    Entry {
        command: &ds_cli_pm::task::assign::COMMAND,
        handler: ds_cli_pm::task::assign::run,
        render: ds_cli_pm::task::assign::render,
    },
    Entry {
        command: &ds_cli_pm::task::detach::COMMAND,
        handler: ds_cli_pm::task::detach::run,
        render: ds_cli_pm::task::detach::render,
    },
    Entry {
        command: &ds_cli_pm::task::attach::COMMAND,
        handler: ds_cli_pm::task::attach::run,
        render: ds_cli_pm::task::attach::render,
    },
    Entry {
        command: &ds_cli_pm::task::respond::COMMAND,
        handler: ds_cli_pm::task::respond::run,
        render: ds_cli_pm::task::respond::render,
    },
    // Where the work is: read it, set it from the DS Grid objects a comment
    // names, clear it (task-geometry-from-objects.md).
    Entry {
        command: &ds_cli_pm::geometry::read::COMMAND,
        handler: ds_cli_pm::geometry::read::run,
        render: ds_cli_pm::geometry::read::render,
    },
    Entry {
        command: &ds_cli_pm::geometry::clear::COMMAND,
        handler: ds_cli_pm::geometry::clear::run,
        render: ds_cli_pm::geometry::clear::render,
    },
    Entry {
        command: &ds_cli_pm::model_link::ADD,
        handler: ds_cli_pm::model_link::add,
        render: ds_cli_pm::model_link::render,
    },
    Entry {
        command: &ds_cli_pm::model_link::REMOVE,
        handler: ds_cli_pm::model_link::remove,
        render: ds_cli_pm::model_link::render,
    },
    Entry {
        command: &ds_cli_pm::model_link::LIST,
        handler: ds_cli_pm::model_link::list,
        render: ds_cli_pm::model_link::render,
    },
    Entry {
        command: &ds_cli_pm::task::proposals::PROPOSE,
        handler: ds_cli_pm::task::proposals::run_propose,
        render: ds_cli_pm::task::proposals::render_propose,
    },
    Entry {
        command: &ds_cli_pm::task::proposals::REQUEST_ADMISSION,
        handler: ds_cli_pm::task::proposals::run_request_admission,
        render: ds_cli_pm::task::proposals::render_request_admission,
    },
    Entry {
        command: &ds_cli_pm::task::proposals::ADMIT,
        handler: ds_cli_pm::task::proposals::run_admit,
        render: ds_cli_pm::task::proposals::render_admit,
    },
    Entry {
        command: &ds_cli_pm::task::proposals::DECLINE,
        handler: ds_cli_pm::task::proposals::run_decline,
        render: ds_cli_pm::task::proposals::render_decline,
    },
    Entry {
        command: &ds_cli_pm::task::proposals::LOG_HOURS,
        handler: ds_cli_pm::task::proposals::run_log_hours,
        render: ds_cli_pm::task::proposals::render_log_hours,
    },
    Entry {
        command: &ds_cli_pm::record::list::COMMAND,
        handler: ds_cli_pm::record::list::run,
        render: ds_cli_pm::record::list::render,
    },
    Entry {
        command: &ds_cli_pm::record::read::COMMAND,
        handler: ds_cli_pm::record::read::run,
        render: ds_cli_pm::record::read::render,
    },
    Entry {
        command: &ds_cli_pm::task::block::COMMAND,
        handler: ds_cli_pm::task::block::run,
        render: ds_cli_pm::task::block::render,
    },
    Entry {
        command: &ds_cli_pm::task::unblock::COMMAND,
        handler: ds_cli_pm::task::unblock::run,
        render: ds_cli_pm::task::unblock::render,
    },
    Entry {
        command: &ds_cli_pm::party::list::COMMAND,
        handler: ds_cli_pm::party::list::run,
        render: ds_cli_pm::party::list::render,
    },
    Entry {
        command: &ds_cli_pm::party::create::COMMAND,
        handler: ds_cli_pm::party::create::run,
        render: ds_cli_pm::party::create::render,
    },
    Entry {
        command: &ds_cli_pm::party::update::COMMAND,
        handler: ds_cli_pm::party::update::run,
        render: ds_cli_pm::party::update::render,
    },
    Entry {
        command: &ds_cli_pm::record::thread::COMMAND,
        handler: ds_cli_pm::record::thread::run,
        render: ds_cli_pm::record::thread::render,
    },
    Entry {
        command: &ds_cli_pm::note::CREATE,
        handler: ds_cli_pm::note::run,
        render: ds_cli_pm::note::render,
    },
    Entry {
        command: &ds_cli_pm::note_memory::READ,
        handler: ds_cli_pm::note_memory::read,
        render: ds_cli_pm::note_memory::render,
    },
    Entry {
        command: &ds_cli_pm::note_memory::QUERY,
        handler: ds_cli_pm::note_memory::query,
        render: ds_cli_pm::note_memory::render,
    },
    Entry {
        command: &ds_cli_pm::note_memory::CONTEXT,
        handler: ds_cli_pm::note_memory::context,
        render: ds_cli_pm::note_memory::render,
    },
    Entry {
        command: &ds_cli_pm::note_memory::UPDATE,
        handler: ds_cli_pm::note_memory::update,
        render: ds_cli_pm::note_memory::render,
    },
    Entry {
        command: &ds_cli_pm::note_memory::VERSIONS,
        handler: ds_cli_pm::note_memory::versions,
        render: ds_cli_pm::note_memory::render,
    },
    Entry {
        command: &ds_cli_pm::note_memory::BACKUP,
        handler: ds_cli_pm::note_memory::backup,
        render: ds_cli_pm::note_memory::render,
    },
    Entry {
        command: &ds_cli_pm::note_memory::RETIRE,
        handler: ds_cli_pm::note_memory::retire,
        render: ds_cli_pm::note_memory::render,
    },
    Entry {
        command: &ds_cli_pm::note_memory::RESTORE,
        handler: ds_cli_pm::note_memory::restore,
        render: ds_cli_pm::note_memory::render,
    },
    Entry {
        command: &ds_cli_pm::note_memory::CONSOLIDATE,
        handler: ds_cli_pm::note_memory::consolidate,
        render: ds_cli_pm::note_memory::render,
    },
    Entry { command: &ds_cli_pm::checklist_notes::PROJECT_READ, handler: ds_cli_pm::checklist_notes::project_read, render: ds_cli_pm::checklist_notes::render },
    Entry { command: &ds_cli_pm::checklist_notes::PROJECT_EDIT, handler: ds_cli_pm::checklist_notes::project_edit, render: ds_cli_pm::checklist_notes::render },
    Entry { command: &ds_cli_pm::documents::COPY, handler: ds_cli_pm::documents::copy, render: ds_cli_pm::documents::render },
    Entry { command: &ds_cli_pm::documents::MAIL, handler: ds_cli_pm::documents::mail, render: ds_cli_pm::documents::render },
    Entry {
        command: &ds_cli_pm::record::create::COMMAND,
        handler: ds_cli_pm::record::create::run,
        render: ds_cli_pm::record::create::render,
    },
    Entry {
        command: &ds_cli_pm::record::reply::COMMAND,
        handler: ds_cli_pm::record::reply::run,
        render: ds_cli_pm::record::reply::render,
    },
    Entry {
        command: &ds_cli_pm::record::update::COMMAND,
        handler: ds_cli_pm::record::update::run,
        render: ds_cli_pm::record::update::render,
    },
];

/// Project Assets lists its commands in the order a session uses them: find
/// the asset, look at it, then act on it. Domain help prints this order
/// verbatim, so the index doubles as the procedure.
static ASSETS_ENTRIES: &[Entry] = &[
    Entry {
        command: &ds_cli_assets::semantic::EXPORT,
        handler: ds_cli_assets::semantic::export,
        render: ds_cli_assets::semantic::render,
    },
    Entry {
        command: &ds_cli_assets::semantic::LOOKUP,
        handler: ds_cli_assets::semantic::lookup,
        render: ds_cli_assets::semantic::render,
    },
    Entry {
        command: &ds_cli_assets::semantic::READ,
        handler: ds_cli_assets::semantic::read,
        render: ds_cli_assets::semantic::render,
    },
    Entry {
        command: &ds_cli_assets::semantic::IMPORT,
        handler: ds_cli_assets::semantic::import,
        render: ds_cli_assets::semantic::render,
    },
    Entry {
        command: &ds_cli_assets::backup::COMMAND,
        handler: ds_cli_assets::backup::run,
        render: ds_cli_assets::backup::render,
    },
    Entry {
        command: &ds_cli_assets::shared::PUBLISH_MAP,
        handler: ds_cli_assets::shared::publish_map,
        render: ds_cli_assets::shared::render,
    },
    Entry {
        command: &ds_cli_assets::shared::RESOLVE,
        handler: ds_cli_assets::shared::resolve,
        render: ds_cli_assets::shared::render,
    },
    Entry {
        command: &ds_cli_assets::shared::MAPS,
        handler: ds_cli_assets::shared::maps,
        render: ds_cli_assets::shared::render,
    },
    Entry {
        command: &ds_cli_assets::shared::REFERENCE,
        handler: ds_cli_assets::shared::reference,
        render: ds_cli_assets::shared::render,
    },
    Entry {
        command: &ds_cli_assets::list::COMMAND,
        handler: ds_cli_assets::list::run,
        render: ds_cli_assets::list::render,
    },
    Entry {
        command: &ds_cli_assets::tree::COMMAND,
        handler: ds_cli_assets::tree::run,
        render: ds_cli_assets::tree::render,
    },
    Entry {
        command: &ds_cli_assets::versions::COMMAND,
        handler: ds_cli_assets::versions::run,
        render: ds_cli_assets::versions::render,
    },
    Entry {
        command: &ds_cli_assets::read::COMMAND,
        handler: ds_cli_assets::read::run,
        render: ds_cli_assets::read::render,
    },
    Entry {
        command: &ds_cli_assets::download::COMMAND,
        handler: ds_cli_assets::download::run,
        render: ds_cli_assets::download::render,
    },
    Entry {
        command: &ds_cli_assets::preview::COMMAND,
        handler: ds_cli_assets::preview::run,
        render: ds_cli_assets::preview::render,
    },
    Entry {
        command: &ds_cli_assets::classify::COMMAND,
        handler: ds_cli_assets::classify::run,
        render: ds_cli_assets::classify::render,
    },
    Entry {
        command: &ds_cli_assets::promote::COMMAND,
        handler: ds_cli_assets::promote::run,
        render: ds_cli_assets::promote::render,
    },
    Entry {
        command: &ds_cli_assets::attach::COMMAND,
        handler: ds_cli_assets::attach::run,
        render: ds_cli_assets::attach::render,
    },
    Entry {
        command: &ds_cli_assets::ingest::COMMAND,
        handler: ds_cli_assets::ingest::run,
        render: ds_cli_assets::ingest::render,
    },
    Entry {
        command: &ds_cli_assets::folder::COMMAND,
        handler: ds_cli_assets::folder::run,
        render: ds_cli_assets::folder::render,
    },
];

/// Design collaboration is durable project metadata, not map-owned local
/// state. It is available through a paired desktop without an open map.
static DESIGN_ENTRIES: &[Entry] = &[
    Entry {
        command: &ds_cli_design::models::LIST,
        handler: ds_cli_design::models::list,
        render: ds_cli_design::models::render,
    },
    Entry {
        command: &ds_cli_design::models::EXPORT,
        handler: ds_cli_design::models::export,
        render: ds_cli_design::models::render,
    },
    Entry {
        command: &ds_cli_design::models_publish::COMMAND,
        handler: ds_cli_design::models_publish::run,
        render: ds_cli_design::models_publish::render,
    },
    Entry {
        command: &ds_cli_design::intake_upload::COMMAND,
        handler: ds_cli_design::intake_upload::run,
        render: ds_cli_design::intake_upload::render,
    },
    Entry {
        command: &ds_cli_design::native_tags::DEFINITIONS,
        handler: ds_cli_design::native_tags::definitions,
        render: ds_cli_design::native_tags::render,
    },
    Entry {
        command: &ds_cli_design::native_tags::PREVIEW,
        handler: ds_cli_design::native_tags::preview,
        render: ds_cli_design::native_tags::render,
    },
    Entry {
        command: &ds_cli_design::native_tags::APPLY,
        handler: ds_cli_design::native_tags::apply,
        render: ds_cli_design::native_tags::render,
    },
    Entry {
        command: &ds_cli_design::native_tags::PROJECTION,
        handler: ds_cli_design::native_tags::projection,
        render: ds_cli_design::native_tags::render,
    },
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
        command: &ds_cli_design::attachment::list_project::COMMAND,
        handler: ds_cli_design::attachment::list_project::run,
        render: ds_cli_design::attachment::list_project::render,
    },
    Entry {
        command: &ds_cli_design::attachment::show::COMMAND,
        handler: ds_cli_design::attachment::show::run,
        render: ds_cli_design::attachment::show::render,
    },
    Entry {
        command: &ds_cli_design::attachment::versions::COMMAND,
        handler: ds_cli_design::attachment::versions::run,
        render: ds_cli_design::attachment::versions::render,
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
        command: &ds_cli_design::attachment::set_latest::COMMAND,
        handler: ds_cli_design::attachment::set_latest::run,
        render: ds_cli_design::attachment::set_latest::render,
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
        command: &ds_cli_design::materials::PREVIEW,
        handler: ds_cli_design::materials::preview,
        render: ds_cli_design::materials::render,
    },
    Entry {
        command: &ds_cli_design::materials::APPLY,
        handler: ds_cli_design::materials::apply,
        render: ds_cli_design::materials::render,
    },
    Entry {
        command: &ds_cli_design::known_columns::list::COMMAND,
        handler: ds_cli_design::known_columns::list::run,
        render: ds_cli_design::known_columns::list::render,
    },
    Entry {
        command: &ds_cli_design::config::SHEETS,
        handler: ds_cli_design::config::sheets,
        render: ds_cli_design::config::render,
    },
    Entry {
        command: &ds_cli_design::config::READ,
        handler: ds_cli_design::config::read,
        render: ds_cli_design::config::render,
    },
    Entry {
        command: &ds_cli_design::config::DIFF,
        handler: ds_cli_design::config::diff,
        render: ds_cli_design::config::render,
    },
    Entry {
        command: &ds_cli_design::config::SET,
        handler: ds_cli_design::config::set,
        render: ds_cli_design::config::render,
    },
    Entry {
        command: &ds_cli_design::config::SAVE,
        handler: ds_cli_design::config::save,
        render: ds_cli_design::config::render,
    },
    Entry {
        command: &ds_cli_design::config::DUPLICATE,
        handler: ds_cli_design::config::duplicate,
        render: ds_cli_design::config::render,
    },
    Entry {
        command: &ds_cli_design::config_copy::PLAN,
        handler: ds_cli_design::config_copy::plan,
        render: ds_cli_design::config_copy::render,
    },
    Entry {
        command: &ds_cli_design::config_copy::APPLY,
        handler: ds_cli_design::config_copy::apply,
        render: ds_cli_design::config_copy::render,
    },
    Entry {
        command: &ds_cli_design::feeder_limits::READ,
        handler: ds_cli_design::feeder_limits::read,
        render: ds_cli_design::feeder_limits::render,
    },
    Entry {
        command: &ds_cli_design::feeder_limits::SET,
        handler: ds_cli_design::feeder_limits::set,
        render: ds_cli_design::feeder_limits::render,
    },
    Entry {
        command: &ds_cli_design::category_catalog::METER,
        handler: ds_cli_design::category_catalog::meter,
        render: ds_cli_design::category_catalog::render,
    },
    Entry {
        command: &ds_cli_design::category_catalog::READ,
        handler: ds_cli_design::category_catalog::read,
        render: ds_cli_design::category_catalog::render,
    },
    Entry {
        command: &ds_cli_design::category_catalog::ALIAS_SET,
        handler: ds_cli_design::category_catalog::alias,
        render: ds_cli_design::category_catalog::render,
    },
    Entry {
        command: &ds_cli_design::category_catalog::METER_DEFAULT,
        handler: ds_cli_design::category_catalog::meter_default,
        render: ds_cli_design::category_catalog::render,
    },
    Entry {
        command: &ds_cli_design::category_catalog::CUSTOMER_UNBIND,
        handler: ds_cli_design::category_catalog::customer_unbind,
        render: ds_cli_design::category_catalog::render,
    },
    Entry {
        command: &ds_cli_design::category_catalog::CUSTOMER_RENAME,
        handler: ds_cli_design::category_catalog::customer_rename,
        render: ds_cli_design::category_catalog::render,
    },
    Entry {
        command: &ds_cli_design::category_catalog::CUSTOMER_RETIRE,
        handler: ds_cli_design::category_catalog::customer_retire,
        render: ds_cli_design::category_catalog::render,
    },
    Entry {
        command: &ds_cli_design::category_catalog::CUSTOMER_RETIRE_UNNAMED,
        handler: ds_cli_design::category_catalog::customer_retire_unnamed,
        render: ds_cli_design::category_catalog::render,
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
        command: &ds_cli_design::comment::redact::COMMAND,
        handler: ds_cli_design::comment::redact::run,
        render: ds_cli_design::comment::redact::render,
    },
    Entry {
        command: &ds_cli_design::lv::project_export::COMMAND,
        handler: ds_cli_design::lv::project_export::run,
        render: ds_cli_design::lv::project_export::render,
    },
    Entry {
        command: &ds_cli_design::lv::analysis_read::COMMAND,
        handler: ds_cli_design::lv::analysis_read::run,
        render: ds_cli_design::lv::analysis_read::render,
    },
    Entry {
        command: &ds_cli_design::lv::project_run::COMMAND,
        handler: ds_cli_design::lv::project_run::run,
        render: ds_cli_design::lv::project_run::render,
    },
    Entry {
        command: &ds_cli_design::lv::project_save::COMMAND,
        handler: ds_cli_design::lv::project_save::run,
        render: ds_cli_design::lv::project_save::render,
    },
    Entry {
        command: &ds_cli_design::repair::DESCRIBE,
        handler: ds_cli_design::repair::describe,
        render: ds_cli_design::repair::render,
    },
    Entry {
        command: &ds_cli_design::repair::AUDIT,
        handler: ds_cli_design::repair::audit,
        render: ds_cli_design::repair::render,
    },
    Entry {
        command: &ds_cli_design::repair::COMPARE,
        handler: ds_cli_design::repair::compare,
        render: ds_cli_design::repair::render,
    },
    Entry {
        command: &ds_cli_design::repair::PROPOSE,
        handler: ds_cli_design::repair::propose,
        render: ds_cli_design::repair::render,
    },
    Entry {
        command: &ds_cli_design::repair::PROJECT,
        handler: ds_cli_design::repair::project,
        render: ds_cli_design::repair::render,
    },
    Entry {
        command: &ds_cli_design::repair::WORKSPACE,
        handler: ds_cli_design::repair::workspace,
        render: ds_cli_design::repair::render,
    },
    Entry {
        command: &ds_cli_design::repair::PUBLISH,
        handler: ds_cli_design::repair::publish,
        render: ds_cli_design::repair::render,
    },
    Entry {
        command: &ds_cli_design::project::SOURCES,
        handler: ds_cli_design::project::sources,
        render: ds_cli_design::project::render,
    },
    Entry {
        command: &ds_cli_design::project::INIT,
        handler: ds_cli_design::project::init,
        render: ds_cli_design::project::render,
    },
    Entry {
        command: &ds_cli_design::project::WRITE,
        handler: ds_cli_design::project::write,
        render: ds_cli_design::project::render,
    },
    Entry {
        command: &ds_cli_design::project::EDIT,
        handler: ds_cli_design::project::edit,
        render: ds_cli_design::project::render,
    },
    Entry {
        command: &ds_cli_design::project::READ,
        handler: ds_cli_design::project::read,
        render: ds_cli_design::project::render,
    },
    Entry {
        command: &ds_cli_design::project::RESTORE,
        handler: ds_cli_design::project::restore,
        render: ds_cli_design::project::render,
    },
    Entry {
        command: &ds_cli_design::project::REVISIONS,
        handler: ds_cli_design::project::revisions,
        render: ds_cli_design::project::render,
    },
    Entry {
        command: &ds_cli_design::project::COMPARE,
        handler: ds_cli_design::project::compare,
        render: ds_cli_design::project::render,
    },
    Entry {
        command: &ds_cli_design::project::STATUS,
        handler: ds_cli_design::project::status,
        render: ds_cli_design::project::render,
    },
    Entry {
        command: &ds_cli_design::project::PROCESS,
        handler: ds_cli_design::project::process,
        render: ds_cli_design::project::render,
    },
    Entry {
        command: &ds_cli_design::project::CANCEL,
        handler: ds_cli_design::project::cancel,
        render: ds_cli_design::project::render,
    },
    Entry {
        command: &ds_cli_design::project::RESULT,
        handler: ds_cli_design::project::result,
        render: ds_cli_design::project::render,
    },
    Entry {
        command: &ds_cli_design::project::OUTBOX,
        handler: ds_cli_design::project::outbox,
        render: ds_cli_design::project::render,
    },
    Entry {
        command: &ds_cli_design::project::REPORT,
        handler: ds_cli_design::project::report,
        render: ds_cli_design::project::render,
    },
    Entry {
        command: &ds_cli_design::lv::process::COMMAND,
        handler: ds_cli_design::lv::process::run,
        render: ds_cli_design::lv::process::render,
    },
    Entry {
        command: &ds_cli_design::lv::voltage_drop::COMMAND,
        handler: ds_cli_design::lv::voltage_drop::run,
        render: ds_cli_design::lv::voltage_drop::render,
    },
    Entry {
        command: &ds_cli_design::process_settings::COMMAND,
        handler: ds_cli_design::process_settings::run,
        render: ds_cli_design::process_settings::render,
    },
    Entry {
        command: &ds_cli_design::autoprocess::COMMAND,
        handler: ds_cli_design::autoprocess::run,
        render: ds_cli_design::autoprocess::render,
    },
    Entry {
        command: &ds_cli_design::force_gate::COMMAND,
        handler: ds_cli_design::force_gate::run,
        render: ds_cli_design::force_gate::render,
    },
    Entry {
        command: &ds_cli_design::data_lane::COMMAND,
        handler: ds_cli_design::data_lane::run,
        render: ds_cli_design::data_lane::render,
    },
    Entry {
        command: &ds_cli_design::transformer::status::COMMAND,
        handler: ds_cli_design::transformer::status::run,
        render: ds_cli_design::transformer::status::render,
    },
    Entry {
        command: &ds_cli_design::collisions::COMMAND,
        handler: ds_cli_design::collisions::run,
        render: ds_cli_design::collisions::render,
    },
    Entry {
        command: &ds_cli_design::migrate::plan::COMMAND,
        handler: ds_cli_design::migrate::plan::run,
        render: ds_cli_design::migrate::plan::render,
    },
    Entry {
        command: &ds_cli_design::migrate::apply::COMMAND,
        handler: ds_cli_design::migrate::apply::run,
        render: ds_cli_design::migrate::apply::render,
    },
    Entry {
        command: &ds_cli_design::pinned::COMMAND,
        handler: ds_cli_design::pinned::run,
        render: ds_cli_design::pinned::render,
    },
    Entry {
        command: &ds_cli_design::activities::sweep::COMMAND,
        handler: ds_cli_design::activities::sweep::run,
        render: ds_cli_design::activities::sweep::render,
    },
    Entry {
        command: &ds_cli_design::activities::read::COMMAND,
        handler: ds_cli_design::activities::read::run,
        render: ds_cli_design::activities::read::render,
    },
    Entry {
        command: &ds_cli_design::preview::BULK_PLAN,
        handler: ds_cli_design::preview::run_bulk_plan,
        render: ds_cli_design::preview::render_bulk_plan,
    },
    Entry {
        command: &ds_cli_design::preview::DOWNLOAD_PLAN,
        handler: ds_cli_design::preview::run_download_plan,
        render: ds_cli_design::preview::render_download_plan,
    },
    Entry {
        command: &ds_cli_design::preview::DOWNLOAD_FETCH,
        handler: ds_cli_design::preview::run_download_fetch,
        render: ds_cli_design::preview::render_download_fetch,
    },
    Entry { command: &ds_cli_design::history_import::PREPARE, handler: ds_cli_design::history_import::prepare, render: ds_cli_design::history_import::render },
    Entry { command: &ds_cli_design::history_import::PUBLISH, handler: ds_cli_design::history_import::publish, render: ds_cli_design::history_import::render },
    Entry {
        command: &ds_cli_design::version_catalog::CATALOG_COMMAND,
        handler: ds_cli_design::version_catalog::catalog,
        render: ds_cli_design::version_catalog::render,
    },
    Entry {
        command: &ds_cli_design::version_catalog::REVISION_SHOW,
        handler: ds_cli_design::version_catalog::show,
        render: ds_cli_design::version_catalog::render,
    },
    Entry {
        command: &ds_cli_design::version_catalog::REVISION_PUBLISH,
        handler: ds_cli_design::version_catalog::publish,
        render: ds_cli_design::version_catalog::render,
    },
    Entry {
        command: &ds_cli_design::version_catalog::RENUMBER,
        handler: ds_cli_design::version_catalog::renumber,
        render: ds_cli_design::version_catalog::render,
    },
    Entry {
        command: &ds_cli_design::version_catalog::REVISION_BIND,
        handler: ds_cli_design::version_catalog::bind,
        render: ds_cli_design::version_catalog::render,
    },
    Entry {
        command: &ds_cli_design::version_catalog::REVISION_PROMOTE,
        handler: ds_cli_design::version_catalog::promote,
        render: ds_cli_design::version_catalog::render,
    },
    Entry {
        command: &ds_cli_design::version_catalog::REVISION_COMPARE,
        handler: ds_cli_design::version_catalog::compare,
        render: ds_cli_design::version_catalog::render,
    },
    Entry {
        command: &ds_cli_design::versions::LIST,
        handler: ds_cli_design::versions::list,
        render: ds_cli_design::versions::render,
    },
    Entry {
        command: &ds_cli_design::versions::COMPARE,
        handler: ds_cli_design::versions::compare,
        render: ds_cli_design::versions::render,
    },
    Entry {
        command: &ds_cli_design::versions::SHOW,
        handler: ds_cli_design::versions::show,
        render: ds_cli_design::versions::render,
    },
    Entry {
        command: &ds_cli_design::versions::BEGIN,
        handler: ds_cli_design::versions::begin,
        render: ds_cli_design::versions::render,
    },
    Entry {
        command: &ds_cli_design::versions::BEGIN_BATCH,
        handler: ds_cli_design::versions::begin_batch,
        render: ds_cli_design::versions::render,
    },
    Entry {
        command: &ds_cli_design::versions::SUMMARIES,
        handler: ds_cli_design::versions::summaries,
        render: ds_cli_design::versions::render,
    },
    Entry {
        command: &ds_cli_design::versions::STATUS,
        handler: ds_cli_design::versions::status,
        render: ds_cli_design::versions::render,
    },
    Entry {
        command: &ds_cli_design::preview::CONFLICT_LIST,
        handler: ds_cli_design::preview::run_conflict_list,
        render: ds_cli_design::preview::render_conflict_list,
    },
    Entry {
        command: &ds_cli_design::preview::CONFLICT_CHECK,
        handler: ds_cli_design::preview::run_conflict_check,
        render: ds_cli_design::preview::render_conflict_check,
    },
    Entry {
        command: &ds_cli_design::preview::PRESENCE_STATUS,
        handler: ds_cli_design::preview::run_presence_status,
        render: ds_cli_design::preview::render_presence_status,
    },
    Entry {
        command: &ds_cli_design::transformer::dashboard::COMMAND,
        handler: ds_cli_design::transformer::dashboard::run,
        render: ds_cli_design::transformer::dashboard::render,
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
    Entry {
        command: &ds_cli_design::transformer::backup::BACKUP,
        handler: ds_cli_design::transformer::backup::run_backup,
        render: ds_cli_design::transformer::backup::render_backup,
    },
    Entry {
        command: &ds_cli_design::transformer::backup::RESTORE_DELETED,
        handler: ds_cli_design::transformer::backup::run_restore_deleted,
        render: ds_cli_design::transformer::backup::render_restore_deleted,
    },
];

static INSTALL_ENTRIES: &[Entry] = &[
    Entry {
        command: &ds_cli_installs::list::COMMAND,
        handler: ds_cli_installs::list::run,
        render: ds_cli_installs::list::render,
    },
    Entry {
        command: &ds_cli_installs::show::COMMAND,
        handler: ds_cli_installs::show::run,
        render: ds_cli_installs::show::render,
    },
    Entry {
        command: &ds_cli_installs::policy::COMMAND,
        handler: ds_cli_installs::policy::run,
        render: ds_cli_installs::policy::render,
    },
    Entry {
        command: &ds_cli_installs::retire::COMMAND,
        handler: ds_cli_installs::retire::run,
        render: ds_cli_installs::retire::render,
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
        command: &ds_cli_desktop::list::COMMAND,
        handler: ds_cli_desktop::list::run,
        render: ds_cli_desktop::list::render,
    },
    Entry {
        command: &ds_cli_desktop::connectivity::STATUS,
        handler: ds_cli_desktop::connectivity::status,
        render: ds_cli_desktop::connectivity::render,
    },
    Entry {
        command: &ds_cli_desktop::connectivity::SET,
        handler: ds_cli_desktop::connectivity::set,
        render: ds_cli_desktop::connectivity::render,
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
    Entry {
        command: &ds_cli_desktop::printing::SETTINGS_COMMAND,
        handler: ds_cli_desktop::printing::settings,
        render: ds_cli_desktop::printing::render,
    },
    Entry {
        command: &ds_cli_desktop::printing::TRANSFORMERS_COMMAND,
        handler: ds_cli_desktop::printing::transformers,
        render: ds_cli_desktop::printing::render,
    },
    Entry {
        command: &ds_cli_desktop::printing::EXPORT_COMMAND,
        handler: ds_cli_desktop::printing::export,
        render: ds_cli_desktop::printing::render,
    },
    Entry {
        command: &ds_cli_desktop::printing::PREVIEW_COMMAND,
        handler: ds_cli_desktop::printing::preview,
        render: ds_cli_desktop::printing::render,
    },
    Entry {
        command: &ds_cli_desktop::printing_view::COMMAND,
        handler: ds_cli_desktop::printing_view::run,
        render: ds_cli_desktop::printing::render,
    },
    Entry {
        command: &ds_cli_desktop::printing::PREPARE_COMMAND,
        handler: ds_cli_desktop::printing::run,
        render: ds_cli_desktop::printing::render,
    },
    Entry {
        command: &ds_cli_desktop::custom_print::AREA_COMMAND,
        handler: ds_cli_desktop::custom_print::area,
        render: ds_cli_desktop::printing::render,
    },
    Entry {
        command: &ds_cli_desktop::custom_print::EXPORT_COMMAND,
        handler: ds_cli_desktop::custom_print::export,
        render: ds_cli_desktop::printing::render,
    },
    Entry {
        command: &ds_cli_desktop::custom_print::LIST_COMMAND,
        handler: ds_cli_desktop::custom_print::list,
        render: ds_cli_desktop::printing::render,
    },
    Entry {
        command: &ds_cli_desktop::custom_print::ATTACH_COMMAND,
        handler: ds_cli_desktop::custom_print::attach,
        render: ds_cli_desktop::printing::render,
    },
    Entry {
        command: &ds_cli_desktop::printing::SEED_CONTEXT_COMMAND,
        handler: ds_cli_desktop::printing::seed_context,
        render: ds_cli_desktop::printing::render,
    },
    Entry {
        command: &ds_cli_desktop::artifact::READ_COMMAND,
        handler: ds_cli_desktop::artifact::read,
        render: ds_cli_desktop::artifact::render,
    },
    Entry {
        command: &ds_cli_desktop::artifact::COPY_COMMAND,
        handler: ds_cli_desktop::artifact::copy,
        render: ds_cli_desktop::artifact::render,
    },
    Entry {
        command: &ds_cli_desktop::data::STATUS_COMMAND,
        handler: ds_cli_desktop::data::status,
        render: ds_cli_desktop::data::render,
    },
    Entry {
        command: &ds_cli_desktop::data::CATALOG_COMMAND,
        handler: ds_cli_desktop::data::catalog,
        render: ds_cli_desktop::data::render,
    },
    Entry {
        command: &ds_cli_desktop::data::PUBLISH_COMMAND,
        handler: ds_cli_desktop::data::publish,
        render: ds_cli_desktop::data::render,
    },
    Entry {
        command: &ds_cli_desktop::data::INSTALL_COMMAND,
        handler: ds_cli_desktop::data::install,
        render: ds_cli_desktop::data::render,
    },
    Entry {
        command: &ds_cli_desktop::data::STORAGE_COMMAND,
        handler: ds_cli_desktop::data::storage,
        render: ds_cli_desktop::data::render,
    },
    Entry {
        command: &ds_cli_desktop::data::REMOVE_COMMAND,
        handler: ds_cli_desktop::data::remove,
        render: ds_cli_desktop::data::render,
    },
    Entry {
        command: &ds_cli_desktop::sync::SANITIZE_PREVIEW_COMMAND,
        handler: ds_cli_desktop::sync::sanitize_preview,
        render: ds_cli_desktop::sync::render,
    },
    Entry {
        command: &ds_cli_desktop::sync::SANITIZE_APPLY_COMMAND,
        handler: ds_cli_desktop::sync::sanitize_apply,
        render: ds_cli_desktop::sync::render,
    },
    Entry {
        command: &ds_cli_desktop::published::COMMAND,
        handler: ds_cli_desktop::published::run,
        render: ds_cli_desktop::sync::render,
    },
    Entry {
        command: &ds_cli_desktop::sync::STATUS_COMMAND,
        handler: ds_cli_desktop::sync::status,
        render: ds_cli_desktop::sync::render,
    },
    Entry {
        command: &ds_cli_desktop::sync_plan::PLAN_COMMAND,
        handler: ds_cli_desktop::sync_plan::run,
        render: ds_cli_desktop::sync_plan::render,
    },
    Entry {
        command: &ds_cli_desktop::sync::RETRY_COMMAND,
        handler: ds_cli_desktop::sync::retry,
        render: ds_cli_desktop::sync::render,
    },
];

/// Feedback is a loop: report a gap, find it again once a session has
/// addressed it, say what it still waits on, and close it with what changed.
/// The order is the loop.
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
        command: &ds_cli_feedback::note::COMMAND,
        handler: ds_cli_feedback::note::run,
        render: ds_cli_feedback::note::render,
    },
    Entry {
        command: &ds_cli_feedback::close::COMMAND,
        handler: ds_cli_feedback::close::run,
        render: ds_cli_feedback::close::render,
    },
    Entry {
        command: &ds_cli_feedback::draft::COMMAND,
        handler: ds_cli_feedback::draft::run,
        render: ds_cli_feedback::draft::render,
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
    Entry {
        command: &ds_cli_workstation::local_data::STATUS_COMMAND,
        handler: ds_cli_workstation::local_data::status,
        render: ds_cli_workstation::local_data::render_status,
    },
    Entry {
        command: &ds_cli_workstation::local_data::CLEAN_COMMAND,
        handler: ds_cli_workstation::local_data::clean,
        render: ds_cli_workstation::local_data::render_clean,
    },
];

static MCP_ENTRIES: &[Entry] = &[
    Entry {
        command: &ds_cli_mcp::serve::COMMAND,
        handler: serve_mcp,
        render: ds_cli_mcp::serve::render,
    },
    Entry {
        command: &ds_cli_mcp::install::COMMAND,
        handler: ds_cli_mcp::install::run,
        render: ds_cli_mcp::install::render,
    },
];

static SERVER_ENTRIES: &[Entry] = &[
    Entry {
        command: &ds_cli_server::cache_retire::COMMAND,
        handler: ds_cli_server::cache_retire::command,
        render: ds_cli_server::render,
    },
    Entry {
        command: &ds_cli_server::ui_components::CAPTURE,
        handler: ds_cli_server::ui_components::capture,
        render: ds_cli_server::render,
    },
    Entry {
        command: &ds_cli_server::ui_components::MEMBER,
        handler: ds_cli_server::ui_components::member,
        render: ds_cli_server::render,
    },
    Entry {
        command: &ds_cli_server::component_lifecycle::ACTIVATE,
        handler: ds_cli_server::component_lifecycle::activate,
        render: ds_cli_server::render,
    },
    Entry {
        command: &ds_cli_server::component_lifecycle::ROLLBACK,
        handler: ds_cli_server::component_lifecycle::rollback,
        render: ds_cli_server::render,
    },
    Entry {
        command: &ds_cli_auth::core_identity::IDENTITY,
        handler: ds_cli_auth::core_identity::identity,
        render: ds_cli_server::render,
    },
    Entry {
        command: &ds_cli_auth::core_identity::AUTH_STATUS,
        handler: ds_cli_auth::core_identity::auth_status,
        render: ds_cli_server::render,
    },
    Entry {
        command: &ds_cli_server::core_components::REMOVE,
        handler: ds_cli_server::core_components::remove,
        render: ds_cli_server::render,
    },
    Entry {
        command: &ds_cli_server::core_components::SELECTION,
        handler: ds_cli_server::core_components::selection,
        render: ds_cli_server::render,
    },
    Entry {
        command: &ds_cli_server::module_updates::STAGE,
        handler: ds_cli_server::module_updates::stage,
        render: ds_cli_server::render,
    },
    Entry {
        command: &ds_cli_server::profile_session::COMMAND,
        handler: ds_cli_server::profile_session::command,
        render: ds_cli_server::render,
    },
    Entry {
        command: &ds_cli_server::desktop_update::INSPECT,
        handler: ds_cli_server::desktop_update::inspect,
        render: ds_cli_server::render,
    },
    Entry {
        command: &ds_cli_server::modules::INVENTORY,
        handler: ds_cli_server::modules::inventory,
        render: ds_cli_server::render,
    },
    Entry {
        command: &ds_cli_server::modules::RELEASE,
        handler: ds_cli_server::modules::release,
        render: ds_cli_server::render,
    },
    Entry {
        command: &ds_cli_server::ENGINE,
        handler: ds_cli_server::engine,
        render: ds_cli_server::render,
    },
    Entry {
        command: &ds_cli_server::SERVE,
        handler: ds_cli_server::serve,
        render: ds_cli_server::render,
    },
    Entry {
        command: &ds_cli_server::SUBMIT,
        handler: ds_cli_server::submit,
        render: ds_cli_server::render,
    },
    Entry {
        command: &ds_cli_server::SOLAR_SUBMIT,
        handler: ds_cli_server::solar_submit,
        render: ds_cli_server::render,
    },
    Entry {
        command: &ds_cli_server::TILE_SUBMIT,
        handler: ds_cli_server::tile_submit,
        render: ds_cli_server::render,
    },
    Entry {
        command: &ds_cli_server::STATUS,
        handler: ds_cli_server::status,
        render: ds_cli_server::render,
    },
    Entry {
        command: &ds_cli_server::ACTIVITY,
        handler: ds_cli_server::activity,
        render: ds_cli_server::render,
    },
    Entry {
        command: &ds_cli_server::survey_queue::COMMAND,
        handler: ds_cli_server::survey_queue::command,
        render: ds_cli_server::render,
    },
    Entry {
        command: &ds_cli_server::sync_read::READ,
        handler: ds_cli_server::sync_read::read,
        render: ds_cli_server::render,
    },
    Entry {
        command: &ds_cli_server::sync_recovery::STATUS,
        handler: ds_cli_server::sync_recovery::status,
        render: ds_cli_server::render,
    },
    Entry {
        command: &ds_cli_server::sync_recovery::RETRY,
        handler: ds_cli_server::sync_recovery::retry,
        render: ds_cli_server::render,
    },
    Entry {
        command: &ds_cli_server::sync_recovery::PREVIEW,
        handler: ds_cli_server::sync_recovery::preview,
        render: ds_cli_server::render,
    },
    Entry {
        command: &ds_cli_server::sync_recovery::APPLY,
        handler: ds_cli_server::sync_recovery::apply,
        render: ds_cli_server::render,
    },
    Entry {
        command: &ds_cli_server::CANCEL,
        handler: ds_cli_server::cancel,
        render: ds_cli_server::render,
    },
    Entry {
        command: &ds_cli_server::RESULT,
        handler: ds_cli_server::result,
        render: ds_cli_server::render,
    },
    Entry {
        command: &ds_cli_server::INPUT,
        handler: ds_cli_server::input,
        render: ds_cli_server::render,
    },
    // `server.layers.*` are retired here deliberately. One operation has one
    // command id whichever host executes it, so the drawer is
    // `ds map layer list|show|hide|reorder` with an explicit
    // `--target server|desktop[:instance]`; ds-cli-server keeps only the
    // owner-only socket those commands call for the Server target.
];

static MESSAGING_ENTRIES: &[Entry] = &[
    Entry {
        command: &ds_cli_messaging::INSTALLATIONS,
        handler: ds_cli_messaging::installations,
        render: ds_cli_messaging::render,
    },
    Entry {
        command: &ds_cli_messaging::REVOKE_INSTALLATION,
        handler: ds_cli_messaging::revoke_installation,
        render: ds_cli_messaging::render,
    },
    Entry {
        command: &ds_cli_messaging::CONFIG,
        handler: ds_cli_messaging::config,
        render: ds_cli_messaging::render,
    },
    Entry {
        command: &ds_cli_messaging::PEOPLE,
        handler: ds_cli_messaging::people,
        render: ds_cli_messaging::render,
    },
    Entry {
        command: &ds_cli_messaging::CONVERSATIONS,
        handler: ds_cli_messaging::conversations,
        render: ds_cli_messaging::render,
    },
    Entry {
        command: &ds_cli_messaging::PROJECT,
        handler: ds_cli_messaging::project,
        render: ds_cli_messaging::render,
    },
    Entry {
        command: &ds_cli_messaging::DIRECT,
        handler: ds_cli_messaging::direct,
        render: ds_cli_messaging::render,
    },
    Entry {
        command: &ds_cli_messaging::PERSONAL,
        handler: ds_cli_messaging::personal,
        render: ds_cli_messaging::render,
    },
    Entry {
        command: &ds_cli_messaging::READ,
        handler: ds_cli_messaging::read,
        render: ds_cli_messaging::render,
    },
    Entry {
        command: &ds_cli_messaging::SEND,
        handler: ds_cli_messaging::send,
        render: ds_cli_messaging::render,
    },
    Entry {
        command: &ds_cli_messaging::REPLY,
        handler: ds_cli_messaging::reply,
        render: ds_cli_messaging::render,
    },
    Entry {
        command: &ds_cli_messaging::MARK_READ,
        handler: ds_cli_messaging::mark_read,
        render: ds_cli_messaging::render,
    },
    Entry {
        command: &ds_cli_messaging::NOTIFICATIONS,
        handler: ds_cli_messaging::notifications,
        render: ds_cli_messaging::render,
    },
    Entry {
        command: &ds_cli_messaging::ACK,
        handler: ds_cli_messaging::acknowledge,
        render: ds_cli_messaging::render,
    },
];

static DOMAINS: &[Registered] = &[
    Registered { domain: &ds_cli_pm::checklist_notes::DOMAIN, entries: NOTES_ENTRIES },
    Registered {
        domain: &ds_cli_messaging::DOMAIN,
        entries: MESSAGING_ENTRIES,
    },
    Registered {
        domain: &ds_cli_server::DOMAIN,
        entries: SERVER_ENTRIES,
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
        domain: &ds_cli_pm::DOMAIN,
        entries: WORK_ENTRIES,
    },
    Registered {
        domain: &ds_cli_assets::DOMAIN,
        entries: ASSETS_ENTRIES,
    },
    Registered {
        domain: &ds_cli_design::DOMAIN,
        entries: DESIGN_ENTRIES,
    },
    Registered {
        domain: &ds_cli_installs::DOMAIN,
        entries: INSTALL_ENTRIES,
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
        domain: &ds_cli_auth::account::DOMAIN,
        entries: ACCOUNT_ENTRIES,
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

/// MCP consumes the same domain declarations as CLI capability discovery.
/// Root meta commands remain diagnostics, as in the tiered CLI catalogue.
fn serve_mcp(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    ds_cli_mcp::serve::run(
        inputs,
        DOMAINS
            .iter()
            .flat_map(|registered| registered.entries.iter())
            .map(|entry| entry.command),
        crate::build::identity(),
    )
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
    // The host the caller named rides beside the identity, not inside it: a
    // machine with no native profile has no identity and must still honour it.
    let _host = ds_cli_desktop::ops::scope_target(host_target(entry.command, &inputs));

    // A report command's explicit project is a touch of that project's
    // publication domain. The scoped kernel pass reads its remote heads on
    // every touch; no CLI timer or second freshness rule is involved. MCP
    // invokes this same binary dispatch path.
    match entry.command.id {
        "report.project.compounded" => ds_cli_report::project::combined::preflight(&inputs)?,
        "report.project.compute" => ds_cli_report::project::compute::preflight(&inputs)?,
        "report.project.export" => ds_cli_report::project::export::preflight(&inputs)?,
        "report.project.outputs.set" => ds_cli_report::project::settings::preflight_set(&inputs)?,
        "report.project.publish" => ds_cli_report::project::publish::preflight(&inputs)?,
        "report.preview.read" | "report.preview.refresh" => {
            ds_cli_report::retained_preview::preflight(&inputs)?
        }
        _ => {}
    }
    let touch = if report_project_touch(entry.command.id, &inputs) {
        // Reconciliation is a remote read and may advance held local state.
        // Establish a named-project identity before it runs for operations
        // that will require one; offline held reads remain credential-free.
        if !report_held_read(entry.command.id)
            && let Some(lane) = inputs.value("lane")
        {
            ds_cli_auth::headless_identity_for_named_project(lane)?;
        }
        Some(ds_cli_report::touch::project(
            inputs.require("lane")?,
            inputs.require("project")?,
            inputs.value("core-state-dir"),
        ))
    } else {
        None
    };
    if let Some(touch) = &touch
        && !touch.current
        && !report_held_read(entry.command.id)
    {
        return Err(Failure::unavailable(
            "report_reconciliation_required",
            "this project's reports could not be reconciled before publication or mutation",
        )
        .detail(touch.receipt.clone())
        .remedy("restore this machine's report sync connection, then retry the same command"));
    }
    let mut answer = (entry.handler)(&inputs, context)?;
    if let Some(touch) = touch
        && let Some(object) = answer.as_object_mut()
    {
        object.insert("reconciliation".to_owned(), touch.receipt);
    }
    Ok(answer)
}

fn report_project_touch(id: &str, inputs: &Inputs) -> bool {
    // A local proof promises no publication queue. The existing scoped pass
    // can upload older rows, so these modes wait for a read-only reconcile
    // hook instead of silently breaking that promise.
    if report_held_read(id)
        || id == "report.project.mv-setup.resolve"
        || id == "report.project.map-inputs"
        || id == "report.project.crs"
        || (id == "report.project.export"
            && (inputs.switch("dry-run")
                || inputs.value("held-preview").is_some()
                || inputs.value("preview-layout").is_some()
                || !inputs.repeated("print-layout").is_empty()))
    {
        return false;
    }
    id.starts_with("report.project.")
        || matches!(
            id,
            "report.publication.list" | "report.publication.show" | "report.artifact.remove"
        )
}

fn report_held_read(id: &str) -> bool {
    matches!(
        id,
        "report.project.scope"
            | "report.project.settings"
            | "report.project.archives"
            | "report.publication.list"
            | "report.publication.show"
    )
}

/// A declared command lane keeps its own default. Lane-less paired commands
/// may select the existing Core lane explicitly; absent selection stays Stable.
fn paired_auth_lane(
    command_lane: Option<&str>,
    core_lane: Option<&std::ffi::OsStr>,
) -> Result<ds_cli_auth::Lane, Failure> {
    if let Some(lane) = command_lane {
        return ds_cli_auth::Lane::parse(lane);
    }
    let Some(lane) = core_lane else {
        return Ok(ds_cli_auth::Lane::Stable);
    };
    let lane = lane.to_str().ok_or_else(|| {
        Failure::invalid("invalid_lane", "DS_CORE_LANE must be stable or canary")
            .remedy("set DS_CORE_LANE to stable or canary, or remove the explicit selection")
    })?;
    ds_cli_auth::Lane::parse(lane).map_err(|failure| {
        failure.remedy("set DS_CORE_LANE to stable or canary, or remove the explicit selection")
    })
}

fn scope_headless_identity(
    command: &Command,
    inputs: &Inputs,
) -> Result<ds_cli_desktop::ops::HeadlessIdentityGuard, Failure> {
    let core_lane = std::env::var_os("DS_CORE_LANE");
    scope_headless_identity_selected(command, inputs, core_lane.as_deref())
}

fn scope_headless_identity_selected(
    command: &Command,
    inputs: &Inputs,
    core_lane: Option<&std::ffi::OsStr>,
) -> Result<ds_cli_desktop::ops::HeadlessIdentityGuard, Failure> {
    scope_headless_identity_probed(
        command,
        inputs,
        core_lane,
        ds_cli_auth::probe_headless_identity,
        ds_cli_auth::probe_headless_identity_for_named_project,
    )
}

/// An explicitly pinned window owns context only when this command cannot
/// name a project itself. Session defaults retain saved-project arbitration.
fn window_supplies_project(command: &Command, inputs: &Inputs) -> Result<bool, Failure> {
    use ds_cli_desktop::ops::{DESCRIPTOR_ARG, Host, TARGET_ARG};
    if command.requires != ds_cli_contract::spec::Requires::Window
        || command.arg("project").is_some()
    {
        return Ok(false);
    }
    let named_instance = command
        .arg(TARGET_ARG.name)
        .filter(|arg| arg.value == TARGET_ARG.value)
        .and_then(|arg| inputs.value(arg.name))
        .map(|target| ds_cli_desktop::ops::host(Some(target)))
        .transpose()?
        .is_some_and(|host| matches!(host, Host::Desktop(Some(_))));
    let named_descriptor = command
        .arg(DESCRIPTOR_ARG.name)
        .filter(|arg| arg.value == DESCRIPTOR_ARG.value)
        .and_then(|arg| inputs.value(arg.name))
        .is_some();
    Ok(named_instance || named_descriptor)
}

fn scope_headless_identity_probed(
    command: &Command,
    inputs: &Inputs,
    core_lane: Option<&std::ffi::OsStr>,
    selected: impl FnOnce(
        &str,
    )
        -> Result<Option<(ds_cli_auth::ProviderIdentity, Option<String>)>, Failure>,
    named: impl FnOnce(&str) -> Result<Option<ds_cli_auth::ProviderIdentity>, Failure>,
) -> Result<ds_cli_desktop::ops::HeadlessIdentityGuard, Failure> {
    // Pure backend commands carry this observation through dispatch without
    // probing or requiring Desktop. If (and only if) a handler reaches the
    // shared typed bridge seam, `ops::invoke` snapshots the map, arbitrates,
    // and sends an atomic invocation fence.
    // `ds desktop list` is the enumeration every instance-targeted refusal
    // points at, and "which of these can serve my work" is a question only
    // this observation can answer. It declares no authority — it must answer
    // on a machine with nothing running, and gating it would make the one call
    // that explains the situation the one call that refuses to — so it is
    // named here rather than by its authority, and a probe that cannot answer
    // leaves it reporting the instances without marking any of them.
    let enumeration = command.id == ds_cli_desktop::list::COMMAND.id;
    // `ds dsgrid publish-version` declares the route it owns — native
    // publication with `--path` and an explicit project. Its paired fallback
    // without `--path` publishes the Desktop's working copy and is arbitrated
    // exactly as a `project` command always was: the Desktop's project must
    // be the caller's.
    let authority = if command.id == "dsgrid.publish-version" && inputs.value("path").is_none() {
        ds_cli_contract::Authority::Project
    } else {
        command.authority
    };
    if (!matches!(
        authority,
        ds_cli_contract::Authority::DesktopUser | ds_cli_contract::Authority::Project
    ) && !enumeration)
        || command.id == "auth.link.approve"
    {
        return Ok(ds_cli_desktop::ops::scope_headless_identity(None));
    }
    let window_project = window_supplies_project(command, inputs)?;
    let observed = match paired_auth_lane(inputs.value("lane"), core_lane).and_then(|lane| {
        if window_project {
            // The window named by the caller supplies its project through the
            // existing bridge snapshot; never read the unrelated saved address.
            named(lane.token()).map(|identity| identity.map(|identity| (identity, None)))
        } else {
            selected(lane.token())
        }
    }) {
        Ok(observed) => observed,
        Err(error) if headless_probe_means_absent(&error) || enumeration => None,
        Err(error) => return Err(error),
    };
    let observed = observed.map(
        |(identity, project)| ds_cli_desktop::ops::HeadlessIdentity {
            uid: identity.uid().to_owned(),
            lane: identity.lane().to_owned(),
            credential_audience_sha256: identity.credential_audience_sha256().to_owned(),
            project,
            command_authority: authority,
        },
    );
    Ok(ds_cli_desktop::ops::scope_headless_identity(observed))
}

/// The host this invocation named, for a command that declares the host flag.
///
/// `--target` is one flag with one grammar, and a handful of commands used the
/// word first for something in their own domain — a panel to open, an export
/// format. So the flag is recognised by its declaration rather than by its
/// name: only a `--target` declared with the host's own placeholder routes,
/// and `DS_TARGET` is that flag's session default, applying exactly where the
/// flag itself does.
fn host_target(command: &Command, inputs: &Inputs) -> Option<String> {
    let declared = command
        .arg(ds_cli_desktop::ops::TARGET_ARG.name)
        .filter(|arg| arg.value == ds_cli_desktop::ops::TARGET_ARG.value)?;
    inputs
        .value(declared.name)
        .map(str::to_owned)
        .or_else(ds_cli_desktop::ops::env_target)
}

fn headless_probe_means_absent(error: &Failure) -> bool {
    error.code() == "native_profile_not_configured"
        || error.code() == "native_state_protection_unavailable"
}

#[cfg(test)]
mod asset_download_tests {
    use super::*;

    #[test]
    fn the_download_descriptor_and_path_dispatch_the_same_read_adapter() {
        let entry = find_by_id("assets.download").expect("discoverable download read");
        let tokens = [
            "assets",
            "download",
            "--project",
            "p",
            "--asset",
            "a_4v6w339ffevs",
        ]
        .map(str::to_owned);
        let (by_path, consumed) = find_by_path(&tokens).expect("dispatchable download read");
        assert!(std::ptr::eq(entry, by_path));
        assert_eq!(consumed, 2);
        assert!(std::ptr::eq(
            entry.command,
            &ds_cli_assets::download::COMMAND
        ));
        assert!(std::ptr::fn_addr_eq(
            entry.handler,
            ds_cli_assets::download::run as Handler
        ));
        let inputs = ds_cli_contract::parse(entry.command, &tokens[consumed..]).unwrap();
        assert_eq!(inputs.value("project"), Some("p"));
        assert_eq!(inputs.value("asset"), Some("a_4v6w339ffevs"));
        assert!(!entry.command.confirmation_required_for(&inputs));
    }
}

#[cfg(test)]
mod identity_preflight_tests {
    use super::*;

    #[test]
    fn lane_less_map_preflight_uses_explicit_core_lane_and_keeps_stable_default() {
        use std::ffi::OsStr;
        for command in [
            &ds_cli_map::design::open::COMMAND,
            &ds_cli_map::design::process_setup::COMMAND,
        ] {
            assert!(command.arg("lane").is_none());
        }
        assert_eq!(
            paired_auth_lane(None, None).unwrap(),
            ds_cli_auth::Lane::Stable
        );
        for (text, expected) in [
            ("stable", ds_cli_auth::Lane::Stable),
            ("canary", ds_cli_auth::Lane::Canary),
        ] {
            assert_eq!(
                paired_auth_lane(None, Some(OsStr::new(text))).unwrap(),
                expected
            );
            // Declared lanes (including their declared defaults) take priority.
            assert_eq!(
                paired_auth_lane(Some(text), Some(OsStr::new("invalid"))).unwrap(),
                expected
            );
        }
        for invalid in ["", "local", "CANARY", " canary"] {
            let failure = paired_auth_lane(None, Some(OsStr::new(invalid))).unwrap_err();
            assert_eq!(failure.code(), "invalid_lane");
        }
    }

    #[test]
    fn enumeration_survives_invalid_selection_but_scoped_map_work_refuses() {
        let list = &ds_cli_desktop::list::COMMAND;
        let listing = ds_cli_contract::parse(list, &[]).unwrap();
        let open = &ds_cli_map::design::open::COMMAND;
        let opening =
            ds_cli_contract::parse(open, &["--transformer".into(), "sample".into()]).unwrap();
        for invalid in ["", "local", "invalid"] {
            let selected = Some(std::ffi::OsStr::new(invalid));
            assert!(scope_headless_identity_selected(list, &listing, selected).is_ok());
            let failure = scope_headless_identity_selected(open, &opening, selected)
                .err()
                .unwrap();
            assert_eq!(failure.code(), "invalid_lane");
        }
    }

    #[cfg(unix)]
    #[test]
    fn selected_non_unicode_core_lane_refuses_without_affecting_declared_lane() {
        use std::os::unix::ffi::OsStrExt;
        let lane = std::ffi::OsStr::from_bytes(&[0xff]);
        assert_eq!(
            paired_auth_lane(None, Some(lane)).unwrap_err().code(),
            "invalid_lane"
        );
        assert_eq!(
            paired_auth_lane(Some("canary"), Some(lane)).unwrap(),
            ds_cli_auth::Lane::Canary
        );
        let list = &ds_cli_desktop::list::COMMAND;
        let inputs = ds_cli_contract::parse(list, &[]).unwrap();
        assert!(scope_headless_identity_selected(list, &inputs, Some(lane)).is_ok());
    }

    const NAMED_INSTANCE: &str = "11111111111111111111111111111111";

    fn observed_for_window(
        command: &Command,
        inputs: &Inputs,
        window_project: bool,
    ) -> ds_cli_desktop::ops::HeadlessIdentityGuard {
        scope_headless_identity_probed(
            command,
            inputs,
            None,
            |lane| {
                assert!(
                    !window_project,
                    "a named window must not read the saved Core selection"
                );
                Ok(Some((
                    ds_cli_auth::ProviderIdentity::new(lane, &"a".repeat(64), "uid-1")?,
                    Some("NorthernHub".into()),
                )))
            },
            |lane| {
                assert!(
                    window_project,
                    "the existing default must retain its selected-project probe"
                );
                Ok(Some(ds_cli_auth::ProviderIdentity::new(
                    lane,
                    &"a".repeat(64),
                    "uid-1",
                )?))
            },
        )
        .unwrap()
    }

    #[test]
    fn only_explicit_window_context_without_a_project_input_skips_saved_selection() {
        let command = &ds_cli_map::design::list::COMMAND;
        for args in [
            vec!["--target".into(), format!("desktop:{NAMED_INSTANCE}")],
            vec!["--desktop-descriptor".into(), "fixture.json".into()],
        ] {
            let inputs = ds_cli_contract::parse(command, &args).unwrap();
            let _guard = observed_for_window(command, &inputs, true);
            let requirement = ds_cli_desktop::ops::scoped_requirement().unwrap();
            assert_eq!(requirement.project, None);
            assert_eq!(requirement.uid, "uid-1");
            assert_eq!(requirement.lane, "stable");
            assert_eq!(requirement.audience_sha256, "a".repeat(64));
            assert!(
                !requirement.project_independent,
                "the command still requires the window's project"
            );
        }
        for args in [
            vec![],
            vec!["--target".into(), "desktop".into()],
            vec!["--target".into(), "core".into()],
        ] {
            let inputs = ds_cli_contract::parse(command, &args).unwrap();
            let _guard = observed_for_window(command, &inputs, false);
            assert_eq!(
                ds_cli_desktop::ops::scoped_requirement()
                    .unwrap()
                    .project
                    .as_deref(),
                Some("NorthernHub")
            );
        }
        let switch = &ds_cli_desktop::project::SWITCH_COMMAND;
        let inputs = ds_cli_contract::parse(
            switch,
            &[
                "--project".into(),
                "Gisagara".into(),
                "--target".into(),
                format!("desktop:{NAMED_INSTANCE}"),
            ],
        )
        .unwrap();
        let _guard = observed_for_window(switch, &inputs, false);
        assert_eq!(
            ds_cli_desktop::ops::scoped_requirement()
                .unwrap()
                .project
                .as_deref(),
            Some("NorthernHub")
        );
        let backend = &ds_cli_server::STATUS;
        let inputs =
            ds_cli_contract::parse(backend, &["--project".into(), "Gisagara".into()]).unwrap();
        let _guard = scope_headless_identity_probed(
            backend,
            &inputs,
            None,
            |_| panic!("headless path unchanged"),
            |_| panic!("headless path must not use a window"),
        )
        .unwrap();
        assert!(ds_cli_desktop::ops::scoped_requirement().is_none());
    }

    /// A real typed bridge transport, with a fake application's session and
    /// atomic dispatch response. No protected state or live window is touched.
    fn window_bridge(
        session: Value,
        invocations: usize,
        switched: bool,
    ) -> (
        ds_cli_desktop::discover::Descriptor,
        std::thread::JoinHandle<Vec<Value>>,
    ) {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
        listener.set_nonblocking(true).unwrap();
        let address = listener.local_addr().unwrap();
        let worker = std::thread::spawn(move || {
            let mut posted = Vec::new();
            for index in 0..=invocations {
                let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
                let mut stream = loop {
                    match listener.accept() {
                        Ok((stream, _)) => break stream,
                        Err(error)
                            if error.kind() == std::io::ErrorKind::WouldBlock
                                && std::time::Instant::now() < deadline =>
                        {
                            std::thread::sleep(std::time::Duration::from_millis(5))
                        }
                        Err(error) => {
                            panic!("bridge fixture did not receive its expected request: {error}")
                        }
                    }
                };
                stream
                    .set_read_timeout(Some(std::time::Duration::from_secs(2)))
                    .unwrap();
                let mut bytes = Vec::new();
                let mut buffer = [0u8; 4096];
                let header_end = loop {
                    let count = stream.read(&mut buffer).unwrap();
                    assert!(count > 0);
                    bytes.extend_from_slice(&buffer[..count]);
                    if let Some(end) = bytes.windows(4).position(|part| part == b"\r\n\r\n") {
                        break end + 4;
                    }
                };
                let headers = std::str::from_utf8(&bytes[..header_end]).unwrap();
                let size = headers
                    .lines()
                    .filter_map(|line| line.split_once(':'))
                    .find_map(|(name, value)| {
                        name.eq_ignore_ascii_case("content-length")
                            .then(|| value.trim().parse::<usize>().unwrap())
                    })
                    .unwrap_or(0);
                while bytes.len() < header_end + size {
                    let count = stream.read(&mut buffer).unwrap();
                    assert!(count > 0);
                    bytes.extend_from_slice(&buffer[..count]);
                }
                let (status, response) = if index == 0 {
                    (200, session.clone())
                } else {
                    let request: Value =
                        serde_json::from_slice(&bytes[header_end..header_end + size]).unwrap();
                    // The view switched after its session snapshot. The app
                    // refuses the old project/revision instead of dispatching.
                    let response = if switched {
                        assert_eq!(request["identity_fence"]["project"], "Gisagara");
                        (
                            409,
                            serde_json::json!({"error":{"class":"conflict","code":"context_generation_stale","message":"window project changed before dispatch","remedy":"retry the intended view"}}),
                        )
                    } else {
                        (200, serde_json::json!({"project":"Gisagara"}))
                    };
                    posted.push(request);
                    response
                };
                let body = serde_json::to_vec(&response).unwrap();
                write!(stream,"HTTP/1.1 {status} {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",if status==200{"OK"}else{"Conflict"},body.len()).unwrap();
                stream.write_all(&body).unwrap();
            }
            posted
        });
        let descriptor = ds_cli_desktop::discover::Descriptor {
            url: format!("http://{address}"),
            token: "fixture-only".into(),
            pid: std::process::id(),
            instance_id: NAMED_INSTANCE.into(),
            identity: ds_command_kernel::desktop_instance::Identity::Minted,
            profile: Some("stable".into()),
            path: "fixture-only.json".into(),
            window: None,
        };
        (descriptor, worker)
    }

    #[test]
    fn named_window_project_keeps_identity_and_atomic_window_fences() {
        use serde_json::json;
        let command = &ds_cli_map::design::list::COMMAND;
        for case in ["same", "uid", "lane", "audience", "switched", "default"] {
            let explicit = case != "default";
            let args = if explicit {
                vec!["--target".into(), format!("desktop:{NAMED_INSTANCE}")]
            } else {
                vec![]
            };
            let inputs = ds_cli_contract::parse(command, &args).unwrap();
            let _identity = observed_for_window(command, &inputs, explicit);
            let _target = ds_cli_desktop::ops::scope_target(
                explicit.then(|| format!("desktop:{NAMED_INSTANCE}")),
            );
            let mut session = json!({"uid":"uid-1","lane":"stable","credential_audience_sha256":"a".repeat(64),"project":"Gisagara","session_revision":7,"windows":[{"label":ds_cli_desktop::discover::OWNER_WINDOW_LABEL,"project":"Gisagara","generation":3}]});
            match case {
                "uid" => session["uid"] = json!("other-account"),
                "lane" => session["lane"] = json!("canary"),
                "audience" => session["credential_audience_sha256"] = json!("b".repeat(64)),
                _ => {}
            }
            let invokes = usize::from(matches!(case, "same" | "switched"));
            let (descriptor, worker) = window_bridge(session, invokes, case == "switched");
            let response = ds_cli_desktop::ops::invoke(
                &descriptor,
                &ds_cli_map::DESIGN_LIST,
                json!({}),
                std::time::Duration::from_secs(2),
            );
            let posted = worker.join().unwrap();
            match case {
                "same" => {
                    assert_eq!(response.unwrap()["project"], "Gisagara");
                    assert_eq!(posted[0]["identity_fence"]["project"], "Gisagara");
                    assert_eq!(posted[0]["identity_fence"]["session_revision"], 7);
                    assert_eq!(posted[0]["identity_fence"]["context_generation"], 3);
                    assert!(posted[0]["arguments"].get("identity_fence").is_none());
                }
                "switched" => assert_eq!(response.unwrap_err().code(), "context_generation_stale"),
                "default" => assert_eq!(response.unwrap_err().code(), "desktop_project_not_open"),
                _ => assert_eq!(response.unwrap_err().code(), "auth_context_mismatch"),
            }
        }
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
}

#[cfg(test)]
mod report_touch_tests {
    use super::*;

    fn inputs(command: &Command, args: &[&str]) -> Inputs {
        ds_cli_contract::parse(
            command,
            &args.iter().map(|arg| (*arg).to_owned()).collect::<Vec<_>>(),
        )
        .expect("declared inputs")
    }

    #[test]
    fn held_transformer_preview_does_not_reconcile_unrelated_report_publications() {
        let held = inputs(
            &ds_cli_report::project::export::COMMAND,
            &[
                "--project",
                "p",
                "--lane",
                "canary",
                "--held-preview",
                "/tmp/held-preview.json",
                "--out-dir",
                "/tmp/held-preview-scratch",
            ],
        );
        assert!(!report_project_touch("report.project.export", &held));
        assert!(!report_held_read("report.project.export"));

        let delivery = inputs(
            &ds_cli_report::project::export::COMMAND,
            &[
                "--project",
                "p",
                "--lane",
                "canary",
                "--out-dir",
                "/tmp/delivery",
            ],
        );
        assert!(report_project_touch("report.project.export", &delivery));
    }

    #[test]
    fn publication_touches_the_exact_project_but_reads_and_local_proofs_do_not_publish() {
        let scope = inputs(&ds_cli_report::project::scope::COMMAND, &["--project", "p"]);
        assert!(!report_project_touch("report.project.scope", &scope));
        assert!(!report_project_touch("report.project.settings", &scope));
        let crs = inputs(
            &ds_cli_report::project::crs::COMMAND,
            &["--project", "p", "--lane", "stable"],
        );
        assert!(!report_project_touch("report.project.crs", &crs));
        assert!(!report_held_read("report.project.crs"));
        assert!(!report_project_touch(
            "report.project.mv-setup.resolve",
            &scope
        ));
        assert!(report_held_read("report.project.scope"));

        let publish = inputs(
            &ds_cli_report::project::publish::COMMAND,
            &["--project", "p", "--from", "/tmp/prints"],
        );
        assert!(report_project_touch("report.project.publish", &publish));
        assert!(!report_held_read("report.project.publish"));

        let dry = inputs(
            &ds_cli_report::project::export::COMMAND,
            &["--project", "p", "--out-dir", "/tmp/proof", "--dry-run"],
        );
        assert!(!report_project_touch("report.project.export", &dry));

        let ordinary = inputs(
            &ds_cli_report::project::export::COMMAND,
            &["--project", "p", "--out-dir", "/tmp/prints"],
        );
        assert!(report_project_touch("report.project.export", &ordinary));

        let inventory = inputs(
            &ds_cli_report::outbox::INVENTORY,
            &["--project", "p", "--transformer", "tx_1"],
        );
        assert!(!report_project_touch("report.outbox.inventory", &inventory));
        assert!(!report_held_read("report.outbox.inventory"));
    }
}
