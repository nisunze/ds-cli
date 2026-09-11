//! Every refusal code a command can emit must be documented.
//!
//! `ds` promises that a caller can plan for failure from `--help` alone: the
//! REFUSALS section lists each code with the situation that produces it and
//! the remedy. That promise is only worth anything if the list is complete,
//! and completeness is exactly the property that rots — a handler grows a new
//! `Failure::invalid("something_new", …)` and nothing notices.
//!
//! So this test reads the domain crates' own source, collects every error code
//! they can construct — written out, named through a declared `Refusal`
//! constant, or dispatched by a match arm — and requires each one to appear in
//! some command's declared refusals. It is source analysis rather than
//! execution because most of these codes are reached only in situations a test
//! cannot reliably produce — a full disk, a killed engine, a corrupted package.
//!
//! Codes that are genuinely internal-only are listed in [`NOT_A_REFUSAL`],
//! with the reason. That list is the escape hatch, and it is deliberately
//! short: putting a code there is a claim that a caller can never see it.
//!
//! A source scan fails silently — a shape it cannot read looks like a shape
//! that is not there — so a call site whose code this scan cannot resolve is
//! not skipped but reported, and has to be accounted for in
//! [`CODE_NOT_A_LITERAL`].

use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

mod common;

/// Codes a caller cannot reach, with why.
const NOT_A_REFUSAL: &[(&str, &str)] = &[
    (
        "missing_declared_input",
        "raised only if a command declares an input required and the parser \
         then fails to supply it — a defect in ds, not a situation a caller \
         can create",
    ),
    (
        "unmapped_task",
        "raised only if a validated --task choice has no engine subcommand \
         behind it, which the choice list makes unreachable",
    ),
    (
        "unmapped_choice",
        "raised only if a validated --target/--mode/--container choice has no          engine value behind it, which the choice list makes unreachable",
    ),
    (
        "invalid_lane",
        "raised only if the auth host receives a lane outside the command's parser-enforced stable/canary choices",
    ),
    (
        "callee_wait_failed",
        "raised only if the OS cannot report on a child ds itself spawned",
    ),
    (
        "undeclared_bridge_argument",
        "raised only if a ds map handler builds an argument key its own \
         BridgeOp does not declare — a defect in ds caught at the boundary, \
         and one tests/bridge_parity.rs proves cannot be a schema drift",
    ),
    (
        "catalog_action_invalid",
        "raised only if a parser-validated global catalog action has no match arm behind it",
    ),
    (
        "catalog_action_not_allowed",
        "raised only if a parser-validated global catalog action escapes the exact read/write allowlist that declared it",
    ),
    (
        "data_distribution_unavailable",
        "emitted by ds-cli-auth's `data_distribution` host, which no command calls yet: the \
         headless `ds data project-cache status|seed` commands of the seeding design land in a \
         later slice and must declare `DATA_DISTRIBUTION_UNAVAILABLE_REFUSAL`, at which point this \
         entry has to go",
    ),
    (
        "reference_bundle_download_failed",
        "emitted by ds-cli-auth's `download_reference_bundle` host, which no command calls yet: \
         the same later slice declares `REFERENCE_BUNDLE_DOWNLOAD_FAILED_REFUSAL` and removes this \
         entry",
    ),
];

/// Call sites whose code argument this scan cannot read, and why.
///
/// A constructor handed a variable spells no code, so there is nothing here to
/// check; listing the file is a claim that its codes are written down
/// somewhere this scan does reach — beside the value that chooses them, or on
/// the application's side of the bridge. The list is short and required: a NEW
/// dynamic constructor fails this test rather than quietly removing its codes
/// from the check, which is how `ds tile` once had every one of its codes
/// unscanned while the test passed.
const CODE_NOT_A_LITERAL: &[(&str, &str)] = &[
    (
        "ds-cli-auth/src/state.rs",
        "the protected-state code is chosen by a `StoreError` match above the \
         constructor; each of the three is declared by the commands that touch \
         that state",
    ),
    (
        "ds-cli-design/src/features.rs",
        "the code is the selection kernel's own `FeatureSelectionError::code()`, \
         mapped here to a class rather than renamed",
    ),
    (
        "ds-cli-design/src/lv/artifact.rs",
        "`exists_code`/`write_code` belong to the `ArtifactContract` being \
         written and are declared as literals beside that contract's name",
    ),
    (
        "ds-cli-desktop/src/bridge.rs",
        "the code is the paired application's own, preserved across the bridge; \
         `every_application_refusal_code_is_documented` is the check for that \
         side",
    ),
    (
        "ds-cli-exec/src/lib.rs",
        "`missing_code` is declared by each callee the caller names, so the \
         literal lives with the command that owns the executable",
    ),
    (
        "ds-cli-library/src/lib.rs",
        "`engine_failure` is a shared wrapper; each of its call sites passes a \
         literal code",
    ),
    (
        "ds-cli-pls/src/deviation_labels.rs",
        "the code is chosen with its remedy by a match above the constructor",
    ),
    (
        "ds-cli-pls/src/terrain_reconcile.rs",
        "the code is chosen with its remedy by a match above the constructor",
    ),
    (
        "ds-cli-solar/src/compare.rs",
        "`require_file` takes the code from the caller that names the flag",
    ),
    (
        "ds-cli-solar/src/exports.rs",
        "the code is chosen from the io error kind above the constructor; both \
         values are declared in `EXPORT_REFUSALS`",
    ),
    (
        "ds-cli-solar/src/paired_run.rs",
        "`require_exact_value` takes the code from the caller that names the \
         input",
    ),
    (
        "ds-cli-solar/src/seed.rs",
        "the fallback arm re-emits the ds-side code `SERVER_CODES` maps the \
         server's own name onto, and every one of those is a literal in \
         `APPLY_REFUSALS` in this same file — which the `expect` beside the \
         match requires. The two named arms above it this scan does resolve",
    ),
];

fn ds(args: &[&str]) -> Value {
    common::json(args).0
}

