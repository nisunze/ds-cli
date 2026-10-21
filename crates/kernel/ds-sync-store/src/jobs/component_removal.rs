//! Core component removal shares the writer fence with job pin capture.
use super::*;
use ds_command_kernel::{module_install::ActivationGuard, module_release::ModuleComponent};

#[cfg(test)]
mod tests;

impl Store {
    /// Observe every pending job for this connection and component, then run
    /// the atomic selection removal while still holding admission's IMMEDIATE
    /// database transaction. A racing submission captures either before this
    /// observation or after removal; it cannot capture between them.
    ///
    /// This is Core's job-only lifecycle boundary: Core owns no native editor
    /// sessions or held edits, and this selection change needs no host restart.
    /// Captured jobs keep their immutable executable. Historical or misbound
    /// pending jobs are retained and refuse removal until explicitly resolved.
    /// Project narrowing is deliberately ignored: removal affects the whole
    /// component/lane, including contextless legacy rows invisible to project
    /// lists. The census streams rows without a UI page limit or sealed inputs.
    ///
    /// `remove` may take only the module-store lock and replace the selection.
    /// It must not reenter this database, submit work, acquire an engine, or
    /// observe Desktop. All capture paths must keep the existing lock order:
    /// database admission writer first, module-store lock second.
    pub fn with_core_component_removal<T>(
        &mut self,
        caller: &JobCaller<'_>,
        component: ModuleComponent,
        remove: impl FnOnce(ActivationGuard) -> Result<T>,
    ) -> Result<T> {
        if caller.owner.is_empty()
            || caller.uid.is_empty()
            || caller.deployment.is_empty()
            || !matches!(caller.lane, "stable" | "canary")
        {
            return Err(Error::Kernel("component_removal_context_invalid".into()));
        }
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        {
            let mut statement = transaction.prepare(
                "SELECT row FROM compute_jobs WHERE owner=?1 AND lane=?2 AND phase IN ('queued','running')",
            )?;
            let rows = statement.query_map(params![caller.owner, caller.lane], |row| {
                row.get::<_, String>(0)
            })?;
            for row in rows {
                let job: Job = serde_json::from_str(&row?)?;
                let uses_component = matches!(
                    (&job.engine, component),
                    (
                        compute_jobs::EngineKind::LvProcessing,
                        ModuleComponent::Network
                    ) | (
                        compute_jobs::EngineKind::SolarPrepared,
                        ModuleComponent::Solar
                    )
                );
                if !uses_component {
                    continue;
                }
                if job.owner != caller.owner
                    || job.lane != caller.lane
                    || job.context.as_ref().is_some_and(|context| {
                        context.principal_uid != caller.uid
                            || context.deployment != caller.deployment
                    })
                    || compute_jobs::validate_engine_pin(&job).is_err()
                {
                    return Err(Error::Refused(Refusal {
                        code: "component_uncaptured_jobs",
                        message: format!(
                            "this component has retained pending work without a matching captured native engine; cancel that work before removal; to run it again, {}",
                            compute_jobs::ENGINE_PIN_REMEDY
                        ),
                        retry_after_ms: None,
                        scope: None,
                    }));
                }
            }
        }
        // These are facts of this Core job host, never client-authored idle
        // assertions or observations about another native host's sessions.
        let value = remove(ActivationGuard {
            unsaved_edits: false,
            uncaptured_sessions: 0,
            host_restart_required: false,
        })?;
        transaction.commit()?;
        Ok(value)
    }
}
