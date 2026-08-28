//! Strict argument parsing against a command's declared inputs.
//!
//! Strict means: a flag the command did not declare is an error, not an
//! ignored token. A value outside a declared choice set is an error. A
//! required input that is absent is an error *naming what is missing*. Every
//! one of those errors is a typed [`Failure`] with a stable code, because
//! "invalid input" is the failure an agent hits most and the one it most
//! needs to fix without a human.
//!
//! Unknown flags get a near-miss suggestion. An agent that guessed `--file`
//! for `--model` should be told, not left to re-read help.

use std::collections::BTreeMap;

use serde_json::json;

use crate::outcome::Failure;
use crate::spec::{Arg, ArgKind, Command};

/// Parsed inputs for one command, validated against its declaration.
#[derive(Debug, Default)]
pub struct Inputs {
    values: BTreeMap<&'static str, String>,
    repeated: BTreeMap<&'static str, Vec<String>>,
    switches: BTreeMap<&'static str, bool>,
}

impl Inputs {
    /// The value of a declared `Value` argument, falling back to its declared
    /// default. Returns `None` only for an optional flag with no default.
    pub fn value(&self, name: &str) -> Option<&str> {
        self.values.get(name).map(String::as_str)
    }

    /// The value of a required argument. Parsing already proved it is
    /// present, so this cannot fail for a correctly declared command.
    pub fn require(&self, name: &str) -> Result<&str, Failure> {
        self.value(name).ok_or_else(|| {
            Failure::internal(
                "missing_declared_input",
                format!("input `--{name}` was declared required but is absent after parsing"),
            )
        })
    }

    pub fn repeated(&self, name: &str) -> &[String] {
        self.repeated.get(name).map_or(&[], Vec::as_slice)
    }

    pub fn switch(&self, name: &str) -> bool {
        self.switches.get(name).copied().unwrap_or(false)
    }
}