/// Codes declared by each domain's commands, plus the union across all of
/// them.
///
/// Source ownership is by crate, while a number of commands share a crate and
/// can legitimately share a failure constructor. This static check therefore
/// proves the narrower, truthful invariant: every constructed caller-visible
/// code is declared by at least one command in its owner domain. Per-command
/// execution and descriptor tests cover the command-specific contract; do not
/// describe this aggregate source scan as proof of a particular command's
/// REFUSALS section.
fn declared_codes() -> (BTreeMap<String, BTreeSet<String>>, BTreeSet<String>) {
    let mut by_domain: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut all = BTreeSet::new();
    let index = ds(&["capabilities", "--output", "json"]);

    let mut targets: Vec<(String, String)> = Vec::new();
    for domain in index["data"]["domains"].as_array().expect("domains") {
        let id = domain["id"].as_str().expect("domain id");
        let commands = ds(&["capabilities", id, "--output", "json"]);
        for command in commands["data"]["commands"].as_array().expect("commands") {
            targets.push((
                id.to_string(),
                command["id"].as_str().expect("id").to_string(),
            ));
        }
    }
    for meta in common::META_COMMANDS {
        targets.push(("meta".to_string(), meta.to_string()));
    }

    for (domain, id) in targets {
        let descriptor = ds(&["capabilities", &id, "--output", "json"]);
        let command = &descriptor["data"]["command"];
        let entry = by_domain.entry(domain).or_default();
        for refusal in command["refusals"].as_array().into_iter().flatten() {
            if let Some(code) = refusal["code"].as_str() {
                entry.insert(code.to_string());
                all.insert(code.to_string());
            }
        }
        // An availability check's code is also caller-visible, through the
        // dispatch gate, so it counts as declared.
        if let Some(code) = command["unavailable"]["code"].as_str() {
            entry.insert(code.to_string());
            all.insert(code.to_string());
        }
    }
    (by_domain, all)
}

/// Codes constructed anywhere under `dir`, and the call sites this scan could
/// not read a code from.
///
/// Two passes: the crate's own `Refusal` constants first, because a handler
/// routinely passes `NAME.code` for a constant declared in the crate root, and
/// a scan that cannot follow that reports nothing for a whole domain.
fn constructed_codes(dir: &Path) -> (BTreeSet<String>, Vec<(PathBuf, String)>) {
    let sources: Vec<(PathBuf, String)> = rust_files(dir)
        .into_iter()
        .map(|file| {
            let source = std::fs::read_to_string(&file).expect("read source");
            (file, source)
        })
        .collect();

    let mut consts = BTreeMap::new();
    for (_, source) in &sources {
        consts.extend(refusal_consts(source));
    }

    let mut codes = BTreeSet::new();
    let mut unresolved = Vec::new();
    for (file, source) in &sources {
        let (found, unread) = codes_in_source(source, &consts);
        codes.extend(found);
        unresolved.extend(unread.into_iter().map(|site| (file.clone(), site)));
    }
    (codes, unresolved)
}

const CONSTRUCTORS: &[&str] = &[
    "Failure::new(",
    "Failure::invalid(",
    "Failure::unavailable(",
    "Failure::unauthorized(",
    "Failure::conflict(",
    "Failure::failed(",
    "Failure::internal(",
    "Availability::unavailable(",
];

/// One source's constructed codes, plus the call sites whose code argument is
/// not something this scan can resolve.
///
/// Split out from the crate walk so the shapes below can be probed directly:
/// the failure mode of a source scan is silence, and a shape it drops looks
/// exactly like a shape that is not there.
fn codes_in_source(
    source: &str,
    consts: &BTreeMap<String, String>,
) -> (BTreeSet<String>, Vec<String>) {
    let mut codes = BTreeSet::new();
    let mut unresolved = Vec::new();

    for constructor in CONSTRUCTORS {
        let mut offset = 0;
        while let Some(at) = source[offset..].find(constructor) {
            let call = offset + at;
            offset = call + constructor.len();
            // Match a whole type name: HostFailure has a code-first
            // constructor, unlike the class-first contract Failure.
            if source[..call]
                .chars()
                .next_back()
                .is_some_and(|c| c.is_alphanumeric() || c == '_')
            {
                continue;
            }
            let mut scan = &source[offset..];
            // `Failure::new` takes the class first; skip to the next argument
            // before reading the code.
            if *constructor == "Failure::new(" {
                match scan.find(',') {
                    Some(comma) => scan = &scan[comma + 1..],
                    None => continue,
                }
            }
            // Bound the window instead of refusing newlines. Rustfmt wraps a
            // constructor whose message is long, so the code is routinely on
            // the line *after* the paren — an earlier version of this scan
            // skipped exactly those and reported clean while three codes went
            // undocumented. Slice on a char boundary: these sources contain em
            // dashes and arrows, and a byte-index cut lands inside one.
            let window = &scan[..char_boundary(scan, 300)];
            // The code argument itself must be the literal. Searching the
            // whole window for the first quote instead would read a *later*
            // literal — the next argument, or the next statement's map key —
            // as this call's code, and report a word like `remedy` as an
            // undocumented refusal.
            let argument = skip_trivia(window);
            if let Some(literal) = leading_literal(argument).filter(|code| is_code(code)) {
                codes.insert(literal.to_string());
                continue;
            }
            // `NAME.code` names a declared `Refusal`, so the code is written
            // down — once, beside its `when` and `remedy` — rather than absent.
            if let Some(code) = const_reference(argument, consts) {
                codes.insert(code);
                continue;
            }
            // `"code" => Failure::x(code, …)` dispatches an owner's or a
            // server's code by name. Read BACKWARDS from the call so the arm
            // pattern is the only literal that can be taken: reading forwards
            // is what used to pick up the next argument.
            let patterns = arm_patterns(&source[..call]);
            if !patterns.is_empty() {
                codes.extend(patterns);
                continue;
            }
            unresolved.push(format!("{constructor}{}", one_line(argument)));
        }
    }
    (codes, unresolved)
}

