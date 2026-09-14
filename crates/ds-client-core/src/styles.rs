//! Guided style publication over the same captured native project and
//! authority. The publish body and every rule a receipt is admitted under are
//! the kernel's (`ds_command_kernel::style_document`), shared with the
//! browser; this module only carries them into the native client's errors.
use crate::{ClientError, ErrorKind, TransportResponse};
use ds_command_kernel::style_document as wire;
pub use ds_command_kernel::style_plan::StyleInstruction;
use serde_json::Value;

fn bad() -> ClientError {
    ClientError::new(
        ErrorKind::UnreadableResponse,
        "invalid governed style response",
    )
}
/// The kernel's refusal, carried as the route's bounded refusal so callers
/// read the same words `ds` and the browser show.
fn refused(refusal: wire::Refusal) -> ClientError {
    ClientError::with_service_refusal(
        ErrorKind::UnreadableResponse,
        "invalid governed style response",
        crate::ServiceRefusal::new(0, Some(refusal.code), Some(&refusal.message)),
    )
}

/// Whether one publish receipt is the style that was authored: the kernel's
/// answer, over the whole parsed response body.
pub(crate) fn admit_publish(
    expected: &Value,
    response: TransportResponse,
) -> Result<Value, ClientError> {
    if response.body.is_empty() || response.body.len() > crate::LAYERS_RESPONSE_LIMIT {
        return Err(bad());
    }
    let answered: Value = serde_json::from_slice(&response.body).map_err(|_| bad())?;
    wire::admit_publish(expected, &answered).map_err(refused)
}

#[derive(Clone)]
pub struct StyleEditReceipt {
    pub(crate) data: Value,
}
impl StyleEditReceipt {
    pub fn data(&self) -> &Value {
        &self.data
    }
}
