//! Where bytes go, and in what shape.
//!
//! One rule, enforced in one place: the answer goes to stdout, everything
//! else goes to stderr. A caller can pipe stdout into a JSON parser without
//! filtering, and a progress line can never corrupt a result.

use std::io::{self, IsTerminal, Write};

use serde::Serialize;

use crate::outcome::{ExitClass, Failure, error_envelope, success_envelope};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    /// Presentation. Layout may change between releases; do not parse it.
    Human,
    /// The automation contract. Compact by default — indentation is bytes a
    /// machine does not read.
    Json,
}

#[derive(Debug, Clone, Copy)]
pub struct Output {
    pub format: Format,
    pub pretty: bool,
    pub color: bool,
}

impl Output {
    /// Colour is off unless stdout is a terminal, `NO_COLOR` is unset, and
    /// the caller did not say otherwise. Agent shells and CI get clean bytes
    /// without having to ask.
    pub fn resolve(format: Format, pretty: bool, no_color: bool) -> Self {
        let color = !no_color
            && std::env::var_os("NO_COLOR").is_none()
            && format == Format::Human
            && io::stdout().is_terminal();
        Self {
            format,
            pretty,
            color,
        }
    }

    pub const fn is_json(self) -> bool {
        matches!(self.format, Format::Json)
    }

    /// Write a success. `human` is only called in human mode, so a renderer
    /// never costs anything in an automated run.
    pub fn success(
        self,
        command: &str,
        contract: u32,
        data: serde_json::Value,
        human: impl FnOnce(&serde_json::Value) -> String,
    ) -> io::Result<()> {
        let stdout = io::stdout();
        let mut handle = stdout.lock();
        match self.format {
            Format::Json => {
                self.write_json(&mut handle, &success_envelope(command, contract, data))?;
            }
            Format::Human => {
                let text = human(&data);
                handle.write_all(text.as_bytes())?;
                if !text.ends_with('\n') {
                    handle.write_all(b"\n")?;
                }
            }
        }
        handle.flush()
    }

    /// Write a failure. In JSON mode the envelope goes to stdout, because a
    /// refusal is a result an agent must parse; in human mode it goes to
    /// stderr, because a person piping stdout wants only the answer.
    pub fn failure(self, command: &str, contract: u32, failure: &Failure) -> io::Result<()> {
        match self.format {
            Format::Json => {
                let stdout = io::stdout();
                let mut handle = stdout.lock();
                self.write_json(&mut handle, &error_envelope(command, contract, failure))?;
                handle.flush()
            }
            Format::Human => {
                let stderr = io::stderr();
                let mut handle = stderr.lock();
                writeln!(handle, "{}: {}", failure.class().token(), failure.message())?;
                if let Some(remedy) = failure.remedy_text() {
                    writeln!(handle, "  → {remedy}")?;
                }
                // `detail` is often where the actionable part lives — the
                // accepted values of a rejected choice, the engine's own
                // reason, the digests to pin. Omitting it in human mode
                // produced refusals whose remedy said "read detail" while
                // nothing on screen showed one.
                if let Some(detail) = failure.detail_value() {
                    for line in flatten_detail(detail) {
                        writeln!(handle, "  {line}")?;
                    }
                }
                for next in failure.next_commands() {
                    writeln!(handle, "  next: {next}")?;
                }
                handle.flush()
            }
        }
    }

    /// Plain text to stdout, for help. Help is presentation even in JSON mode
    /// unless the caller explicitly asked for the machine descriptor.
    pub fn text(self, text: &str) -> io::Result<()> {
        let stdout = io::stdout();
        let mut handle = stdout.lock();
        handle.write_all(text.as_bytes())?;
        if !text.ends_with('\n') {
            handle.write_all(b"\n")?;
        }
        handle.flush()
    }

    fn write_json(self, writer: &mut impl Write, value: &impl Serialize) -> io::Result<()> {
        if self.pretty {
            serde_json::to_writer_pretty(&mut *writer, value)?;
        } else {
            serde_json::to_writer(&mut *writer, value)?;
        }
        writer.write_all(b"\n")
    }
}

/// A diagnostic. Never stdout, never in the way of a result.
pub fn note(message: &str) {
    let _ = writeln!(io::stderr(), "{message}");
}

/// The process exit code for an outcome.
pub const fn exit_code(class: ExitClass) -> u8 {
    class.code()
}

/// Render a bounded `detail` object as flat `key: value` lines.
///
/// Deliberately shallow and capped. `detail` is structured context for a
/// machine; a person needs the gist, and a refusal that filled the terminal
/// would be worse than one that said nothing.
fn flatten_detail(detail: &serde_json::Value) -> Vec<String> {
    const MAX_LINES: usize = 8;
    const MAX_VALUE: usize = 220;

    fn scalar(value: &serde_json::Value) -> String {
        match value {
            serde_json::Value::String(text) => text.clone(),
            other => other.to_string(),
        }
    }

    let mut lines = Vec::new();
    match detail {
        serde_json::Value::Object(fields) => {
            for (key, value) in fields {
                if lines.len() >= MAX_LINES {
                    lines.push("…".to_string());
                    break;
                }
                let rendered = match value {
                    serde_json::Value::Array(items) => items
                        .iter()
                        .take(12)
                        .map(scalar)
                        .collect::<Vec<_>>()
                        .join(", "),
                    serde_json::Value::Object(_) => {
                        // One level in, then stop: deeper nesting belongs to
                        // the JSON envelope, not to a terminal.
                        flatten_detail(value).join("; ")
                    }
                    other => scalar(other),
                };
                let rendered: String = rendered.chars().take(MAX_VALUE).collect();
                if !rendered.is_empty() {
                    lines.push(format!("{key}: {rendered}"));
                }
            }
        }
        other => lines.push(scalar(other)),
    }
    lines
}