/// `NAME` → code for every `Refusal` constant declared in one source, written
/// out or through a crate's `refusal!` / `native_refusal!` macro.
fn refusal_consts(source: &str) -> BTreeMap<String, String> {
    let mut consts = BTreeMap::new();

    let mut rest = source;
    while let Some(at) = rest.find("const ") {
        rest = &rest[at + "const ".len()..];
        let name = leading_name(rest);
        if name.is_empty() {
            continue;
        }
        let after = rest[name.len()..].trim_start();
        let Some(declaration) = after.strip_prefix(':') else {
            continue;
        };
        let declaration = declaration.trim_start();
        if !declaration.starts_with("Refusal") {
            continue;
        }
        let window = &declaration[..char_boundary(declaration, 300)];
        if let Some(code) = field_literal(window, "code:") {
            consts.insert(name.to_string(), code);
        }
    }

    // `native_refusal!` ends in the same characters, so one search covers both
    // spellings of the macro.
    let mut rest = source;
    while let Some(at) = rest.find("refusal!(") {
        rest = &rest[at + "refusal!(".len()..];
        let arguments = skip_trivia(rest);
        let name = leading_name(arguments);
        if name.is_empty() {
            continue;
        }
        let Some(after_name) = arguments[name.len()..].trim_start().strip_prefix(',') else {
            continue;
        };
        if let Some(code) = leading_literal(skip_trivia(after_name)).filter(|code| is_code(code)) {
            consts.insert(name.to_string(), code.to_string());
        }
    }

    consts
}

/// The `SCREAMING_CASE` identifier at the start of `text`, if any.
fn leading_name(text: &str) -> &str {
    let end = text
        .find(|character: char| {
            !(character.is_ascii_uppercase() || character.is_ascii_digit() || character == '_')
        })
        .unwrap_or(text.len());
    &text[..end]
}

/// The code of `NAME.code` / `path::NAME.code` at the start of `text`.
fn const_reference(text: &str, consts: &BTreeMap<String, String>) -> Option<String> {
    let end = text
        .find(|character: char| {
            !(character.is_ascii_alphanumeric() || character == '_' || character == ':')
        })
        .unwrap_or(text.len());
    let path = &text[..end];
    let after = text[end..].strip_prefix(".code")?;
    if after.starts_with(|character: char| character.is_ascii_alphanumeric() || character == '_') {
        return None;
    }
    consts.get(path.rsplit("::").next()?).cloned()
}

/// The literals of the match arm whose body starts where `before` ends.
///
/// `"a" | "b" => Failure::invalid(code, …)` names both codes; anything else
/// immediately before the call means this is not an arm dispatch and nothing
/// is claimed.
fn arm_patterns(before: &str) -> Vec<String> {
    let Some(mut head) = before.trim_end().strip_suffix("=>") else {
        return Vec::new();
    };
    let mut patterns = Vec::new();
    loop {
        head = head.trim_end();
        let Some(open) = head.strip_suffix('"').and_then(|head| head.rfind('"')) else {
            break;
        };
        let literal = &head[open + 1..head.len() - 1];
        if !is_code(literal) {
            break;
        }
        patterns.push(literal.to_string());
        head = &head[..open];
        match head.trim_end().strip_suffix('|') {
            Some(more) => head = more,
            None => break,
        }
    }
    patterns
}

/// Whitespace and `//` comments, skipped: a comment above the code argument is
/// how this file's own sources explain a constructor, and it must not hide one.
fn skip_trivia(text: &str) -> &str {
    let mut rest = text.trim_start();
    while let Some(comment) = rest.strip_prefix("//") {
        rest = match comment.find('\n') {
            Some(end) => comment[end + 1..].trim_start(),
            None => "",
        };
    }
    rest
}

/// The contents of a double-quoted literal at the start of `text`.
fn leading_literal(text: &str) -> Option<&str> {
    let text = text.strip_prefix('"')?;
    let close = text.find('"')?;
    Some(&text[..close])
}

/// The literal value of `field` within `window`.
fn field_literal(window: &str, field: &str) -> Option<String> {
    let at = window.find(field)?;
    let value = skip_trivia(&window[at + field.len()..]);
    leading_literal(value)
        .filter(|code| is_code(code))
        .map(str::to_string)
}

/// The shape of every refusal code: lowercase words joined by underscores.
fn is_code(code: &str) -> bool {
    !code.is_empty()
        && code
            .chars()
            .all(|character| character.is_ascii_lowercase() || character == '_')
}

/// A call site's argument, flattened to one readable line for a failure.
fn one_line(text: &str) -> String {
    let collapsed = text.split_whitespace().collect::<Vec<_>>().join(" ");
    collapsed.chars().take(70).collect()
}

