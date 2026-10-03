use ds_cli_contract::spec::Arg;

#[test]
fn consolidated_lanes_preserve_every_previous_field() {
    assert_eq!(
        format!(
            "{:?}",
            Arg::value("lane", "<stable|canary>", "Deployment lane.")
                .default("stable")
                .choices(&["stable", "canary"])
        ),
        format!(
            "{:?}",
            ds_cli_contract::spec::LANE.summary("Deployment lane.")
        )
    );
    assert_eq!(
        format!(
            "{:?}",
            Arg::value(
                "lane",
                "<stable|canary>",
                "Deployment lane; stable is the default.",
            )
            .default("stable")
            .choices(&["stable", "canary"])
        ),
        format!(
            "{:?}",
            ds_cli_contract::spec::LANE.summary("Deployment lane; stable is the default.")
        )
    );
    assert_eq!(
        format!(
            "{:?}",
            Arg::value("lane", "<stable|canary>", "Native credential lane.")
                .choices(&["stable", "canary"])
                .default("stable")
        ),
        format!("{:?}", ds_cli_contract::spec::LANE)
    );
    assert_eq!(
        format!(
            "{:?}",
            Arg::value("lane", "<stable|canary>", "Native authentication lane.")
                .default("stable")
                .choices(&["stable", "canary"])
        ),
        format!(
            "{:?}",
            ds_cli_contract::spec::LANE.summary("Native authentication lane.")
        )
    );
    assert_eq!(
        format!(
            "{:?}",
            ds_cli_contract::spec::Arg::value("lane", "<stable|canary>", "Native deployment lane.")
                .default("stable")
                .choices(&["stable", "canary"])
        ),
        format!(
            "{:?}",
            ds_cli_contract::spec::LANE.summary("Native deployment lane.")
        )
    );
    assert_eq!(
        format!(
            "{:?}",
            Arg::value("lane", "<lane>", "Native credential lane.")
                .choices(&["canary", "stable"])
                .default("canary")
        ),
        format!(
            "{:?}",
            ds_cli_contract::spec::LANE
                .placeholder("<lane>")
                .choices(&["canary", "stable"])
                .default("canary")
        )
    );
    assert_eq!(
        format!(
            "{:?}",
            Arg::value("lane", "<stable|canary>", "Which lane's catalogue.")
                .default("stable")
                .choices(&["stable", "canary"])
        ),
        format!(
            "{:?}",
            ds_cli_contract::spec::LANE.summary("Which lane's catalogue.")
        )
    );
    assert_eq!(
        format!(
            "{:?}",
            Arg::value("lane", "<stable|canary>", "Request's deployment lane.")
                .default("stable")
                .choices(&["stable", "canary"])
        ),
        format!(
            "{:?}",
            ds_cli_contract::spec::LANE.summary("Request's deployment lane.")
        )
    );
    assert_eq!(
        format!(
            "{:?}",
            Arg::value("lane", "<stable|canary>", "Native user lane.")
                .default("stable")
                .choices(&["stable", "canary"])
        ),
        format!(
            "{:?}",
            ds_cli_contract::spec::Arg {
                summary: "Native user lane.",
                ..ds_cli_contract::spec::LANE
            }
        )
    );
    assert_eq!(
        format!(
            "{:?}",
            Arg::value(
                "lane",
                "<stable|canary>",
                "Native credential lane for the entire captured run.",
            )
            .default("stable")
            .choices(&["stable", "canary"])
        ),
        format!(
            "{:?}",
            ds_cli_contract::spec::Arg {
                summary: "Native credential lane for the entire captured run.",
                ..ds_cli_contract::spec::LANE
            }
        )
    );
    assert_eq!(
        format!(
            "{:?}",
            Arg::value(
                "lane",
                "<stable|canary>",
                "Native user lane matching the source receipt.",
            )
            .default("stable")
            .choices(&["stable", "canary"])
        ),
        format!(
            "{:?}",
            ds_cli_contract::spec::Arg {
                summary: "Native user lane matching the source receipt.",
                ..ds_cli_contract::spec::LANE
            }
        )
    );
    assert_eq!(
        format!(
            "{:?}",
            Arg::value(
                "lane",
                "<stable|canary>",
                "Deployment lane; stable is the default.",
            )
            .default("stable")
            .choices(&["stable", "canary"])
        ),
        format!(
            "{:?}",
            ds_cli_contract::spec::Arg {
                summary: "Deployment lane; stable is the default.",
                ..ds_cli_contract::spec::LANE
            }
        )
    );
    assert_eq!(
        format!(
            "{:?}",
            Arg::value("lane", "<stable|canary>", "Signed-in lane; default stable.")
                .choices(&["stable", "canary"])
        ),
        format!(
            "{:?}",
            ds_cli_contract::spec::Arg {
                summary: "Signed-in lane; default stable.",
                default: None,
                ..ds_cli_contract::spec::LANE
            }
        )
    );
    assert_eq!(
        format!(
            "{:?}",
            Arg::value(
                "lane",
                "<stable|canary>",
                "Native authentication lane; defaults to stable.",
            )
        ),
        format!(
            "{:?}",
            ds_cli_contract::spec::Arg {
                summary: "Native authentication lane; defaults to stable.",
                default: None,
                choices: &[],
                ..ds_cli_contract::spec::LANE
            }
        )
    );
    assert_eq!(
        format!(
            "{:?}",
            Arg::value("lane", "<stable|canary>", "Native authentication lane.")
                .default("stable")
                .choices(&["stable", "canary"])
        ),
        format!(
            "{:?}",
            ds_cli_contract::spec::Arg {
                summary: "Native authentication lane.",
                ..ds_cli_contract::spec::LANE
            }
        )
    );
    assert_eq!(
        format!(
            "{:?}",
            Arg::value(
                "lane",
                "<stable|canary>",
                "Reserved publication lane; local calculation uses owned inputs.",
            )
        ),
        format!(
            "{:?}",
            ds_cli_contract::spec::Arg {
                summary: "Reserved publication lane; local calculation uses owned inputs.",
                default: None,
                choices: &[],
                ..ds_cli_contract::spec::LANE
            }
        )
    );
    assert_eq!(
        format!(
            "{:?}",
            Arg::value(
                "lane",
                "<stable|canary>",
                "Native authority lane; defaults to stable.",
            )
        ),
        format!(
            "{:?}",
            ds_cli_contract::spec::Arg {
                summary: "Native authority lane; defaults to stable.",
                default: None,
                choices: &[],
                ..ds_cli_contract::spec::LANE
            }
        )
    );
    assert_eq!(
        format!(
            "{:?}",
            Arg::value("lane", "<lane>", "stable or canary; default stable.")
        ),
        format!(
            "{:?}",
            ds_cli_contract::spec::Arg {
                summary: "stable or canary; default stable.",
                value: "<lane>",
                default: None,
                choices: &[],
                ..ds_cli_contract::spec::LANE
            }
        )
    );
    assert_eq!(
        format!(
            "{:?}",
            Arg::value(
                "lane",
                "<stable|canary>",
                "Credential lane for shared asset reads; default stable.",
            )
            .choices(&["stable", "canary"])
        ),
        format!(
            "{:?}",
            ds_cli_contract::spec::Arg {
                summary: "Credential lane for shared asset reads; default stable.",
                default: None,
                ..ds_cli_contract::spec::LANE
            }
        )
    );
    assert_eq!(
        format!(
            "{:?}",
            Arg::value("lane", "<stable|canary>", "Native Server authority lane.")
                .default("stable")
                .choices(&["stable", "canary"])
        ),
        format!(
            "{:?}",
            ds_cli_contract::spec::Arg {
                summary: "Native Server authority lane.",
                ..ds_cli_contract::spec::LANE
            }
        )
    );
    assert_eq!(
        format!(
            "{:?}",
            Arg::value(
                "lane",
                "<stable|canary>",
                "Native lane; defaults to stable.",
            )
        ),
        format!(
            "{:?}",
            ds_cli_contract::spec::Arg {
                summary: "Native lane; defaults to stable.",
                default: None,
                choices: &[],
                ..ds_cli_contract::spec::LANE
            }
        )
    );
    assert_eq!(
        format!(
            "{:?}",
            Arg::value(
                "lane",
                "<stable|canary>",
                "Native authentication lane and account/device context.",
            )
            .default("stable")
            .choices(&["stable", "canary"])
        ),
        format!(
            "{:?}",
            ds_cli_contract::spec::Arg {
                summary: "Native authentication lane and account/device context.",
                ..ds_cli_contract::spec::LANE
            }
        )
    );
    assert_eq!(
        format!(
            "{:?}",
            Arg::value(
                "lane",
                "<stable|canary>",
                "Native credential lane; default stable.",
            )
            .choices(&["stable", "canary"])
        ),
        format!(
            "{:?}",
            ds_cli_contract::spec::Arg {
                summary: "Native credential lane; default stable.",
                default: None,
                ..ds_cli_contract::spec::LANE
            }
        )
    );
}
