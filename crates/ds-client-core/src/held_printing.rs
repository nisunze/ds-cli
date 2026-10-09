//! Printing reads a host performs under a session credential it already holds:
//! the Desktop shell's webview-supplied bearer, attached to its pinned gateway
//! origin exactly as its sync door does (compare
//! [`crate::HeldSessionSnapshotRead`]).
//!
//! The core fixes each read's method, path, body and bounds and decodes the
//! answer with the same strict decoder a native session's read uses, so the
//! Desktop's print catalogue is the CLI's (`report.layout.list`,
//! `report.standard.list|get`). The host chooses no route or field; it only
//! moves bytes and returns the status and body it received.
use crate::{
    ClientError, ErrorKind, PrintingRequest, PrintingStandardRequest, SecretRequestBody,
    TransportResponse,
};
use serde_json::Value;

enum Read {
    Setups(PrintingRequest),
    Governed(PrintingStandardRequest),
}

/// One printing read of one explicit project, planned by the core.
pub struct HeldSessionPrintingRead {
    project: String,
    read: Read,
}

impl HeldSessionPrintingRead {
    /// The project's printing setups, as `report.layout.list` reads them.
    pub fn setups(project: &str) -> Result<Self, ClientError> {
        let request = PrintingRequest::List {};
        request.validate_for(project)?;
        Ok(Self {
            project: project.to_owned(),
            read: Read::Setups(request),
        })
    }

    /// One read of the project's governed documents, as
    /// `report.standard.list|get` reads them.
    pub fn governed(project: &str, request: PrintingStandardRequest) -> Result<Self, ClientError> {
        crate::authorized::validate_tiles(project)?;
        request.validate()?;
        Ok(Self {
            project: project.to_owned(),
            read: Read::Governed(request),
        })
    }

    pub const fn method(&self) -> &'static str {
        // Both printing routes take a POSTed body.
        crate::PRINTING_METHOD
    }

    pub fn path(&self) -> &'static str {
        match self.read {
            Read::Setups(_) => crate::PRINTING_PATH,
            Read::Governed(_) => crate::PRINTING_STANDARD_PATH,
        }
    }

    pub fn timeout_seconds(&self) -> u64 {
        match self.read {
            Read::Setups(_) => crate::TILE_TIMEOUT_SECONDS,
            Read::Governed(_) => crate::printing_standard::PRINTING_STANDARD_TIMEOUT_SECONDS,
        }
    }

    pub fn response_limit(&self) -> usize {
        match self.read {
            Read::Setups(_) => crate::PRINTING_RESPONSE_LIMIT,
            Read::Governed(_) => crate::PRINTING_STANDARD_RESPONSE_LIMIT,
        }
    }

    pub fn body(&self) -> SecretRequestBody {
        SecretRequestBody::new(match &self.read {
            Read::Setups(request) => request.body(&self.project),
            Read::Governed(request) => request.body(&self.project),
        })
    }

    /// The refusal for a read whose bytes the host could not move (no
    /// connection, a timeout, an unreadable or oversized body stream).
    pub fn unreachable(&self) -> ClientError {
        ClientError::new(
            ErrorKind::Transient,
            "the printing service could not be reached",
        )
    }

    /// The answer to this exact read, or the same typed refusal a native
    /// session receives for the same status and body.
    pub fn decode(&self, response: TransportResponse) -> Result<Value, ClientError> {
        match &self.read {
            Read::Setups(request) => crate::printing::decode(response, request),
            Read::Governed(request) => {
                crate::printing_standard::decode(response, &self.project, request)
            }
        }
    }
}

/// The refusal code `ds report standard` documents for a governed read that
/// failed, so a catalogue that lists without its governed documents names the
/// same cause in the Desktop as in a terminal.
pub fn governed_refusal_code(error: &ClientError) -> &'static str {
    if error
        .service_refusal()
        .and_then(|refusal| refusal.code())
        .is_some_and(|code| code == "print_standard_invalid")
    {
        return "print_standard_invalid";
    }
    match error.kind() {
        ErrorKind::RouteUnavailable => "print_standard_route_unavailable",
        ErrorKind::ResourceNotFound => "print_standard_not_found",
        ErrorKind::InvalidInput => "print_standard_request_invalid",
        ErrorKind::Transient => "print_standard_unreachable",
        ErrorKind::UnreadableResponse => "print_standard_unreadable",
        _ => "print_standard_refused",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ok(data: Value) -> TransportResponse {
        TransportResponse::new(
            200,
            serde_json::to_vec(&json!({"success": true, "data": data})).unwrap(),
        )
    }

    #[test]
    fn the_setups_read_is_the_cli_list_of_the_named_project() {
        let read = HeldSessionPrintingRead::setups("project_one").unwrap();
        assert_eq!(read.method(), "POST");
        assert_eq!(read.path(), "/api/v1/printing");
        let body: Value = serde_json::from_slice(read.body().as_bytes()).unwrap();
        assert_eq!(
            body,
            json!({"action":"list","scope":"project","project_id":"project_one"})
        );
        let listed = read.decode(ok(json!({"setups": []}))).unwrap();
        assert_eq!(listed["setups"], json!([]));
        assert!(HeldSessionPrintingRead::setups("").is_err());
    }

    #[test]
    fn the_governed_read_refuses_another_project_and_names_a_missing_route() {
        let read =
            HeldSessionPrintingRead::governed("project_one", PrintingStandardRequest::List {})
                .unwrap();
        assert_eq!(read.path(), "/api/v1/printing/standard");
        let body: Value = serde_json::from_slice(read.body().as_bytes()).unwrap();
        assert_eq!(
            body,
            json!({"action":"list","scope":"project","project_id":"project_one"})
        );
        let catalogue = json!({"scope":"project","project_id":"project_one",
            "schema":"ds.printing-standard-catalog/v1","documents":[]});
        assert_eq!(read.decode(ok(catalogue)).unwrap()["documents"], json!([]));
        let foreign = json!({"scope":"project","project_id":"project_two",
            "schema":"ds.printing-standard-catalog/v1","documents":[]});
        assert!(read.decode(ok(foreign)).is_err());
        let unrouted = read
            .decode(TransportResponse::new(404, b"Not Found".to_vec()))
            .unwrap_err();
        assert_eq!(
            governed_refusal_code(&unrouted),
            "print_standard_route_unavailable"
        );
        assert_eq!(
            governed_refusal_code(&read.unreachable()),
            "print_standard_unreachable"
        );
        assert!(HeldSessionPrintingRead::governed(
            "project_one",
            PrintingStandardRequest::Get {
                kind: crate::PrintingStandardKind::A4,
                id: "../x".into()
            }
        )
        .is_err());
    }
}