/// The scan's own negative control.
///
/// Every shape below is one the domain crates actually write. The property
/// under test is not only that each resolves, but that a shape this scan
/// cannot read is REPORTED: a silent skip looks exactly like a source with no
/// constructors in it, and the whole check then passes on nothing.
///
/// `Failure::new` earns its place for a second reason: it is the only
/// constructor whose code is not the first argument, so the class-skip above is
/// the only thing standing between `result_exists` and silence.
///
/// `Availability::unavailable` earns its place for a third: it is the one
/// entry in [`CONSTRUCTORS`] that is not a `Failure`, and dropping that needle
/// costs the live tree two codes without failing anything — every crate that
/// writes it also writes a `Failure`, so no domain goes empty and no
/// `CODE_NOT_A_LITERAL` entry goes stale. Only a fixture states it.
#[test]
fn codes_in_source_reads_every_constructor_shape() {
    const SOURCE: &str = r#"
pub const PROJECT_CONTEXT_CHANGED: Refusal = Refusal {
    code: "project_context_changed",
    when: "the selected project changed between two reads",
    remedy: "run the plan again against the current selected project",
};

native_refusal!(
    NATIVE_PROFILE,
    "native_profile_not_configured",
    "the exact packaged native profile is unavailable",
    "install one complete ds release"
);

fn wrapped() -> Failure {
    Failure::invalid(
        "wrapped_literal",
        format!("a message long enough that rustfmt puts the code on its own line"),
    )
    .remedy("the remedy is not a code")
}

fn host_error() { HostFailure::new("host_literal", "message"); }
fn unrelated() { DifferentFailure::new("unrelated", "not_a_code"); }

fn through_a_const() -> Failure {
    Failure::conflict(PROJECT_CONTEXT_CHANGED.code, "the project changed")
        .remedy(PROJECT_CONTEXT_CHANGED.remedy)
}

fn through_a_path_const() -> Failure {
    Failure::unavailable(crate::NATIVE_PROFILE.code, "no packaged profile")
}

fn dispatch(code: &str, message: String) -> Failure {
    match code {
        "arm_first" | "arm_second" => Failure::invalid(code, message),
        "arm_wrapped" => Failure::failed(code, message).remedy("split the request"),
        _ => Failure::internal("arm_fallback", message),
    }
}

fn class_first() -> Failure {
    Failure::new(
        ExitClass::Conflict,
        "class_first_literal",
        format!("`{}` already exists", path.display()),
    )
    .remedy("choose a new path; the engine has no --force, on purpose")
}

fn behind_a_comment() -> Failure {
    Failure::unauthorized(
        // Written out here so the detail key below cannot be read as the code.
        "commented_literal",
        "a message",
    )
    .detail(json!({ "remedy_key": "not a code" }))
}

fn availability(is_windows: bool) -> Availability {
    if is_windows {
        Availability::unavailable(
            "availability_literal",
            "this durable state cannot be proven on Windows",
            "use Linux, or install a build that proves it",
        )
    } else {
        Availability::Available
    }
}

fn dynamic(&self) -> Failure {
    Failure::unavailable(self.missing_code, format!("`{}` was not found", self.name))
}
"#;

    let consts = refusal_consts(SOURCE);
    assert_eq!(
        consts.get("PROJECT_CONTEXT_CHANGED").map(String::as_str),
        Some("project_context_changed"),
        "a written-out Refusal constant must resolve"
    );
    assert_eq!(
        consts.get("NATIVE_PROFILE").map(String::as_str),
        Some("native_profile_not_configured"),
        "a macro-declared Refusal constant must resolve"
    );

    let (codes, unresolved) = codes_in_source(SOURCE, &consts);
    let expected: BTreeSet<String> = [
        "arm_fallback",
        "arm_first",
        "arm_second",
        "arm_wrapped",
        "availability_literal",
        "class_first_literal",
        "commented_literal",
        "native_profile_not_configured",
        "project_context_changed",
        "wrapped_literal",
    ]
    .iter()
    .map(|code| code.to_string())
    .collect();
    assert_eq!(codes, expected);

    // The one genuinely dynamic call is named rather than dropped, which is
    // what CODE_NOT_A_LITERAL then has to account for.
    assert_eq!(unresolved.len(), 1, "unresolved: {unresolved:?}");
    assert!(
        unresolved[0].contains("Failure::unavailable(") && unresolved[0].contains("missing_code"),
        "an unreadable site must name its constructor and argument: {}",
        unresolved[0]
    );
}

/// The largest char boundary at or below `limit` bytes.
fn char_boundary(text: &str, limit: usize) -> usize {
    let mut end = limit.min(text.len());
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    end
}

fn rust_files(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return files;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            files.extend(rust_files(&path));
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            files.push(path);
        }
    }
    files
}

fn crates_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("workspace root")
        .join("crates")
}

