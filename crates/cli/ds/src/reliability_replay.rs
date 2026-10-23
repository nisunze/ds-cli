//! The replay of one invocation, projected from its command's descriptor
//! (ds-command-kernel `docs/contracts/ds-cli-reliability-and-feedback.md` §4).
//!
//! Every word after `ds` travels, in order, so a coding session can run the
//! exact invocation again. A word is withheld only where the descriptor says
//! so or nothing can: a value of an input the descriptor marks `sensitive`
//! (`<redacted>`), a value no declared input takes (`<unbound>`: a misspelt
//! flag's value could be anything), and, when the whole list would exceed the
//! backlog's bound, the longest values first (`<elided>`). Path words, flags,
//! choices, project ids, transformer names and paths stay: they are what makes
//! a report replayable. A panic adds where it happened, `file:line`, and never
//! its message.

use std::sync::{Once, OnceLock};

use ds_cli_auth::reliability::reporter::{
    self, ELIDED, REDACTED, REPLAY_MAX_CHARS, Replay, UNBOUND,
};
use ds_cli_contract::spec::{Arg, ArgKind, Command};

use crate::build;

/// The global switches `ds` strips before routing; `--version` is one only
/// where the command does not declare its own.
const GLOBAL_SWITCHES: &[&str] = &["--pretty", "--no-color", "--yes", "--help", "-h"];

static PANIC_AT: OnceLock<String> = OnceLock::new();
static HOOK: Once = Once::new();

/// Remember where this process first panics, then let the panic print and
/// unwind exactly as it would have: the hook it wraps still runs.
pub fn capture_panic_location() {
    HOOK.call_once(|| {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            if let Some(location) = info.location() {
                let _ = PANIC_AT.set(location_text(location.file(), location.line()));
            }
            previous(info);
        }));
    });
}

/// Where this process panicked, when it did.
pub fn panic_location() -> Option<String> {
    PANIC_AT.get().cloned()
}

/// A panic's `file:line` as it may leave the machine: a workspace path stays
/// as the compiler named it; a dependency or standard-library path loses the
/// machine's own directories (a home directory names its user).
pub fn location_text(file: &str, line: u32) -> String {
    let file = file.replace('\\', "/");
    let after = |marker: &str| file.find(marker).map(|at| &file[at + marker.len()..]);
    let shown = if let Some(rest) = after("/registry/src/") {
        // `<index>/<crate>-<version>/src/...`: the crate and its version.
        rest.split_once('/')
            .map_or(rest, |(_, rest)| rest)
            .to_string()
    } else if let Some(rest) = after("/git/checkouts/") {
        rest.to_string()
    } else if let Some(rest) = file.strip_prefix("/rustc/") {
        rest.split_once('/')
            .map_or(rest, |(_, rest)| rest)
            .to_string()
    } else if file.starts_with('/') || file.get(1..2) == Some(":") {
        let words: Vec<&str> = file.rsplit('/').take(4).collect();
        words.into_iter().rev().collect::<Vec<_>>().join("/")
    } else {
        file.clone()
    };
    format!("{shown}:{line}")
}

/// One word of the argument list and, when it is a value a declared input
/// takes, that input: only those words may be elided.
struct Word {
    prefix: String,
    value: String,
    value_of: Option<&'static str>,
}

impl Word {
    fn kept(text: &str) -> Self {
        Self {
            prefix: String::new(),
            value: text.to_string(),
            value_of: None,
        }
    }
    fn text(&self) -> String {
        format!("{}{}", self.prefix, self.value)
    }
}

/// The replay of `argv` (every word after `ds`) for the resolved `command`,
/// or nothing when even the elided list cannot meet the backlog's bound.
pub fn replay(
    argv: &[String],
    command: &'static Command,
    panic_location: Option<String>,
) -> Option<Replay> {
    let mut redacted: Vec<String> = Vec::new();
    let mut words = project(argv, command, &mut redacted);
    let mut replay = Replay {
        version: build::VERSION.to_string(),
        source_sha: build::SOURCE_SHA.to_string(),
        dirty: build::dirty(),
        channel: ds_cli_auth::reliability::release_lane().to_string(),
        os: std::env::consts::OS.to_string(),
        arch: std::env::consts::ARCH.to_string(),
        command: command.id.to_string(),
        argv: words.iter().map(Word::text).collect(),
        redacted,
        elided: Vec::new(),
        panic_location,
    };
    while replay.encode().chars().count() > REPLAY_MAX_CHARS {
        let longest = words
            .iter()
            .enumerate()
            .filter(|(_, word)| word.value_of.is_some())
            .max_by_key(|(_, word)| word.value.chars().count())
            .map(|(index, _)| index)?;
        let word = &mut words[longest];
        if let Some(name) = word.value_of.take() {
            if !replay.elided.iter().any(|seen| seen == name) {
                replay.elided.push(name.to_string());
            }
        }
        word.value = ELIDED.to_string();
        replay.argv[longest] = word.text();
    }
    replay.is_valid().then_some(replay)
}

