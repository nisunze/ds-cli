//! Read-only consumption of a held printing catalogue and document pages.
//! Hosts report their current view; selection and dirty-draft admission remain
//! native. A plan never acquires, refreshes, renders, saves or discards data.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::{catalogue::Location, session};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub project: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub location: Option<Location>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page: Option<usize>,
}

pub fn validate(request: &Request) -> Result<(), String> {
    if request.project.is_empty()
        || request.project.len() > 160
        || request.project.trim() != request.project
        || request.project.chars().any(char::is_control)
    {
        return Err("printing_request_invalid: one exact bounded project is required".into());
    }
    if request.location.is_some() && request.page.is_some() {
        return Err(
            "printing_request_invalid: select a template, then navigate its held pages".into(),
        );
    }
    if request.page == Some(0) {
        return Err("printing_request_invalid: page numbers start at one".into());
    }
    if let Some(location) = &request.location {
        if serde_json::to_vec(location)
            .map_err(|e| e.to_string())?
            .len()
            > 4096
        {
            return Err("printing_request_invalid: template location exceeds its bound".into());
        }
        match location {
            Location::GovernedHtml { project_id, .. }
            | Location::GovernedStandard { project_id, .. }
                if project_id != &request.project =>
            {
                return Err("printing_request_invalid: template belongs to another project".into());
            }
            _ => {}
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Facts {
    pub project: String,
    pub locations: Vec<Location>,
    pub shown: Option<Location>,
    pub session: session::State,
    pub page_count: usize,
    pub current_page: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct Refusal {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Plan {
    pub load: Option<Location>,
    pub page: Option<usize>,
    pub refusal: Option<Refusal>,
}

fn refuse(code: &str, message: &str) -> Plan {
    Plan {
        load: None,
        page: None,
        refusal: Some(Refusal {
            code: code.into(),
            message: message.into(),
        }),
    }
}

pub fn plan(request: &Request, facts: &Facts) -> Result<Plan, String> {
    validate(request)?;
    if request.project != facts.project || facts.locations.len() > 512 {
        return Err("printing_request_invalid: held printing view belongs to another project or exceeds its bound".into());
    }
    if let Some(location) = &request.location {
        if facts
            .locations
            .iter()
            .filter(|held| *held == location)
            .count()
            != 1
        {
            return Ok(refuse("printing_view_template_unavailable", "The exact template location is not in the held project catalogue. Open or explicitly refresh the project printing library."));
        }
        if facts.shown.as_ref() == Some(location) {
            return Ok(Plan {
                load: None,
                page: None,
                refusal: None,
            });
        }
        let event = match location {
            Location::Layout { id } => session::Event::Load {
                id: id.clone(),
                revision: String::new(),
                global: false,
                confirmed: false,
                same_document: false,
            },
            _ => session::Event::Discard { confirmed: false },
        };
        if let Some(refusal) = session::step(&facts.session, &event)?.refusal {
            return Ok(refuse(&refusal.code, "Save or explicitly discard the current print draft before selecting another template."));
        }
        return Ok(Plan {
            load: Some(location.clone()),
            page: None,
            refusal: None,
        });
    }
    if let Some(number) = request.page {
        if number > facts.page_count || facts.page_count == 0 {
            return Ok(refuse("printing_view_page_unavailable", "This page is not held in the current printing document. Open the document before navigating its pages."));
        }
    }
    Ok(Plan {
        load: None,
        page: request.page,
        refusal: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn facts() -> Facts {
        Facts {
            project: "p1".into(),
            locations: vec![
                Location::Layout { id: "a0".into() },
                Location::Layout { id: "a3".into() },
            ],
            shown: Some(Location::Layout { id: "a0".into() }),
            session: session::State {
                has_layout: true,
                ..Default::default()
            },
            page_count: 7,
            current_page: 1,
        }
    }
    fn request() -> Request {
        Request {
            project: "p1".into(),
            location: None,
            page: None,
        }
    }
    #[test]
    fn dirty_work_refuses_selection_but_keeps_same_template_and_all_held_pages_free() {
        let mut held = facts();
        held.session.dirty = true;
        let mut ask = request();
        ask.location = held.shown.clone();
        assert!(plan(&ask, &held).unwrap().load.is_none());
        ask.location = Some(held.locations[1].clone());
        assert_eq!(
            plan(&ask, &held).unwrap().refusal.unwrap().code,
            "printing_discard_unconfirmed"
        );
        ask.location = None;
        for page in 1..=held.page_count {
            ask.page = Some(page);
            assert_eq!(plan(&ask, &held).unwrap().page, Some(page));
        }
        assert!(held.session.dirty);
    }
    #[test]
    fn exact_pins_scope_and_actual_page_count_bound_the_read_only_plan() {
        let held = facts();
        let mut ask = request();
        ask.page = Some(8);
        assert_eq!(
            plan(&ask, &held).unwrap().refusal.unwrap().code,
            "printing_view_page_unavailable"
        );
        ask.page = Some(0);
        assert!(plan(&ask, &held).is_err());
        ask.page = None;
        ask.project = "p2".into();
        assert!(plan(&ask, &held).is_err());
        ask.project = "p1".into();
        ask.location = Some(Location::Layout {
            id: "not-listed".into(),
        });
        assert_eq!(
            plan(&ask, &held).unwrap().refusal.unwrap().code,
            "printing_view_template_unavailable"
        );
        ask.location = Some(Location::GovernedHtml {
            project_id: "p2".into(),
            id: "html".into(),
            revision_id: "r".into(),
            content_sha256: "a".repeat(64),
            html_sha256: "b".repeat(64),
        });
        assert!(plan(&ask, &held).is_err());
    }
    #[test]
    fn governed_selection_uses_the_exact_catalogue_revision_and_never_confirms_discard() {
        let mut held = facts();
        let location = Location::GovernedStandard {
            project_id: "p1".into(),
            id: "cover".into(),
            template_id: "cover".into(),
            revision_id: "r1".into(),
            content_sha256: Some("a".repeat(64)),
        };
        held.locations.push(location.clone());
        let mut ask = request();
        ask.location = Some(location.clone());
        assert_eq!(plan(&ask, &held).unwrap().load, Some(location));
        if let Some(Location::GovernedStandard { revision_id, .. }) = &mut ask.location {
            *revision_id = "r2".into();
        }
        assert!(plan(&ask, &held).unwrap().refusal.is_some());
        held.session.dirty = true;
        ask.location = Some(held.locations[2].clone());
        assert_eq!(
            plan(&ask, &held).unwrap().refusal.unwrap().code,
            "printing_discard_unconfirmed"
        );
    }

    #[test]
    fn the_declared_headless_edit_intent_and_native_plan_return_the_same_result() {
        let held = facts();
        let mut ask = request();
        ask.page = Some(7);
        let input = serde_json::json!({"op":"view_plan","request":ask,"facts":held});
        let response: serde_json::Value =
            serde_json::from_str(&super::super::evaluate(input.to_string().as_bytes()).unwrap()).unwrap();
        assert_eq!(
            response,
            serde_json::json!({"load":null,"page":7,"refusal":null})
        );
        let mut extra = input;
        extra["request"]["refresh"] = serde_json::json!(true);
        assert!(super::super::evaluate(extra.to_string().as_bytes()).is_err());
    }
}