#[test]
fn every_constructible_refusal_code_is_documented() {
    let (by_domain, all_declared) = declared_codes();
    assert!(
        !all_declared.is_empty(),
        "no refusal codes were declared at all"
    );

    let exempt: BTreeSet<&str> = NOT_A_REFUSAL.iter().map(|(code, _)| *code).collect();
    let root = crates_root();

    // Domain crates map to the domain whose commands must document them.
    // `ds-cli-contract` is excluded: its codes are the parser's own
    // (unknown_flag, missing_value, invalid_choice …) and apply to every
    // command equally, so they are documented once in the output contract
    // rather than repeated in every REFUSALS section.
    let domain_crates = [
        // Native auth is now a shared selected-project client boundary used by
        // Design, Survey/Forms, and Solar commands as well as `ds auth`.
        // Caller commands declare the relevant helper refusals.
        ("ds-cli-auth", None),
        ("ds-cli-data", Some("data")),
        ("ds-cli-design", Some("design")),
        ("ds-cli-map", Some("map")),
        ("ds-cli-dsgrid", Some("dsgrid")),
        ("ds-cli-dsgrid-exchange", Some("dsgrid-exchange")),
        ("ds-cli-library", Some("library")),
        ("ds-cli-pls", Some("pls")),
        ("ds-cli-report", Some("report")),
        ("ds-cli-server", Some("server")),
        ("ds-cli-solar", Some("solar")),
        ("ds-cli-work", Some("work")),
        ("ds-cli-assets", Some("assets")),
        ("ds-cli-sre", Some("sre")),
        ("ds-cli-survey", Some("survey")),
        ("ds-cli-style", Some("style")),
        ("ds-cli-tile", Some("tile")),
        ("ds-cli-feedback", Some("feedback")),
        ("ds-cli-shell", Some("shell")),
        ("ds-cli-workstation", Some("workstation")),
        ("ds-cli-mcp", Some("mcp")),
        // Receipt verification returns bounded diagnostic strings to doctor
        // and MCP resources; it constructs no CLI Failure/refusal codes.
        ("ds-cli-skills", None),
        // Shared across every calling domain; declaring it in any one of them
        // is enough for this check, and the per-command help of each caller
        // is what the domain checks above enforce.
        ("ds-cli-exec", None),
        // Also shared, and it became so: `ds-cli-desktop` is the paired-session
        // authority surface, and `ds solar prepare` borrows it to have the
        // application perform an authenticated fetch. Its pairing refusals are
        // therefore reachable from more than the `desktop` domain, and each
        // caller declares them in its own REFUSALS — which is what a reader of
        // one command's help actually needs.
        ("ds-cli-desktop", None),
        // The layer drawer's shared application owner: `ds map layer …` and
        // `ds server layers …` both call it, and each declares its refusals
        // in its own command lists (`ds-cli-map::layer::native`,
        // `ds-cli-server::LAYER_REFUSALS`), which is what a reader of one
        // command's help needs.
        ("ds-layer-ops", None),
    ];

    // A domain crate missing from the list above is silently unchecked, which
    // is exactly how a new domain ships undocumented codes. Prove the list
    // covers every crate on disk rather than trusting that it does.
    let listed: BTreeSet<&str> = domain_crates.iter().map(|(name, _)| *name).collect();
    for entry in std::fs::read_dir(&root).expect("read crates dir").flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name == "ds" || name == "ds-cli-contract" || !entry.path().is_dir() {
            continue;
        }
        assert!(
            listed.contains(name.as_str()),
            "crate `{name}` is not covered by this test. Add it to `domain_crates` \
             with the domain whose commands must document its codes."
        );
    }

    let accounted: BTreeSet<&str> = CODE_NOT_A_LITERAL.iter().map(|(file, _)| *file).collect();
    let mut listed_and_seen: BTreeSet<&str> = BTreeSet::new();
    let mut undocumented: Vec<String> = Vec::new();
    let mut unreadable: Vec<String> = Vec::new();
    let mut scanned = 0usize;
    for (crate_name, domain) in domain_crates {
        let dir = root.join(crate_name).join("src");
        assert!(dir.is_dir(), "crate source missing: {}", dir.display());

        let declared = match domain {
            Some(domain) => by_domain.get(domain).cloned().unwrap_or_default(),
            None => all_declared.clone(),
        };

        let (constructed, unresolved) = constructed_codes(&dir);
        // A crate the scan can read no code from contributes nothing, so the
        // check passes on an empty set for that whole domain. `ds tile` was
        // exactly that: its one constructor takes `PROJECT_CONTEXT_CHANGED.code`
        // and the scan reported zero codes for the entire tile domain.
        assert!(
            !constructed.is_empty() || unresolved.is_empty(),
            "`{crate_name}` constructs {} failure(s) and this scan could read the \
             code of none of them, so nothing in that domain is checked:\n{}",
            unresolved.len(),
            unresolved
                .iter()
                .map(|(_, site)| format!("  {site}"))
                .collect::<Vec<_>>()
                .join("\n")
        );
        scanned += constructed.len();

        for (file, site) in unresolved {
            let relative = file
                .strip_prefix(&root)
                .unwrap_or(&file)
                .display()
                .to_string()
                .replace('\\', "/");
            match accounted.get(relative.as_str()) {
                Some(listed) => {
                    listed_and_seen.insert(listed);
                }
                None => unreadable.push(format!("  {relative}: {site}")),
            }
        }

        for code in constructed {
            if declared.contains(&code) || exempt.contains(code.as_str()) {
                continue;
            }
            match domain {
                Some(domain) => undocumented.push(format!(
                    "  {crate_name}: `{code}` (no `ds {domain}` command declares it)"
                )),
                None => undocumented.push(format!(
                    "  {crate_name}: `{code}` (no command anywhere declares it)"
                )),
            }
        }
    }

    assert!(
        scanned > 0,
        "no constructor call site under {} yielded a code. The scan is matching \
         nothing, which would make this check vacuous — confirm the failure \
         constructors are still spelled as CONSTRUCTORS lists them.",
        root.display()
    );

    assert!(
        unreadable.is_empty(),
        "this scan cannot tell which code these call sites emit:\n{}\n\n\
         Pass the code as a literal, as a declared `NAME.code`, or dispatch it \
         from a match arm naming the code — any of which this scan reads. If it \
         is genuinely dynamic, add the file to CODE_NOT_A_LITERAL with the \
         reason its codes are written down elsewhere.",
        unreadable.join("\n")
    );

    let stale: Vec<&str> = accounted
        .iter()
        .filter(|file| !listed_and_seen.contains(*file))
        .copied()
        .collect();
    assert!(
        stale.is_empty(),
        "CODE_NOT_A_LITERAL lists files that no longer hold an unreadable \
         constructor: {}. Remove them, so the list keeps saying what is \
         actually unchecked.",
        stale.join(", ")
    );

    assert!(
        undocumented.is_empty(),
        "these refusal codes can be emitted but are not documented:\n{}\n\n\
         Add each to the REFUSALS of the command that emits it — with the \
         situation and a remedy — or, if a caller truly cannot reach it, list \
         it in NOT_A_REFUSAL with the reason.",
        undocumented.join("\n")
    );
}

/// The paired application's half of the same invariant.
///
/// A refusal raised in DS GridDesign now reaches the caller with its own class,
/// code and remedy for *every* operation, not just an allowlisted two (see
/// `structured_desktop_refusal` in `ds-cli-desktop`). The allowlist was what
/// used to guarantee that a code crossing the bridge was one `ds` documents;
/// removing it without replacing that guarantee would let the application mint
/// codes no `--help` mentions. This test is the replacement: the *declaration*
/// is the contract, and an undeclared code is a failing build rather than a
/// surprise in production.
///
/// Source analysis, like the Rust half above, and for the same reason: these
/// refusals are reached only in situations a test cannot reliably produce.
fn ds_web() -> Option<PathBuf> {
    let root = match std::env::var_os("DS_WEB_DIR") {
        Some(explicit) => PathBuf::from(explicit),
        None => PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../ds-web"),
    };
    let root = root.canonicalize().unwrap_or(root);
    root.is_dir().then_some(root)
}

/// The shapes that mint an application refusal. The class is the first
/// argument and the code the second in both.
///
/// `cliRefusal` is the helper every throw site uses, but the class it wraps is
/// exported and constructible directly, so `new CliOperationRefusal(…)` is the
/// same mint written out and has to be read the same way.
const APPLICATION_MINTS: &[&str] = &["cliRefusal(", "new CliOperationRefusal("];

