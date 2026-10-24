//! Help and tutorial articles: the language-resolved article library Brain
//! serves for this client's audience. Admission resolves every
//! `help-asset:<id>` reference in an article to its asset's URL or data URI,
//! so a page renders the content as given and carries no asset table.

use ds_gateway_client::Operation;
use std::collections::BTreeMap;
use std::sync::OnceLock;

use percent_encoding::{utf8_percent_encode, AsciiSet, NON_ALPHANUMERIC};
use regex::{Captures, Regex};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::{data, Admitted, Exchange, Plan, Refusal, Request};

/// The application whose help library this client reads.
pub const APP: &str = "ds_griddesign";
/// The audience this client is: the GridDesign web client, on every host.
pub const REQUESTER: &str = "ds-web";
pub const PLATFORM: &str = "web";
/// Articles may carry their images inline as base64.
const RESPONSE_CAP: u64 = 32 * 1024 * 1024;
const LANGUAGE_MAX_CHARS: usize = 16;
pub const INVALID: &str = "help_answer_invalid";
pub const LANGUAGE_INVALID: &str = "help_language_invalid";

/// What JavaScript's `encodeURIComponent` leaves as is.
const URI_COMPONENT: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'_')
    .remove(b'.')
    .remove(b'!')
    .remove(b'~')
    .remove(b'*')
    .remove(b'\'')
    .remove(b'(')
    .remove(b')');

/// How an article's content is written.
#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
#[serde(rename_all = "lowercase")]
pub enum ArticleFormat {
    Markdown,
    Html,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
pub struct HelpArticle {
    pub slug: String,
    pub title: String,
    pub description: String,
    pub category: String,
    pub order: i64,
    /// The article body with every resolvable asset reference resolved.
    pub content: String,
    pub format: ArticleFormat,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
pub struct HelpCategory {
    pub key: String,
    pub label: String,
    pub order: i64,
    pub collapsible: bool,
    #[serde(rename = "defaultOpen")]
    pub default_open: bool,
}

/// The library a help or tutorial page renders.
#[derive(Clone, Debug, Serialize, PartialEq)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
pub struct HelpLibrary {
    pub articles: Vec<HelpArticle>,
    pub categories: Vec<HelpCategory>,
}

#[derive(Deserialize)]
struct Asset {
    #[serde(rename = "contentType", default)]
    content_type: String,
    encoding: String,
    #[serde(default)]
    data: Option<String>,
    #[serde(default)]
    url: Option<String>,
}

#[derive(Deserialize)]
struct Article {
    slug: String,
    title: String,
    description: String,
    category: String,
    order: i64,
    content: String,
    #[serde(default)]
    format: Option<String>,
    #[serde(rename = "assetData", default)]
    asset_data: BTreeMap<String, Asset>,
}

#[derive(Deserialize)]
struct Answer {
    articles: Vec<Article>,
    categories: Vec<HelpCategory>,
    #[serde(default)]
    assets: BTreeMap<String, Asset>,
}

/// The URL an asset reference resolves to: its own URL, or its bytes as a
/// data URI; `None` when the asset holds neither.
fn asset_url(asset: &Asset) -> Option<String> {
    if asset.encoding == "url" {
        return asset
            .url
            .as_deref()
            .map(str::trim)
            .filter(|url| !url.is_empty())
            .map(str::to_owned);
    }
    let data = asset.data.as_deref().filter(|data| !data.is_empty())?;
    let content_type = if asset.content_type.is_empty() {
        "application/octet-stream"
    } else {
        &asset.content_type
    };
    Some(if asset.encoding == "base64" {
        format!("data:{content_type};base64,{data}")
    } else {
        format!(
            "data:{content_type};charset=utf-8,{}",
            utf8_percent_encode(data, URI_COMPONENT)
        )
    })
}

fn reference() -> &'static Regex {
    static REFERENCE: OnceLock<Regex> = OnceLock::new();
    REFERENCE.get_or_init(|| Regex::new(r"help-asset:([A-Za-z0-9_.-]+)").expect("a fixed pattern"))
}

/// Replace each `help-asset:<id>` the assets resolve; leave the rest as is.
fn resolve(
    content: &str,
    shared: &BTreeMap<String, Asset>,
    own: &BTreeMap<String, Asset>,
) -> String {
    if !content.contains("help-asset:") || (shared.is_empty() && own.is_empty()) {
        return content.to_owned();
    }
    reference()
        .replace_all(content, |found: &Captures<'_>| {
            own.get(&found[1])
                .or_else(|| shared.get(&found[1]))
                .and_then(asset_url)
                .unwrap_or_else(|| found[0].to_owned())
        })
        .into_owned()
}