/// Parse `tokens` (everything after the command path) against `command`.
pub fn parse(command: &Command, tokens: &[String]) -> Result<Inputs, Failure> {
    let mut inputs = Inputs::default();
    let mut index = 0usize;

    let positionals: Vec<&Arg> = command
        .args
        .iter()
        .filter(|arg| arg.kind == ArgKind::Positional)
        .collect();
    let mut filled = 0usize;
    // POSIX `--`: after it, a token is an operand even if it looks like a
    // flag. Without it there is no way to pass a value whose text begins with
    // `--`, and the global scan in `ds` would read such a token as its own.
    let mut operands_only = false;

    while index < tokens.len() {
        let token = &tokens[index];
        index += 1;

        if !operands_only && token == "--" {
            operands_only = true;
            continue;
        }

        let stripped = if operands_only {
            None
        } else {
            token.strip_prefix("--")
        };

        let Some(rest) = stripped else {
            let Some(arg) = positionals.get(filled) else {
                return Err(if positionals.is_empty() {
                    Failure::invalid(
                        "unexpected_operand",
                        format!("`{token}` is not a flag; this command takes named inputs only"),
                    )
                    .remedy("pass inputs as `--name value`")
                    .next(format!("ds {} --help", command.path.join(" ")))
                } else {
                    Failure::invalid(
                        "too_many_operands",
                        format!(
                            "`ds {}` takes {} operand(s); `{token}` is one too many",
                            command.path.join(" "),
                            positionals.len()
                        ),
                    )
                    .remedy("pass any further inputs as `--name value`")
                    .next(format!("ds {} --help", command.path.join(" ")))
                });
            };
            check_choice(command, arg, token)?;
            inputs.values.insert(arg.name, token.clone());
            filled += 1;
            continue;
        };

        let (name, inline) = match rest.split_once('=') {
            Some((name, value)) => (name, Some(value.to_string())),
            None => (rest, None),
        };

        let Some(arg) = command.arg(name) else {
            return Err(unknown_flag(command, name));
        };

        match arg.kind {
            ArgKind::Switch => {
                if inline.is_some() {
                    return Err(Failure::invalid(
                        "switch_takes_no_value",
                        format!("`--{name}` is a switch and takes no value"),
                    )
                    .remedy(format!("pass `--{name}` on its own")));
                }
                inputs.switches.insert(arg.name, true);
            }
            ArgKind::Positional => {
                // Reachable only if a caller wrote `--name` for an operand.
                // Accepting it silently would teach two spellings for one
                // input; naming the operand form keeps exactly one.
                return Err(Failure::invalid(
                    "operand_not_a_flag",
                    format!("`{name}` is an operand, not a flag"),
                )
                .remedy(format!(
                    "write it bare: `ds {} {}`",
                    command.path.join(" "),
                    arg.value
                ))
                .next(format!("ds {} --help", command.path.join(" "))));
            }
            ArgKind::Value | ArgKind::Repeated => {
                let value = match inline {
                    Some(value) => value,
                    None => {
                        let next = tokens.get(index).filter(|next| !next.starts_with("--"));
                        match next {
                            Some(value) => {
                                index += 1;
                                value.clone()
                            }
                            None => {
                                return Err(Failure::invalid(
                                    "missing_value",
                                    format!("`--{name}` needs a value"),
                                )
                                .remedy(format!("pass `--{name} {}`", arg.value))
                                .next(format!("ds {} --help", command.path.join(" "))));
                            }
                        }
                    }
                };
                check_choice(command, arg, &value)?;
                if arg.kind == ArgKind::Repeated {
                    inputs.repeated.entry(arg.name).or_default().push(value);
                } else if inputs.values.insert(arg.name, value).is_some() {
                    return Err(Failure::invalid(
                        "repeated_flag",
                        format!("`--{name}` was given more than once"),
                    )
                    .remedy(format!("pass `--{name}` once")));
                }
            }
        }
    }

    for arg in command.args {
        match arg.kind {
            ArgKind::Positional | ArgKind::Value => {
                if !inputs.values.contains_key(arg.name) {
                    if let Some(default) = arg.default {
                        inputs.values.insert(arg.name, default.to_string());
                    } else if arg.required {
                        return Err(Failure::invalid(
                            "missing_input",
                            format!("`--{}` is required", arg.name),
                        )
                        .remedy(format!("pass `--{} {}`", arg.name, arg.value))
                        .next(format!("ds {} --help", command.path.join(" "))));
                    }
                }
            }
            ArgKind::Repeated => {
                if arg.required && !inputs.repeated.contains_key(arg.name) {
                    return Err(Failure::invalid(
                        "missing_input",
                        format!("`--{}` is required at least once", arg.name),
                    )
                    .remedy(format!("pass `--{} {}`", arg.name, arg.value))
                    .next(format!("ds {} --help", command.path.join(" "))));
                }
            }
            ArgKind::Switch => {}
        }
    }

    Ok(inputs)
}

fn check_choice(command: &Command, arg: &Arg, value: &str) -> Result<(), Failure> {
    if arg.choices.is_empty() || arg.choices.contains(&value) {
        return Ok(());
    }
    Err(Failure::invalid(
        "invalid_choice",
        format!("`--{}` does not accept that value", arg.name),
    )
    .remedy(format!("use one of: {}", arg.choices.join(", ")))
    .detail(json!({ "flag": arg.name, "accepted": arg.choices }))
    .next(format!("ds {} --help", command.path.join(" "))))
}

fn unknown_flag(command: &Command, name: &str) -> Failure {
    let mut failure = Failure::invalid(
        "unknown_flag",
        format!(
            "`--{name}` is not an input of `ds {}`",
            command.path.join(" ")
        ),
    );
    if let Some(suggestion) = nearest(name, command.args.iter().map(|arg| arg.name)) {
        failure = failure.remedy(format!("did you mean `--{suggestion}`?"));
    } else {
        failure = failure.remedy("run the command's help for its declared inputs");
    }
    failure.next(format!("ds {} --help", command.path.join(" ")))
}