/// What `structured_desktop_refusal` in `ds-cli-desktop` will actually carry:
/// 1..=80 bytes of `[a-z0-9_]`.
///
/// Stated to match the bridge exactly rather than approximated as `[a-z_]`,
/// which silently dropped every code with a digit in it — codes the bridge
/// accepts, so dropping them here was a hole in the check, not a filter on it.
fn is_bridge_code(code: &str) -> bool {
    !code.is_empty()
        && code.len() <= 80
        && code
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
}

/// The leading identifier of `text`, empty when it does not start with one.
fn identifier(text: &str) -> &str {
    let end = text
        .find(|character: char| !character.is_ascii_alphanumeric() && character != '_')
        .unwrap_or(text.len());
    &text[..end]
}

/// A call site this scan could not read a code from: which file, where, why.
fn unreadable_site(file: &Path, source: &str, offset: usize, why: &str) -> String {
    let line = source[..offset].matches('\n').count() + 1;
    format!("  {}:{line} (byte {offset}): {why}", file.display())
}

/// The literal codes a file states as `code: '…'`.
///
/// Two shapes need this, and neither passes a literal to a mint. A wire object
/// assembled by hand is one: `offlineRefusal` used to build a
/// `CliStructuredRefusal` itself, which is why the one code typed at a sink was
/// the one code this scan never saw. An identity table is the other:
/// `design-collab.ts` maps a refused envelope onto a class, code and remedy and
/// hands the result to `cliRefusal`, so the codes it can emit are literals here
/// rather than at the call.
///
/// Only literals the bridge would carry are kept, and an unreadable one is not
/// reported: this key also holds relayed values (`code: error.code`, the wire
/// branch) and codes that are not refusals at all, so silence here is not
/// evidence of anything. The mint scan is where a code the check cannot read
/// is a defect.
fn literal_code_table(source: &str) -> BTreeSet<String> {
    let mut codes = BTreeSet::new();
    let mut rest = source;
    while let Some(at) = rest.find("code:") {
        rest = &rest[at + "code:".len()..];
        let value = rest.trim_start();
        let Some(quote) = value
            .chars()
            .next()
            .filter(|character| *character == '\'' || *character == '"')
        else {
            continue;
        };
        let after = &value[quote.len_utf8()..];
        let Some(close) = after.find(quote) else {
            continue;
        };
        let code = &after[..close];
        if is_bridge_code(code) {
            codes.insert(code.to_string());
        }
    }
    codes
}

/// One application source's refusal codes, plus the mint sites whose code this
/// scan could not read.
///
/// Split out from the walk for the reason the Rust half above is: the shapes
/// this has to read — the class constructed directly, a code carrying a digit,
/// the helper's own declaration — are in ds-web nowhere or once, so a pass
/// against the live tree is equally consistent with a scan that reads none of
/// them. `application_codes_in_source_reads_every_mint_shape` is where they are
/// put in front of it.
///
/// A file that neither mints nor names the wire type is not read at all:
/// `code:` is an ordinary key in this application, and harvesting it everywhere
/// would report half the repo as undocumented refusals.
fn application_codes_in_source(file: &Path, source: &str) -> (BTreeSet<String>, Vec<String>) {
    let mut codes = BTreeSet::new();
    let mut unreadable = Vec::new();
    let mints = APPLICATION_MINTS.iter().any(|mint| source.contains(mint));
    if !mints && !source.contains("CliStructuredRefusal") {
        return (codes, unreadable);
    }
    // The module that declares the helpers holds their own plumbing: the
    // `export function cliRefusal(` signature, and the single call inside it
    // that hands its `code` parameter to the class. Neither mints a code, and
    // both are recognized below for what they are.
    let declares_helper = source.contains("export function cliRefusal(");
    let table = literal_code_table(source);
    for mint in APPLICATION_MINTS {
        let mut from = 0;
        while let Some(at) = source[from..].find(mint) {
            let start = from + at;
            let arguments = start + mint.len();
            from = arguments;

            // The declaration matches the needle now that the helper's own
            // module is read. Skip it for being a declaration, rather than
            // letting it fall through and trusting the charset filter to
            // discard whatever `refusalClass: CliRefusalClass` looks like.
            if source[..start].trim_end().ends_with("function") {
                continue;
            }

            let tail = &source[arguments..];
            // Bound the window on a char boundary, as the Rust scan does:
            // these sources carry em dashes and a byte cut lands inside one.
            let window = &tail[..char_boundary(tail, 300)];
            let Some(comma) = window.find(',') else {
                unreadable.push(unreadable_site(
                    file,
                    source,
                    start,
                    "no code argument follows the class",
                ));
                continue;
            };
            let argument = window[comma + 1..].trim_start();

            // The helper handing on its own parameter forwards a mint, it does
            // not make one — and only in the module that declares it.
            if declares_helper && identifier(argument) == "code" {
                continue;
            }

            let Some(quote) = argument
                .chars()
                .next()
                .filter(|character| *character == '\'' || *character == '"')
            else {
                // A mint may forward a code from the file's own identity
                // table — `design-collab.ts` turns one refused envelope into a
                // class, code and remedy and hands that to `cliRefusal`. That
                // is a forward, not a hidden code: every code the call can
                // carry is a literal already read from this same file. What
                // stays a defect is a mint that forwards from somewhere this
                // scan cannot see, because then nothing here knows what
                // crosses the bridge.
                if !table.is_empty() {
                    continue;
                }
                unreadable.push(unreadable_site(
                    file,
                    source,
                    start,
                    "the code argument is not a plain string literal, and \
                     this file states no `code:` literals to read it from",
                ));
                continue;
            };
            let after = &argument[quote.len_utf8()..];
            let Some(close) = after.find(quote) else {
                unreadable.push(unreadable_site(
                    file,
                    source,
                    start,
                    "the code literal does not close within the scanned window",
                ));
                continue;
            };
            let code = &after[..close];
            if is_bridge_code(code) {
                codes.insert(code.to_string());
            } else {
                unreadable.push(unreadable_site(
                    file,
                    source,
                    start,
                    &format!(
                        "`{code}` is not a code the bridge will carry \
                         (1..=80 bytes of [a-z0-9_]), so this refusal would \
                         arrive as `desktop_refused`"
                    ),
                ));
            }
        }
    }
    codes.extend(table);
    (codes, unreadable)
}