/// Walk the words the way `ds` reads them: global flags anywhere until `--`,
/// the command's path words, its declared flags and operands.
fn project(argv: &[String], command: &'static Command, redacted: &mut Vec<String>) -> Vec<Word> {
    let positionals: Vec<&'static Arg> = command
        .args
        .iter()
        .filter(|arg| arg.kind == ArgKind::Positional)
        .collect();
    let own_version = command.arg("version").is_some();
    let mut path = command.path.iter().peekable();
    let mut filled = 0usize;
    let mut operands = false;
    let mut words = Vec::with_capacity(argv.len());
    let mut index = 0usize;
    let value = |arg: &'static Arg, prefix: String, text: &str, redacted: &mut Vec<String>| {
        if arg.is_sensitive() {
            if !redacted.iter().any(|seen| seen == arg.name) {
                redacted.push(arg.name.to_string());
            }
            Word {
                prefix,
                value: REDACTED.to_string(),
                value_of: None,
            }
        } else {
            Word {
                prefix,
                value: text.to_string(),
                value_of: Some(arg.name),
            }
        }
    };
    let unbound = |prefix: String| Word {
        prefix,
        value: UNBOUND.to_string(),
        value_of: None,
    };
    while index < argv.len() {
        let token = &argv[index];
        index += 1;
        if operands {
            words.push(match positionals.get(filled) {
                Some(arg) => {
                    filled += 1;
                    value(arg, String::new(), token, redacted)
                }
                None => unbound(String::new()),
            });
            continue;
        }
        if token == "--" {
            operands = true;
            words.push(Word::kept(token));
            continue;
        }
        if token == "--output" {
            words.push(Word::kept(token));
            if let Some(format) = argv.get(index) {
                index += 1;
                words.push(Word::kept(format));
            }
            continue;
        }
        if token.starts_with("--output=")
            || GLOBAL_SWITCHES.contains(&token.as_str())
            || (token == "--version" && !own_version)
        {
            words.push(Word::kept(token));
            continue;
        }
        if let Some(rest) = token.strip_prefix("--") {
            let (name, inline) = match rest.split_once('=') {
                Some((name, inline)) => (name, Some(inline)),
                None => (rest, None),
            };
            let flag = format!("--{name}");
            match command.arg(name) {
                Some(arg) if matches!(arg.kind, ArgKind::Value | ArgKind::Repeated) => match inline
                {
                    Some(inline) => words.push(value(arg, format!("{flag}="), inline, redacted)),
                    None => {
                        words.push(Word::kept(&flag));
                        if let Some(next) = argv.get(index).filter(|next| !next.starts_with("--")) {
                            index += 1;
                            words.push(value(arg, String::new(), next, redacted));
                        }
                    }
                },
                // A switch or an operand written as a flag: the name is
                // vocabulary; anything attached to it is not an input's value.
                Some(_) => words.push(match inline {
                    Some(_) => unbound(format!("{flag}=")),
                    None => Word::kept(&flag),
                }),
                None => {
                    if reporter::flag_names(std::slice::from_ref(&flag)).is_empty() {
                        words.push(unbound(String::new()));
                        continue;
                    }
                    words.push(match inline {
                        Some(_) => unbound(format!("{flag}=")),
                        None => Word::kept(&flag),
                    });
                    if inline.is_none() {
                        if argv.get(index).is_some_and(|next| !next.starts_with("--")) {
                            index += 1;
                            words.push(unbound(String::new()));
                        }
                    }
                }
            }
            continue;
        }
        if filled == 0 && path.peek().is_some_and(|word| **word == token.as_str()) {
            path.next();
            words.push(Word::kept(token));
            continue;
        }
        words.push(match positionals.get(filled) {
            Some(arg) => {
                filled += 1;
                value(arg, String::new(), token, redacted)
            }
            None => unbound(String::new()),
        });
    }
    words
}
