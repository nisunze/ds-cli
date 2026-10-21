//! Solar output vocabulary selected by the one engine declaration table.
use crate::compute_artifact_inventory::Declaration;

pub const REPORT_INPUT_MAX_BYTES: u64 = (8 << 20) - (1 << 10);
const PORTFOLIO_RESULT_MAX_BYTES: u64 = 8 << 20;
const DRAFTS: &[&str] = &[
    "apd-draft-fr",
    "apd-draft-en",
    "network-draft-fr",
    "network-draft-en",
    "plant-draft-fr",
    "plant-draft-en",
    "financial-draft-fr",
    "financial-draft-en",
];
const FINALS: &[&str] = &[
    "apd-final-fr",
    "apd-final-en",
    "network-final-fr",
    "network-final-en",
    "plant-final-fr",
    "plant-final-en",
    "financial-final-fr",
    "financial-final-en",
];

pub fn derived(operation: &str, variant: &str, outputs: &[Declaration]) -> Result<(), String> {
    if operation.starts_with("portfolio-") {
        if !outputs
            .iter()
            .any(|output| output.output_id == "portfolio-result")
        {
            return Err("portfolio must declare its governed result".into());
        }
        for output in outputs {
            let allowed = if variant == "aggregate-v3" {
                ds_solar_contracts::portfolio_publication::output_metadata(
                    &output.output_id,
                    &output.format,
                    &output.content_type,
                )
            } else {
                (output.output_id == "portfolio-result" && output.format == "json")
                    || (output.output_id == "portfolio-draft-fr" && output.format == "md")
            };
            if !allowed
                || (output.output_id == "portfolio-result"
                    && output.size_bytes > PORTFOLIO_RESULT_MAX_BYTES)
            {
                return Err("portfolio output is outside the governed declaration boundary".into());
            }
        }
    } else if operation.starts_with("report-") {
        let final_variant = FINALS.contains(&variant);
        if final_variant {
            if outputs.len() != 1 || outputs[0].output_id != variant || outputs[0].format != "md" {
                return Err("reviewed final must declare only its matching Markdown".into());
            }
        } else {
            let mut drafts = 0;
            for output in outputs {
                if DRAFTS.contains(&output.output_id.as_str()) && output.format == "md" {
                    drafts += 1;
                    if variant != output.output_id {
                        return Err("draft variant differs from its Markdown".into());
                    }
                } else if !(output.output_id.starts_with("plot-") && output.format == "png") {
                    return Err("report output is outside the draft and chart vocabulary".into());
                }
            }
            if drafts != 1 {
                return Err("report must declare one language-specific draft".into());
            }
        }
    }
    Ok(())
}
