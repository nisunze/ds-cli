//! One calculation-settings contract for authored JSON and native calculations.
use crate::native::{CalculateCommand, calculation_concurrency};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RunSettings {
    #[serde(default)]
    pub render_charts: bool,
    pub generate_documents: Option<bool>,
    #[serde(default)]
    pub report_drafts: Vec<String>,
    /// Omitted/null or 0 selects Rust automatic sizing; explicit limits are 1..=32.
    #[schemars(range(min = 0, max = 32))]
    pub concurrency: Option<usize>,
}
impl From<&CalculateCommand> for RunSettings {
    fn from(c: &CalculateCommand) -> Self {
        Self {
            render_charts: c.render_charts,
            generate_documents: c.generate_documents,
            report_drafts: c.report_drafts.clone(),
            concurrency: c.concurrency,
        }
    }
}
impl RunSettings {
    pub fn validate(mut self) -> Result<Self, String> {
        calculation_concurrency(self.concurrency)?;
        if self.concurrency == Some(0) {
            self.concurrency = None;
        }
        self.generate_documents = Some(self.generate_documents.unwrap_or(true));
        if self.report_drafts.is_empty() {
            self.report_drafts.push("apd".into());
        }
        let mut drafts = Vec::new();
        for draft in self.report_drafts {
            if !matches!(draft.as_str(), "apd" | "network" | "plant" | "financial") {
                return Err(format!("unsupported Solar draft report intent: {draft}"));
            }
            if !drafts.contains(&draft) {
                drafts.push(draft);
            }
        }
        self.report_drafts = drafts;
        Ok(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn authored_settings_and_calculation_use_the_same_defaults_and_refusals() {
        let settings: RunSettings = serde_json::from_str(r#"{"concurrency":0}"#).unwrap();
        let settings = settings.validate().unwrap();
        assert_eq!(settings.concurrency, None);
        assert_eq!(settings.report_drafts, ["apd"]);
        assert_eq!(settings.generate_documents, Some(true));
        assert!(
            RunSettings {
                concurrency: Some(33),
                ..Default::default()
            }
            .validate()
            .is_err()
        );
        assert!(
            RunSettings {
                report_drafts: vec!["wrong".into()],
                ..Default::default()
            }
            .validate()
            .is_err()
        );
        assert!(serde_json::from_str::<RunSettings>(r#"{"force":true}"#).is_err());
    }
}
