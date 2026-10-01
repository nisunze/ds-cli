//! Finite member form grants on the existing project-access authority.
//! A captured plan is never rebased and never promotes a project role.
use crate::authorized::Authority;
use crate::{AuthorizedTransport, ClientError, ClientProfile, ErrorKind, ServiceRefusal};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub const MAX_FORMS: usize = 100;
pub const MAX_BYTES: usize = 128 * 1024;
const MAX_VERSION: u64 = (1 << 53) - 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EffectiveForm {
    pub form: String,
    pub read_allowed: bool,
    pub only_own: bool,
    pub edit_allowed: bool,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Plan {
    pub schema: u32,
    pub operation: String,
    pub project_id: String,
    pub target_user_email: String,
    pub actor: String,
    pub base_version: u64,
    pub roles: Vec<String>,
    pub before_forms: Vec<String>,
    pub after_forms: Vec<String>,
    pub bound_forms: Vec<String>,
    pub snapshot: String,
    pub effective_before: Vec<EffectiveForm>,
    pub effective_after: Vec<EffectiveForm>,
    pub hash: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GrantResult {
    pub plan: Plan,
    pub applied: bool,
    pub current_version: u64,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Request {
    Read {
        member: String,
    },
    Plan {
        member: String,
        forms: Vec<String>,
        all: bool,
    },
    Apply {
        member: String,
        plan: Plan,
    },
}
fn invalid() -> ClientError {
    ClientError::new(
        ErrorKind::InvalidInput,
        "member form grant requires an exact existing member and finite captured plan",
    )
}
fn unreadable() -> ClientError {
    ClientError::new(
        ErrorKind::UnreadableResponse,
        "member form grant response violates its bounded identity contract",
    )
}
fn segment(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 128
        && s.trim() == s
        && s != "."
        && s != ".."
        && !s
            .chars()
            .any(|c| c.is_whitespace() || c.is_control() || c == '/' || c == '\\')
}
fn email(s: &str) -> bool {
    s.len() <= 254
        && s == s.to_ascii_lowercase()
        && s.trim() == s
        && s.split('@').count() == 2
        && !s.starts_with('@')
        && !s.ends_with('@')
        && !s
            .chars()
            .any(|c| c.is_whitespace() || c.is_control() || c == '/' || c == '\\')
}
fn form_id(s: &str) -> bool {
    // Existing survey form authority: 2..=200 lowercase slug bytes.
    (2..=200).contains(&s.len())
        && s.as_bytes().first().is_some_and(u8::is_ascii_alphanumeric)
        && s.as_bytes().last().is_some_and(u8::is_ascii_alphanumeric)
        && s.bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_' || c == b'-')
}
fn forms(values: &[String]) -> bool {
    values.len() <= MAX_FORMS
        && values.iter().all(|s| form_id(s))
        && values
            .iter()
            .collect::<std::collections::BTreeSet<_>>()
            .len()
            == values.len()
}
fn digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}
impl Plan {
    pub fn validate(&self) -> Result<(), ClientError> {
        let rows = |items: &[EffectiveForm]| {
            items.len() == self.bound_forms.len()
                && items.len() <= MAX_FORMS
                && items.iter().all(|r| {
                    form_id(&r.form)
                        && self.bound_forms.contains(&r.form)
                        && (!r.only_own || r.read_allowed)
                        && (!r.edit_allowed || r.read_allowed)
                })
                && items
                    .iter()
                    .map(|r| &r.form)
                    .collect::<std::collections::BTreeSet<_>>()
                    .len()
                    == items.len()
        };
        if self.schema != 1
            || !matches!(self.operation.as_str(), "read" | "set")
            || !segment(&self.project_id)
            || !email(&self.target_user_email)
            || !email(&self.actor)
            || self.base_version > MAX_VERSION
            || self.roles.len() > 32
            || !self.roles.iter().all(|s| segment(s))
            || !forms(&self.before_forms)
            || !forms(&self.after_forms)
            || !forms(&self.bound_forms)
            || !digest(&self.snapshot)
            || !digest(&self.hash)
            || !rows(&self.effective_before)
            || !rows(&self.effective_after)
            || (self.operation == "set"
                && (self.after_forms.is_empty()
                    || !self
                        .after_forms
                        .iter()
                        .all(|f| self.bound_forms.contains(f))))
        {
            return Err(invalid());
        }
        Ok(())
    }
}
impl Request {
    pub fn member(&self) -> &str {
        match self {
            Self::Read { member } | Self::Plan { member, .. } | Self::Apply { member, .. } => {
                member
            }
        }
    }
    pub fn validate(&self, project: &str) -> Result<(), ClientError> {
        if !segment(project) || !email(self.member()) {
            return Err(invalid());
        }
        match self {
            Self::Read { .. } => {}
            Self::Plan {
                forms: selected,
                all,
                ..
            } => {
                if !forms(selected)
                    || (*all && !selected.is_empty())
                    || (!all && selected.is_empty())
                {
                    return Err(invalid());
                }
            }
            Self::Apply { plan, .. } => {
                plan.validate()?;
                if plan.operation != "set"
                    || plan.project_id != project
                    || plan.target_user_email != self.member()
                {
                    return Err(invalid());
                }
            }
        }
        Ok(())
    }
    pub fn payload(&self, project: &str) -> Result<Value, ClientError> {
        self.validate(project)?;
        let mut data = json!({"project_id":project,"target_user_email":self.member()});
        let action = match self {
            Self::Read { .. } => "member_form_grant_read",
            Self::Plan { forms, all, .. } => {
                if *all {
                    data["all"] = json!(true);
                } else {
                    data["forms"] = json!(forms);
                }
                "member_form_grant_plan"
            }
            Self::Apply { plan, .. } => {
                data["plan"] = json!(plan);
                "member_form_grant_apply"
            }
        };
        let body = json!({"action":action,"data":data});
        if serde_json::to_vec(&body).map_err(|_| invalid())?.len() > MAX_BYTES {
            return Err(invalid());
        }
        Ok(body)
    }
}
fn decode(
    body: &[u8],
    project: &str,
    actor: &str,
    request: &Request,
) -> Result<GrantResult, ClientError> {
    if body.len() > MAX_BYTES {
        return Err(unreadable());
    }
    let envelope: Value = serde_json::from_slice(body).map_err(|_| unreadable())?;
    if envelope["success"] != true {
        return Err(unreadable());
    }
    let result: GrantResult =
        serde_json::from_value(envelope["data"].clone()).map_err(|_| unreadable())?;
    result.plan.validate().map_err(|_| unreadable())?;
    if result.plan.project_id != project
        || result.plan.actor != actor
        || result.plan.target_user_email != request.member()
        || result.current_version > MAX_VERSION
        || result.current_version < result.plan.base_version
    {
        return Err(unreadable());
    }
    match request {
        Request::Read { .. } => {
            if result.plan.operation != "read"
                || result.applied
                || result.current_version != result.plan.base_version
                || result.plan.before_forms != result.plan.after_forms
                || result.plan.effective_before != result.plan.effective_after
            {
                return Err(unreadable());
            }
        }
        Request::Plan { forms, all, .. } => {
            let mut expected = forms.clone();
            if *all {
                expected = result.plan.bound_forms.clone();
            }
            expected.sort();
            if result.plan.operation != "set"
                || result.applied
                || result.current_version != result.plan.base_version
                || result.plan.after_forms != expected
            {
                return Err(unreadable());
            }
        }
        Request::Apply { plan, .. } => {
            if &result.plan != plan
                || (result.applied
                    && (result.current_version <= plan.base_version
                        || plan.before_forms == plan.after_forms))
                || (!result.applied
                    && (result.current_version != plan.base_version
                        || plan.before_forms != plan.after_forms))
            {
                return Err(unreadable());
            }
        }
    }
    Ok(result)
}
pub(crate) fn run<T: AuthorizedTransport>(
    profile: &ClientProfile,
    transport: &mut T,
    authority: Authority<'_>,
    project: &str,
    request: &Request,
) -> Result<GrantResult, ClientError> {
    let payload = request.payload(project)?;
    if let Request::Apply { plan, .. } = request {
        if plan.actor != authority.email {
            return Err(invalid());
        }
    }
    let response = transport.member_form_grant(crate::MemberFormGrantCall {
        profile,
        payload,
        authority: authority.call(),
    })?;
    if response.body.len() > MAX_BYTES {
        return Err(unreadable());
    }
    if response.status != 200 {
        let body: Value = serde_json::from_slice(&response.body).unwrap_or(Value::Null);
        let code = body["error"]["code"].as_str().map(str::to_ascii_lowercase);
        let refusal = ServiceRefusal::new(
            response.status,
            code.as_deref(),
            body["error"]["message"].as_str(),
        );
        let kind = match response.status {
            401 | 403 => ErrorKind::AuthenticationRejected,
            404 => ErrorKind::ResourceNotFound,
            400 | 409 | 422 => ErrorKind::InvalidInput,
            _ => ErrorKind::Transient,
        };
        return Err(ClientError::with_service_refusal(
            kind,
            "member form grant refused",
            refusal,
        ));
    }
    decode(&response.body, project, authority.email, request)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn plan() -> Plan {
        Plan {
            schema: 1,
            operation: "set".into(),
            project_id: "p".into(),
            target_user_email: "member@example.com".into(),
            actor: "manager@example.com".into(),
            base_version: 0,
            roles: vec![],
            before_forms: vec!["old".into()],
            after_forms: vec!["pole".into()],
            bound_forms: vec!["pole".into()],
            snapshot: "a".repeat(64),
            effective_before: vec![EffectiveForm {
                form: "pole".into(),
                read_allowed: false,
                only_own: false,
                edit_allowed: false,
            }],
            effective_after: vec![EffectiveForm {
                form: "pole".into(),
                read_allowed: true,
                only_own: false,
                edit_allowed: true,
            }],
            hash: "b".repeat(64),
        }
    }
    #[test]
    fn member_form_grant_payload_is_fixed_and_apply_is_pinned() {
        let p = plan();
        let request = Request::Apply {
            member: p.target_user_email.clone(),
            plan: p.clone(),
        };
        assert!(decode(
            &vec![b' '; MAX_BYTES + 1],
            "p",
            "manager@example.com",
            &request
        )
        .is_err());
        assert!(Request::Plan {
            member: "member@example.com".into(),
            forms: (0..=MAX_FORMS).map(|i| format!("form_{i}")).collect(),
            all: false
        }
        .payload("p")
        .is_err());
        let payload = request.payload("p").unwrap();
        assert_eq!(payload["action"], "member_form_grant_apply");
        assert_eq!(payload["data"]["plan"]["base_version"], 0);
        assert!(payload["data"].get("roles").is_none());
        assert!(request.payload("other").is_err());
        for invalid in ["*", "Pole", "../pole", "p"] {
            assert!(Request::Plan {
                member: "member@example.com".into(),
                forms: vec![invalid.into()],
                all: false
            }
            .payload("p")
            .is_err());
        }
        assert!(Request::Plan {
            member: "member@example.com".into(),
            forms: vec![],
            all: false
        }
        .payload("p")
        .is_err());
    }
    #[test]
    fn member_form_grant_decoder_fences_accounts_unknown_fields_and_receipts() {
        let p = plan();
        let request = Request::Apply {
            member: p.target_user_email.clone(),
            plan: p.clone(),
        };
        let result = GrantResult {
            plan: p,
            applied: true,
            current_version: 1,
        };
        let mut wire = json!({"success":true,"data":result});
        let bytes = serde_json::to_vec(&wire).unwrap();
        assert!(decode(&bytes, "p", "manager@example.com", &request).is_ok());
        assert!(decode(&bytes, "p", "other@example.com", &request).is_err());
        assert!(decode(&bytes, "other", "manager@example.com", &request).is_err());
        wire["data"]["plan"]["roles"] = json!(["project_admin"]);
        assert!(decode(
            &serde_json::to_vec(&wire).unwrap(),
            "p",
            "manager@example.com",
            &request
        )
        .is_err());
        wire["data"]["future"] = json!(true);
        assert!(decode(
            &serde_json::to_vec(&wire).unwrap(),
            "p",
            "manager@example.com",
            &request
        )
        .is_err());
    }
    #[test]
    fn member_form_grant_noop_requires_unchanged_intent_and_revision() {
        let mut p = plan();
        p.before_forms = p.after_forms.clone();
        p.effective_before = p.effective_after.clone();
        let request = Request::Apply {
            member: p.target_user_email.clone(),
            plan: p.clone(),
        };
        let mut wire =
            json!({"success":true,"data":GrantResult{plan:p, applied:false, current_version:0}});
        assert!(decode(
            &serde_json::to_vec(&wire).unwrap(),
            "p",
            "manager@example.com",
            &request
        )
        .is_ok());
        wire["data"]["current_version"] = json!(1);
        assert!(decode(
            &serde_json::to_vec(&wire).unwrap(),
            "p",
            "manager@example.com",
            &request
        )
        .is_err());
        wire["data"]["applied"] = json!(true);
        assert!(decode(
            &serde_json::to_vec(&wire).unwrap(),
            "p",
            "manager@example.com",
            &request
        )
        .is_err());
        let changed = plan();
        let request = Request::Apply {
            member: changed.target_user_email.clone(),
            plan: changed.clone(),
        };
        let wire = json!({"success":true,"data":GrantResult{plan:changed, applied:false, current_version:0}});
        assert!(decode(
            &serde_json::to_vec(&wire).unwrap(),
            "p",
            "manager@example.com",
            &request
        )
        .is_err());
    }
    #[test]
    fn member_form_grant_read_refuses_changed_scope_or_effective_permissions() {
        let mut p = plan();
        p.operation = "read".into();
        p.before_forms = p.after_forms.clone();
        p.effective_before = p.effective_after.clone();
        let request = Request::Read {
            member: p.target_user_email.clone(),
        };
        let valid =
            json!({"success":true,"data":GrantResult{plan:p,applied:false,current_version:0}});
        assert!(decode(
            &serde_json::to_vec(&valid).unwrap(),
            "p",
            "manager@example.com",
            &request
        )
        .is_ok());
        let mut changed_grant = valid.clone();
        changed_grant["data"]["plan"]["before_forms"] = json!(["old"]);
        assert!(decode(
            &serde_json::to_vec(&changed_grant).unwrap(),
            "p",
            "manager@example.com",
            &request
        )
        .is_err());
        let mut changed_permission = valid;
        changed_permission["data"]["plan"]["effective_before"][0]["edit_allowed"] = json!(false);
        assert!(decode(
            &serde_json::to_vec(&changed_permission).unwrap(),
            "p",
            "manager@example.com",
            &request
        )
        .is_err());
    }
    #[test]
    fn member_form_grant_all_is_finite_not_a_wildcard() {
        let p = plan();
        let request = Request::Plan {
            member: p.target_user_email.clone(),
            forms: vec![],
            all: true,
        };
        let mut wire =
            json!({"success":true,"data":GrantResult{plan:p,applied:false,current_version:0}});
        assert!(decode(
            &serde_json::to_vec(&wire).unwrap(),
            "p",
            "manager@example.com",
            &request
        )
        .is_ok());
        wire["data"]["plan"]["after_forms"] = json!(["future"]);
        assert!(decode(
            &serde_json::to_vec(&wire).unwrap(),
            "p",
            "manager@example.com",
            &request
        )
        .is_err());
    }
}
