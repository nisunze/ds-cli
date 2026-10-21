use crate::{ClientError, ErrorKind};

/// Project identity admission shared by native clients and CLI adapters.
/// This is a character bound; project authorization and safe local paths are separate.
pub const MAX_PROJECT_ID_CHARS: usize = 200;

pub fn validate_project_id(project: &str) -> Result<(), ClientError> {
    if project.is_empty()
        || project.trim() != project
        || project.chars().count() > MAX_PROJECT_ID_CHARS
        || project.chars().any(char::is_control)
    {
        return Err(ClientError::new(
            ErrorKind::InvalidInput,
            "the selected project id is empty, untrimmed, or outside its 200-character bound",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn project_identity_uses_one_character_bound() {
        for n in [160, 161, 200] {
            assert!(validate_project_id(&"é".repeat(n)).is_ok());
        }
        for id in ["", " p", "p ", "p\nq", &"é".repeat(201)] {
            assert!(validate_project_id(id).is_err());
        }
    }
}