fn library(body: &Value) -> Result<HelpLibrary, Refusal> {
    let answer = Answer::deserialize(data(body))
        .map_err(|_| Refusal::new(INVALID, "The help service answered an invalid library."))?;
    let articles = answer
        .articles
        .into_iter()
        .map(|article| HelpArticle {
            content: resolve(&article.content, &answer.assets, &article.asset_data),
            format: if article.format.as_deref() == Some("html") {
                ArticleFormat::Html
            } else {
                ArticleFormat::Markdown
            },
            slug: article.slug,
            title: article.title,
            description: article.description,
            category: article.category,
            order: article.order,
        })
        .collect();
    Ok(HelpLibrary {
        articles,
        categories: answer.categories,
    })
}

fn language(language: &str) -> Result<&str, Refusal> {
    let valid = !language.is_empty()
        && language.chars().count() <= LANGUAGE_MAX_CHARS
        && language
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '-');
    if valid {
        Ok(language)
    } else {
        Err(Refusal::new(
            LANGUAGE_INVALID,
            "Help is read in one language tag.",
        ))
    }
}

/// The help library of this application, in one language.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HelpArticles {
    pub language: String,
}

impl Exchange for HelpArticles {
    type Output = HelpLibrary;

    fn plan(&self) -> Result<Plan, Refusal> {
        let body = json!({"action": "list", "app": APP, "language": language(&self.language)?,
            "requester": REQUESTER, "platform": PLATFORM, "includeAssets": true});
        Ok(Request::generated(Operation::HelpAction, Some(body))?
            .response_cap(RESPONSE_CAP)
            .into())
    }

    fn admit(self, _status: u16, body: Value) -> Result<Admitted<Self>, Refusal> {
        library(&body).map(Admitted::Done)
    }
}

/// The global tutorial library, in one language.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TutorialArticles {
    pub language: String,
}

impl Exchange for TutorialArticles {
    type Output = HelpLibrary;

    fn plan(&self) -> Result<Plan, Refusal> {
        let body = json!({"action": "list", "language": language(&self.language)?,
            "requester": REQUESTER, "platform": PLATFORM, "includeAssets": true});
        Ok(Request::generated(Operation::TutorialsAction, Some(body))?
            .response_cap(RESPONSE_CAP)
            .into())
    }

    fn admit(self, _status: u16, body: Value) -> Result<Admitted<Self>, Refusal> {
        library(&body).map(Admitted::Done)
    }
}

/// The typed browser exports Svelte calls; each runs the shared executor.
#[cfg(all(feature = "wasm", target_arch = "wasm32"))]
pub(crate) mod exports {
    use super::{HelpArticles, TutorialArticles};
    use crate::gateway_exchange::browser::{run, typed, GatewayHost};
    use wasm_bindgen::prelude::*;

    /// The help library of this application, in one language.
    #[wasm_bindgen(js_name = helpArticles, unchecked_return_type = "HelpLibrary")]
    pub async fn help_articles(host: GatewayHost, language: String) -> Result<JsValue, JsValue> {
        typed(&run(&host, HelpArticles { language }).await?)
    }

    /// The global tutorial library, in one language.
    #[wasm_bindgen(js_name = tutorialArticles, unchecked_return_type = "HelpLibrary")]
    pub async fn tutorial_articles(
        host: GatewayHost,
        language: String,
    ) -> Result<JsValue, JsValue> {
        typed(&run(&host, TutorialArticles { language }).await?)
    }
}

#[cfg(test)]
#[path = "help_tests.rs"]
mod tests;
