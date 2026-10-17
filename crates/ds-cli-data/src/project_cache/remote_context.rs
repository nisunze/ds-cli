//! Seed-only opt-in to the existing governed print-context acquisition door.
//! The default provider and other consumers remain local-input-only.
use super::*;
use ds_cli_auth::{ContourParameters, PrintContextKind};
use ds_project_data::Failure as Cause;

pub(super) fn validate_source_mode(inputs: &Inputs) -> Result<(), Failure> {
    if inputs.switch("remote-context")
        && (inputs.value("local-source").is_some() || inputs.value("publisher-input").is_some())
    {
        return Err(Failure::invalid(
            INVALID_SCOPE.code,
            "--remote-context cannot be combined with --local-source or --publisher-input",
        )
        .remedy("Select exactly one seed source mode"));
    }
    Ok(())
}

pub(super) struct SeedProvider<'a> {
    pub(super) local: CliProvider<'a>,
    pub(super) enabled: bool,
}

impl Provider for SeedProvider<'_> {
    fn acquire(&mut self, dataset: &Dataset, area: &Value) -> Result<Acquisition, Cause> {
        self.acquire_with(dataset, area, ds_cli_auth::data_distribution)
    }
}

impl SeedProvider<'_> {
    fn acquire_with(
        &self,
        dataset: &Dataset,
        area: &Value,
        mut read: impl FnMut(&str, &str, &DataDistributionRequest) -> Result<Value, Failure>,
    ) -> Result<Acquisition, Cause> {
        let kind = if self.enabled
            && dataset.provider == ds_project_data::datasets::BRAIN_PRINT_CONTEXT_PROVIDER
        {
            match dataset.id.as_str() {
                policy::BUILDINGS_DATASET_ID => Some(PrintContextKind::GoogleOpenBuildings),
                policy::CONTOURS_DATASET_ID => Some(PrintContextKind::ElevationContours),
                _ => None,
            }
        } else {
            None
        };
        let Some(kind) = kind else {
            return self.local.acquire_with(dataset, area, |request| {
                read(self.local.lane, self.local.project, request)
            });
        };
        if dataset
            .parameters
            .get("context_kind")
            .and_then(Value::as_str)
            != Some(kind.token())
        {
            return Err(Cause::AcquisitionFailed(
                "dataset context_kind does not match its exact built-in identity".into(),
            ));
        }
        let contour_parameters = if kind == PrintContextKind::ElevationContours {
            let parameters: ContourParameters = serde_json::from_value(json!({
                "minor_interval_m": dataset.parameters.get("minor_interval_m"),
                "index_interval_m": dataset.parameters.get("index_interval_m"),
                "sample_spacing_m": dataset.parameters.get("sample_spacing_m"),
            }))
            .map_err(|error| {
                Cause::AcquisitionFailed(format!("authored contour parameters: {error}"))
            })?;
            parameters.validate().map_err(|error| {
                Cause::AcquisitionFailed(format!("authored contour parameters: {error}"))
            })?;
            Some(parameters)
        } else {
            None
        };
        let request = DataDistributionRequest::QueryPrintContext {
            context_kind: kind,
            area: area.clone(),
            contour_parameters,
        };
        let response = read(self.local.lane, self.local.project, &request).map_err(|error| {
            let message = format!("{}: {}", error.code(), error.message());
            match error.code() {
                "data_distribution_unavailable" | "auth_transient" => {
                    Cause::ProviderUnavailable(message)
                }
                _ => Cause::AcquisitionFailed(message),
            }
        })?;
        let decoded = match kind {
            PrintContextKind::GoogleOpenBuildings => policy::decode_buildings(&response, area),
            PrintContextKind::ElevationContours => policy::decode_contours(&response, area),
        }
        .map_err(Cause::AcquisitionFailed)?;
        Ok(decoded_page(decoded))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    fn provider(enabled: bool) -> SeedProvider<'static> {
        SeedProvider {
            local: CliProvider {
                lane: "canary",
                project: "fixture-project",
            },
            enabled,
        }
    }
    fn contours() -> Dataset {
        ds_project_data::contours_dataset(ds_project_data::ContourSettings {
            minor_interval_m: 10,
            index_interval_m: 50,
            sample_spacing_m: 12.5,
        })
    }
    fn area() -> Value {
        json!({"type":"Polygon","coordinates":[[[30.,-2.],[30.001,-2.],[30.001,-1.999],[30.,-1.999],[30.,-2.]]]})
    }
    fn response(kind: PrintContextKind) -> Value {
        match kind {
            PrintContextKind::GoogleOpenBuildings => json!({"status":"success","provider":"google",
                "layers":{"buildings_context":{"type":"FeatureCollection","features":[]}},
                "counts":{"buildings_context":0},"coverage":{"truncated":false,"dataset_covers_area":null},
                "source_receipt":{"dataset_version":"fixture-v3","asset":"provider asset"}}),
            PrintContextKind::ElevationContours => json!({"status":"success",
                "layers":{"contours_intermediate":{"type":"FeatureCollection","features":[]},
                    "contours_index":{"type":"FeatureCollection","features":[]}},
                "source":"rwanda_dem_10m","source_version":"fixture-dem-v2",
                "minor_interval_m":10,"index_interval_m":50,"effective_spacing_m":12.5,
                "grid":[2,2],"sampled_points":4,"unresolved_points":0,
                "intermediate_features":0,"index_features":0}),
        }
    }

    #[test]
    fn opt_in_forwards_exact_typed_context_project_area_and_authored_parameters() {
        for (dataset, kind) in [
            (
                ds_project_data::buildings_dataset(),
                PrintContextKind::GoogleOpenBuildings,
            ),
            (contours(), PrintContextKind::ElevationContours),
        ] {
            let original = dataset.clone();
            let bounds = area();
            let mut calls = 0;
            let page = provider(true).acquire_with(&dataset, &bounds, |lane, project, request| {
                calls += 1;
                assert_eq!(lane, "canary");
                assert_eq!(project, "fixture-project");
                let mut expected = json!({"action":"query_print_context","context_kind":kind.token(),"area":bounds});
                if kind == PrintContextKind::ElevationContours {
                    expected["contour_parameters"] = json!({"minor_interval_m":10,"index_interval_m":50,"sample_spacing_m":12.5});
                }
                assert_eq!(serde_json::to_value(request).unwrap(), expected);
                Ok(response(kind))
            }).unwrap();
            assert_eq!(calls, 1);
            assert_eq!(dataset, original, "room identity stays unchanged");
            assert!(page.features.is_empty());
            assert_eq!(
                page.source_version,
                if kind == PrintContextKind::GoogleOpenBuildings {
                    "fixture-v3"
                } else {
                    "fixture-dem-v2"
                }
            );
            assert_ne!(page.source_receipt, Value::Null);
        }
    }

    #[test]
    fn missing_invalid_parameters_and_wrong_identity_never_call_remote() {
        let base = contours();
        let mut invalid = Vec::new();
        for (key, value) in [
            ("minor_interval_m", Value::Null),
            ("minor_interval_m", json!(0)),
            ("minor_interval_m", json!(1.5)),
            ("index_interval_m", json!(55)),
            ("index_interval_m", json!(1001)),
            ("sample_spacing_m", json!(4.9)),
            ("sample_spacing_m", json!(251)),
        ] {
            let mut row = base.clone();
            row.parameters.insert(key.into(), value);
            invalid.push(row);
        }
        let mut missing = base.clone();
        missing.parameters.remove("sample_spacing_m");
        invalid.push(missing);
        let mut wrong_id = base.clone();
        wrong_id.id = "other-contours".into();
        invalid.push(wrong_id);
        let mut wrong_provider = base.clone();
        wrong_provider.provider = "other-provider".into();
        invalid.push(wrong_provider);
        let mut wrong_kind = base.clone();
        wrong_kind
            .parameters
            .insert("context_kind".into(), json!("google_open_buildings"));
        invalid.push(wrong_kind);
        for dataset in invalid {
            let mut calls = 0;
            assert!(
                provider(true)
                    .acquire_with(&dataset, &area(), |_, _, _| {
                        calls += 1;
                        Ok(Value::Null)
                    })
                    .is_err()
            );
            assert_eq!(calls, 0, "{} {:?}", dataset.id, dataset.parameters);
        }
        for dataset in [base, ds_project_data::buildings_dataset()] {
            let mut calls = 0;
            assert!(
                provider(false)
                    .acquire_with(&dataset, &area(), |_, _, _| {
                        calls += 1;
                        Ok(Value::Null)
                    })
                    .is_err()
            );
            assert_eq!(calls, 0);
        }
    }

    #[test]
    fn incomplete_response_and_provider_refusal_never_retry_or_substitute() {
        let mut calls = 0;
        let failure = provider(true)
            .acquire_with(&ds_project_data::buildings_dataset(), &area(), |_, _, _| {
                calls += 1;
                let mut partial = response(PrintContextKind::GoogleOpenBuildings);
                partial["coverage"]["truncated"] = json!(true);
                Ok(partial)
            })
            .unwrap_err();
        assert_eq!(failure.code(), "project_dataset_acquisition_failed");
        assert_eq!(calls, 1);
        let failure = provider(true)
            .acquire_with(&contours(), &area(), |_, _, _| {
                calls += 1;
                Err(Failure::unavailable(
                    "data_distribution_unavailable",
                    "fixture unavailable",
                ))
            })
            .unwrap_err();
        assert_eq!(failure.code(), "dataset_provider_unavailable");
        assert_eq!(calls, 2);
    }

    #[test]
    fn remote_source_conflicts_refuse_before_auth_or_manifest_io() {
        for flag in ["--local-source", "--publisher-input"] {
            let tokens = [
                "--project",
                "fixture-project",
                "--remote-context",
                flag,
                "/nonexistent/never-read.json",
            ];
            let inputs =
                ds_cli_contract::args::parse(&SEED_COMMAND, &tokens.map(str::to_owned)).unwrap();
            let error = run_seed(
                &inputs,
                &Context {
                    confirmed: true,
                    output: ds_cli_contract::Output::resolve(
                        ds_cli_contract::Format::Json,
                        false,
                        true,
                    ),
                },
            )
            .unwrap_err();
            assert_eq!(error.code(), INVALID_SCOPE.code);
            assert!(
                error
                    .message()
                    .contains("--remote-context cannot be combined")
            );
        }
        assert!(
            !STATUS_COMMAND
                .args
                .iter()
                .any(|arg| arg.name == "remote-context")
        );
        assert!(
            !QUERY_COMMAND
                .args
                .iter()
                .any(|arg| arg.name == "remote-context")
        );
    }

    #[test]
    fn opted_in_warm_seed_reuses_rooms_without_remote_acquisition() {
        struct Scripted {
            calls: Cell<usize>,
            provider: SeedProvider<'static>,
        }
        impl Provider for Scripted {
            fn acquire(&mut self, dataset: &Dataset, area: &Value) -> Result<Acquisition, Cause> {
                self.provider.acquire_with(dataset, area, |_, _, request| {
                    self.calls.set(self.calls.get() + 1);
                    let DataDistributionRequest::QueryPrintContext { context_kind, .. } = request
                    else {
                        panic!("wrong acquisition kind")
                    };
                    Ok(response(*context_kind))
                })
            }
        }
        let root = std::env::temp_dir().join(format!(
            "ds-cli-remote-context-{}-{}",
            std::process::id(),
            ds_command_kernel::report_export::sha256_hex(
                format!("{:?}", std::time::SystemTime::now()).as_bytes()
            )
        ));
        std::fs::create_dir_all(&root).unwrap();
        let scope = Scope {
            principal: "fixture-user".into(),
            project: "fixture-project".into(),
        };
        let extents = vec![policy::Extent {
            name: "T1".into(),
            bounds: Some([30., -2., 30.001, -1.999]),
        }];
        let policy = buffer_policy(&[200.]).unwrap();
        let mut provider = Scripted {
            calls: Cell::new(0),
            provider: provider(true),
        };
        let mut fetch =
            |_: &BundleReceipt, _: &Path| -> Result<(), String> { panic!("no bundle transfer") };
        for dataset in [ds_project_data::buildings_dataset(), contours()] {
            let source = if dataset.id == policy::BUILDINGS_DATASET_ID {
                ds_project_data::DeclaredSource::Buildings
            } else {
                ds_project_data::DeclaredSource::Contours(ds_project_data::ContourSettings {
                    minor_interval_m: 10,
                    index_interval_m: 50,
                    sample_spacing_m: 12.5,
                })
            };
            let mut hosts = Hosts {
                provider: &mut provider,
                fetch: &mut fetch,
            };
            let first = ds_project_data::ensure(
                &root,
                &scope,
                &dataset,
                &source,
                &policy,
                &extents,
                &[],
                Mode::Acquire(&mut hosts),
            )
            .unwrap();
            assert!(first.covered);
            assert!(first.acquired > 0);
            let previous = provider.calls.get();
            let mut hosts = Hosts {
                provider: &mut provider,
                fetch: &mut fetch,
            };
            let warm = ds_project_data::ensure(
                &root,
                &scope,
                &dataset,
                &source,
                &policy,
                &extents,
                &[],
                Mode::Acquire(&mut hosts),
            )
            .unwrap();
            assert!(warm.covered);
            assert_eq!(warm.acquired, 0);
            assert_eq!(provider.calls.get(), previous);
        }
        std::fs::remove_dir_all(root).unwrap();
    }
}