/// The closest candidate within a small edit distance, or nothing. Silence is
/// better than a confidently wrong suggestion.
pub fn nearest<'a>(input: &str, candidates: impl Iterator<Item = &'a str>) -> Option<&'a str> {
    let budget = match input.chars().count() {
        0..=3 => 1,
        4..=7 => 2,
        _ => 3,
    };
    candidates
        .map(|candidate| (distance(input, candidate), candidate))
        .filter(|(distance, _)| *distance <= budget)
        .min_by_key(|(distance, candidate)| (*distance, candidate.len()))
        .map(|(_, candidate)| candidate)
}

/// Levenshtein distance over `char`s, two rows at a time.
fn distance(left: &str, right: &str) -> usize {
    let left: Vec<char> = left.chars().collect();
    let right: Vec<char> = right.chars().collect();
    if left.is_empty() {
        return right.len();
    }
    let mut previous: Vec<usize> = (0..=right.len()).collect();
    let mut current = vec![0usize; right.len() + 1];
    for (i, left_char) in left.iter().enumerate() {
        current[0] = i + 1;
        for (j, right_char) in right.iter().enumerate() {
            let substitution = previous[j] + usize::from(left_char != right_char);
            current[j + 1] = substitution.min(previous[j + 1] + 1).min(current[j] + 1);
        }
        std::mem::swap(&mut previous, &mut current);
    }
    previous[right.len()]
}

#[cfg(test)]
mod tests {
    use super::parse;
    use crate::spec::{Arg, Authority, Availability, Chapter, Command, Effect, Execution, Refusal};

    fn available() -> Availability {
        Availability::Available
    }

    /// A compact parser fixture for operand edge cases. Shipping discovery
    /// already has an operand (`capabilities selector`); its end-to-end global
    /// confirmation boundary is pinned in the `ds` binary tests.
    static WITH_OPERAND: Command = Command {
        id: "test.operand",
        path: &["test", "operand"],
        contract: 1,
        chapter: Chapter::Catalog,
        summary: "Takes one bare operand.",
        purpose: "Exists only to hold the operand parsing rules to their contract.",
        effect: Effect::Discovery,
        authority: Authority::None,
        execution: Execution::Sync,
        args: &[
            Arg::positional("subject", "<subject>", "The thing named."),
            Arg::value("model", "<name>", "An ordinary valued flag."),
        ],
        output: "Nothing; this command is never dispatched.",
        examples: &[],
        refusals: &[Refusal {
            code: "unexpected_operand",
            when: "a bare token arrives at a command that declares none",
            remedy: "pass inputs as `--name value`",
        }],
        reference: None,
        availability: available,
    };

    fn tokens(parts: &[&str]) -> Vec<String> {
        parts.iter().map(|part| (*part).to_string()).collect()
    }

    #[test]
    fn a_sentinel_turns_a_flag_looking_token_into_the_operand() {
        let inputs = parse(&WITH_OPERAND, &tokens(&["--", "--yes"])).expect("parsed");
        assert_eq!(inputs.value("subject"), Some("--yes"));
    }

    #[test]
    fn the_sentinel_still_lets_real_flags_come_first() {
        let inputs =
            parse(&WITH_OPERAND, &tokens(&["--model", "m1", "--", "--output"])).expect("parsed");
        assert_eq!(inputs.value("model"), Some("m1"));
        assert_eq!(inputs.value("subject"), Some("--output"));
    }

    #[test]
    fn a_second_sentinel_is_an_operand_not_another_sentinel() {
        let inputs = parse(&WITH_OPERAND, &tokens(&["--", "--"])).expect("parsed");
        assert_eq!(inputs.value("subject"), Some("--"));
    }

    #[test]
    fn a_declared_flag_after_the_sentinel_is_no_longer_a_flag() {
        let refused = parse(&WITH_OPERAND, &tokens(&["--", "a", "--model", "m1"])).unwrap_err();
        assert_eq!(refused.code(), "too_many_operands");
    }

    #[test]
    fn a_lone_sentinel_changes_nothing() {
        let inputs = parse(&WITH_OPERAND, &tokens(&["--"])).expect("parsed");
        assert_eq!(inputs.value("subject"), None);
        assert_eq!(inputs.value("model"), None);
    }
}