/// Every code the application can hand the bridge — read from its mint sites
/// and from the identity tables those sites forward — with the sites whose
/// code this scan could not read.
///
/// The second return value is the point: a call the scan cannot read is a call
/// it cannot check, and quietly stepping over those is what would let this test
/// pass while saying nothing. They are reported, not skipped.
fn application_refusal_codes(root: &Path) -> (BTreeSet<String>, Vec<String>) {
    let mut codes = BTreeSet::new();
    let mut unreadable = Vec::new();
    let mut files = Vec::new();
    typescript_files(&root.join("src"), &mut files);
    for file in files {
        // A test may construct a refusal to assert the bridge's own behaviour;
        // that is not a code the product can emit.
        if file
            .to_str()
            .is_some_and(|path| path.contains(".test.") || path.contains("/tests/"))
        {
            continue;
        }
        let source = std::fs::read_to_string(&file).expect("read application source");
        let (found, unread) = application_codes_in_source(&file, &source);
        codes.extend(found);
        unreadable.extend(unread);
    }
    (codes, unreadable)
}

/// `.ts` and `.svelte`: a `<script lang="ts">` block is TypeScript too, and a
/// refusal thrown from a component was invisible while this walked only `.ts`.
fn typescript_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            typescript_files(&path, out);
        } else if path
            .extension()
            .is_some_and(|extension| extension == "ts" || extension == "svelte")
        {
            out.push(path);
        }
    }
}

/// The application scan's own negative control.
///
/// Over ds-web this scan has almost nothing to prove itself on: `new
/// CliOperationRefusal` is written exactly once and that once is the helper
/// forwarding its own parameter, no code in the application carries a digit, no
/// component mints, and no mint spells a code the bridge would refuse. A pass
/// over the live tree is therefore equally consistent with a scan that reads
/// none of those shapes — the same silence
/// `codes_in_source_reads_every_constructor_shape` guards the Rust half from.
/// So the shapes are stated here and read directly, the unreadable ones
/// included: what is under test is as much that a mint this scan cannot read is
/// REPORTED as that one it can read is found.
#[test]
fn application_codes_in_source_reads_every_mint_shape() {
    // A module that mints: literals at the call, a digit in a code, and the
    // class written out instead of going through the helper.
    const MINTS: &str = r#"
export function locked(): never {
	throw cliRefusal('conflict', 'design_locked_by_another_session', 'someone else holds it');
}

export function tooDeep(zoom: number): never {
	throw cliRefusal(
		'invalid_input',
		'tile_zoom_beyond_z22',
		`zoom ${zoom} is past the deepest tile this project builds`,
	);
}

export function noBridge(): never {
	throw new CliOperationRefusal('unavailable', 'desktop_bridge_absent', 'no paired session');
}

export function relayed(error: WireError): never {
	throw cliRefusal('failed', error.code, 'the engine refused');
}
"#;

    let (codes, unreadable) = application_codes_in_source(Path::new("mints.ts"), MINTS);
    let expected: BTreeSet<String> = [
        "design_locked_by_another_session",
        "desktop_bridge_absent",
        "tile_zoom_beyond_z22",
    ]
    .iter()
    .map(|code| code.to_string())
    .collect();
    assert_eq!(codes, expected);
    // The relay is the one call whose code is not written anywhere this scan
    // reaches, and a mint it cannot read is a mint it cannot check. The line is
    // the mint's own — a report that cannot be walked to is one nobody acts on.
    assert_eq!(unreadable.len(), 1, "unreadable: {unreadable:?}");
    assert!(
        unreadable[0].contains("mints.ts:19")
            && unreadable[0].contains("not a plain string literal"),
        "an unreadable mint must name its file, its offset and why: {}",
        unreadable[0]
    );

    // A code the bridge would not carry is reported rather than counted:
    // `structured_desktop_refusal` takes 1..=80 bytes of `[a-z0-9_]` and falls
    // back to `desktop_refused` for anything else, so a mint spelling one is a
    // refusal that arrives at the caller with no identity at all.
    const REJECTED: &str = r#"
export function shout(): never {
	throw cliRefusal('failed', 'Design-Failed', 'the engine refused');
}
"#;

    let (codes, unreadable) = application_codes_in_source(Path::new("shout.ts"), REJECTED);
    assert!(codes.is_empty(), "codes: {codes:?}");
    assert_eq!(unreadable.len(), 1, "unreadable: {unreadable:?}");
    assert!(
        unreadable[0].contains("`Design-Failed` is not a code the bridge will carry"),
        "a code the bridge would drop must be named, not counted: {}",
        unreadable[0]
    );

    // The module that declares the helper mints nothing: the signature is a
    // declaration and the call inside it forwards the `code` parameter.
    const HELPER: &str = r#"
export function cliRefusal(
	refusalClass: CliRefusalClass,
	code: string,
	message: string,
	remedy?: string,
): CliOperationRefusal {
	return new CliOperationRefusal(refusalClass, code, message, remedy);
}
"#;

    let (codes, unreadable) = application_codes_in_source(Path::new("cli-errors.ts"), HELPER);
    assert!(
        codes.is_empty(),
        "the helper declares, it does not mint: {codes:?}"
    );
    assert!(unreadable.is_empty(), "unreadable: {unreadable:?}");

    // A signature is a declaration whichever way its parameters are spelled.
    // `code` is not a reserved name here — the module is free to rename it — so
    // the word `function` in front of the needle is the only thing between an
    // ambient or overload signature and a report of a mint whose code cannot be
    // read. Held here because ds-web states its helper once, in the one shape
    // whose parameter *is* called `code`.
    const SIGNATURE: &str = r#"
export declare function cliRefusal(
	refusalClass: CliRefusalClass,
	refusalCode: string,
	message: string,
	remedy?: string,
): CliOperationRefusal;
"#;

    let (codes, unreadable) = application_codes_in_source(Path::new("cli-errors.d.ts"), SIGNATURE);
    assert!(
        codes.is_empty(),
        "a signature declares, it does not mint: {codes:?}"
    );
    assert!(
        unreadable.is_empty(),
        "a signature is not a mint site to report: {unreadable:?}"
    );

    // The shape this check grew for: a wire object assembled by hand states a
    // code that reaches the bridge without passing a mint at all. `offlineRefusal`
    // was exactly that, and the one code typed at a sink instead of at a throw
    // site was the one code nothing checked.
    const WIRE: &str = r#"
export function offlineRefusal(error: unknown): CliStructuredRefusal | null {
	if (!isOffline(error)) return null;
	return { class: 'unavailable', code: 'offline_mode_enabled', message: 'Offline mode is on.' };
}
"#;

    let (codes, unreadable) = application_codes_in_source(Path::new("offline.ts"), WIRE);
    let expected: BTreeSet<String> = ["offline_mode_enabled"]
        .iter()
        .map(|code| code.to_string())
        .collect();
    assert_eq!(codes, expected);
    assert!(unreadable.is_empty(), "unreadable: {unreadable:?}");

    // An identity table forwards codes it states as literals in the same file,
    // which is a forward rather than a hidden code.
    const TABLE: &str = r#"
const REFUSALS = {
	locked: { refusalClass: 'conflict', code: 'design_collab_locked', remedy: 'wait' },
	stale: { refusalClass: 'invalid_input', code: 'design_collab_stale', remedy: 'reload' },
};

export function fromEnvelope(envelope: RefusedEnvelope): never {
	const mapped = REFUSALS[envelope.reason];
	throw cliRefusal(mapped.refusalClass, mapped.code, envelope.detail, mapped.remedy);
}
"#;

    let (codes, unreadable) = application_codes_in_source(Path::new("design-collab.ts"), TABLE);
    let expected: BTreeSet<String> = ["design_collab_locked", "design_collab_stale"]
        .iter()
        .map(|code| code.to_string())
        .collect();
    assert_eq!(codes, expected);
    assert!(unreadable.is_empty(), "unreadable: {unreadable:?}");

    // `code:` is an ordinary key in this application. A file that mints nothing
    // and names no wire type is not read at all, which is what keeps the check
    // about refusals instead of about every object with a `code`.
    const UNRELATED: &str = r#"
export const settings = { code: 'metric', label: 'Settings' };
"#;

    let (codes, unreadable) = application_codes_in_source(Path::new("settings.ts"), UNRELATED);
    assert!(
        codes.is_empty(),
        "a non-minting file states no refusals: {codes:?}"
    );
    assert!(unreadable.is_empty(), "unreadable: {unreadable:?}");

    // A component's script block is TypeScript, and the refusal thrown from one
    // crosses the same bridge as any other.
    const COMPONENT: &str = r#"
<script lang="ts">
	function refuse(): never {
		throw cliRefusal('conflict', 'layer_already_open', 'the layer is already open');
	}
</script>
"#;

    let (codes, unreadable) = application_codes_in_source(Path::new("Layer.svelte"), COMPONENT);
    let expected: BTreeSet<String> = ["layer_already_open"]
        .iter()
        .map(|code| code.to_string())
        .collect();
    assert_eq!(codes, expected);
    assert!(unreadable.is_empty(), "unreadable: {unreadable:?}");
}

