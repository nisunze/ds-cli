//! Fixed bounded stdin/stdout layout admission. No rendering or data IO.
use ds_command_kernel::printing::{self, layout_admission, Layout};
use serde_json::json;
use std::io::{Read, Write};

const MAX_REPLY_BYTES: usize = printing::MAX_LAYOUT_BYTES + 4096;

fn admitted(input: &[u8]) -> Result<Layout, String> {
    let layout: Layout = serde_json::from_slice(input).map_err(|e| e.to_string())?;
    let layout = printing::upgrade(layout)?;
    layout_admission::validate_heading_encoding(&layout)?;
    layout_admission::validate_fit_around_references(&layout)?;
    printing::validate(&layout)?;
    Ok(layout)
}

fn run(input: impl Read, mut output: impl Write, has_args: bool) -> i32 {
    let result = (|| {
        if has_args {
            return Err("validator accepts only one layout on stdin and no arguments".into());
        }
        let mut bytes = Vec::new();
        input
            .take(printing::MAX_LAYOUT_BYTES as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| "layout stdin could not be read".to_string())?;
        if bytes.len() > printing::MAX_LAYOUT_BYTES {
            return Err("print layout exceeds 750000 input bytes".into());
        }
        admitted(&bytes)
    })();
    let (answer, status) = match result {
        Ok(layout) => (json!({"valid":true,"layout":layout}), 0),
        Err(message) => (
            json!({"valid":false,"error":{"code":"print_layout_invalid","message":message.chars().take(2048).collect::<String>()}}),
            2,
        ),
    };
    let bytes = match serde_json::to_vec(&answer) {
        Ok(bytes) if bytes.len() <= MAX_REPLY_BYTES => bytes,
        _ => return 1,
    };
    if output
        .write_all(&bytes)
        .and_then(|_| output.write_all(b"\n"))
        .is_err()
    {
        return 1;
    }
    status
}
fn main() {
    std::process::exit(run(
        std::io::stdin().lock(),
        std::io::stdout().lock(),
        std::env::args_os().len() != 1,
    ));
}

#[cfg(test)]
mod tests {
    use super::*;
    fn invoke(bytes: &[u8]) -> (i32, serde_json::Value) {
        let mut output = Vec::new();
        let status = run(bytes, &mut output, false);
        assert!(output.len() <= MAX_REPLY_BYTES + 1);
        (status, serde_json::from_slice(&output).unwrap())
    }
    #[test]
    fn validates_and_echoes_one_canonical_layout_without_reinterpreting_fields() {
        let layout = printing::default_layout();
        let (status, answer) = invoke(&serde_json::to_vec(&layout).unwrap());
        assert_eq!(status, 0);
        assert_eq!(answer, json!({"valid":true,"layout":layout}));
    }
    #[test]
    fn rejects_unknown_fields_invalid_dimensions_and_trailing_json() {
        let mut layout = serde_json::to_value(printing::default_layout()).unwrap();
        layout["unknown"] = json!(true);
        assert_ne!(invoke(&serde_json::to_vec(&layout).unwrap()).0, 0);
        layout.as_object_mut().unwrap().remove("unknown");
        layout["page_mm"] = json!([1, 1]);
        assert_ne!(invoke(&serde_json::to_vec(&layout).unwrap()).0, 0);
        assert_ne!(invoke(b"{} {}").0, 0);
    }
    #[test]
    fn shared_checks_refuse_authored_heading_corruption_and_missing_fit_references() {
        let mut layout = printing::default_layout();
        layout.elements.push(serde_json::from_value(json!({
            "id":"schedule", "kind":"table", "rect":[12,32,150,90],
            "table":{"layer":"poles","columns":["angle"],"headings":["Angle \u{00c2}\u{00b0}"],"widths":[1]}
        })).unwrap());
        let (_, answer) = invoke(&serde_json::to_vec(&layout).unwrap());
        assert!(answer["error"]["message"]
            .as_str()
            .unwrap()
            .contains("mojibake"));
        layout
            .elements
            .last_mut()
            .unwrap()
            .table
            .as_mut()
            .unwrap()
            .headings[0] = "Angle (deg)".into();
        assert_eq!(invoke(&serde_json::to_vec(&layout).unwrap()).0, 0);
        layout
            .elements
            .iter_mut()
            .find_map(|element| element.viewport.as_mut())
            .unwrap()
            .fit_around
            .push("absent".into());
        let (_, answer) = invoke(&serde_json::to_vec(&layout).unwrap());
        assert!(answer["error"]["message"]
            .as_str()
            .unwrap()
            .contains("missing element `absent`"));
    }
    #[test]
    fn refuses_oversized_input_and_arguments_with_bounded_structured_errors() {
        assert_eq!(
            invoke(&vec![b' '; printing::MAX_LAYOUT_BYTES + 1]).1["valid"],
            false
        );
        let mut output = Vec::new();
        assert_ne!(run(&b"{}"[..], &mut output, true), 0);
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&output).unwrap()["error"]["code"],
            "print_layout_invalid"
        );
    }
}