#[test]
fn every_application_refusal_code_is_documented() {
    let Some(root) = ds_web() else {
        eprintln!(
            "SKIPPED: this check proves every refusal DS GridDesign can send \
             across the bridge is one `ds` documents.\n  Set DS_WEB_DIR to the \
             ds-web checkout to run it."
        );
        return;
    };
    // The walk has to reach components: a refusal thrown from a
    // `<script lang="ts">` block crosses the same bridge, and ds-web has none
    // today, so nothing but this says the walk could still see one.
    let mut walked = Vec::new();
    typescript_files(&root.join("src"), &mut walked);
    assert!(
        walked.iter().any(|file| file
            .extension()
            .is_some_and(|extension| extension == "svelte")),
        "no `.svelte` file was walked under {}, so a refusal thrown from a \
         component would be invisible to this check",
        root.display()
    );

    let (codes, unreadable) = application_refusal_codes(&root);
    assert!(
        unreadable.is_empty(),
        "these refusal mint sites do not state a code this check can read:\n{}\n\n\
         A code the scan cannot read is a code it cannot check, and stepping \
         over such a site quietly is what would make a pass here mean nothing. \
         State the code as a plain string literal at the call site: a variable, \
         a template literal or a spread hides it from this test exactly as it \
         hides it from a reader of the command's REFUSALS.",
        unreadable.join("\n")
    );
    assert!(
        !codes.is_empty(),
        "no `cliRefusal` or `new CliOperationRefusal` call sites were found in \
         {}. The scan is matching nothing, which would make this check vacuous \
         — confirm the helper is still named `cliRefusal` before trusting a \
         pass.",
        root.display()
    );

    let (_, all_declared) = declared_codes();
    let undocumented: Vec<String> = codes
        .iter()
        .filter(|code| !all_declared.contains(*code))
        .map(|code| format!("  `{code}`"))
        .collect();

    assert!(
        undocumented.is_empty(),
        "DS GridDesign can refuse with these codes, but no `ds` command \
         declares them:\n{}\n\n\
         The bridge now preserves an application refusal's own class, code and \
         remedy for every operation, so an undeclared code would reach a \
         caller that cannot look it up. Add each to the REFUSALS of the \
         command whose operation raises it.",
        undocumented.join("\n")
    );
}
