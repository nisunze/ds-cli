//! Windows PowerShell 5.1 on one entry of the embedded driver bundle.
//!
//! This is the desktop verbs' only process boundary, and it is pinned in
//! `crates/ds/tests/process_boundary.rs`. Its shape is the rule every owner
//! call in `ds` keeps: the interpreter is found under `%SystemRoot%`, never on
//! `PATH`; its switches are literals; the script is one [`Entry`] of a closed
//! enum, extracted from the bundle and verified a moment earlier; parameter
//! names are `&'static str`; and every value is a path, digest, label or
//! number the verb has already validated. No caller string reaches the
//! command line except as one quoted value.
//!
//! The command line is built here, not by the standard library: on Windows
//! each argument is quoted by [`quote`] and passed with `raw_arg`, so the
//! quoting a test proves is the quoting PowerShell receives. Output goes to
//! files in the run's private folder rather than pipes — a run lasts hours
//! and its drivers log freely — and only a bounded tail is ever read back.

use std::fs::File;
use std::io::{Read as _, Seek as _, SeekFrom};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use ds_cli_contract::outcome::Failure;
use serde_json::{Value, json};

use super::bundle::{self, Entry, Extracted};
use super::{BUNDLE_FAILED, RUN_TIMED_OUT};

/// The literal switches every run starts with. `-ExecutionPolicy Bypass`
/// applies to this process only; the scripts are ds's own, just verified.
pub(crate) const SWITCHES: &[&str] = &[
    "-NoLogo",
    "-NoProfile",
    "-NonInteractive",
    "-ExecutionPolicy",
    "Bypass",
    "-File",
];

/// The one document an entry writes. It lives in the run's private folder.
const RESULT_FILE: &str = "ds-result.json";
/// Larger than any result document; beyond this the run is misbehaving.
const MAX_RESULT_BYTES: u64 = 4 * 1024 * 1024;
const TAIL_BYTES: u64 = 8 * 1024;
const POLL: Duration = Duration::from_millis(500);

/// Windows PowerShell 5.1, where Windows installs it. `PATH` is not
/// consulted: `pwsh` 7 or anything else named `powershell` there is not the
/// runtime the drivers were proven on.
pub(crate) fn powershell() -> PathBuf {
    let root = std::env::var_os("SystemRoot").unwrap_or_else(|| r"C:\Windows".into());
    Path::new(&root)
        .join("System32")
        .join("WindowsPowerShell")
        .join("v1.0")
        .join("powershell.exe")
}

/// One run: which entry, with which typed parameters, bounded by how long.
pub(crate) struct Invocation {
    pub(crate) entry: Entry,
    pub(crate) params: Vec<(&'static str, Option<String>)>,
    pub(crate) timeout: Duration,
}

impl Invocation {
    pub(crate) fn new(entry: Entry, timeout: Duration) -> Self {
        Self {
            entry,
            params: Vec::new(),
            timeout,
        }
    }

    /// `-Name value`.
    pub(crate) fn value(mut self, name: &'static str, value: impl Into<String>) -> Self {
        self.params.push((name, Some(value.into())));
        self
    }

    pub(crate) fn optional(self, name: &'static str, value: Option<String>) -> Self {
        match value {
            Some(value) => self.value(name, value),
            None => self,
        }
    }

    /// `-Name` alone, when `on`.
    pub(crate) fn switch(mut self, name: &'static str, on: bool) -> Self {
        if on {
            self.params.push((name, None));
        }
        self
    }
}

/// The argument vector after the interpreter, in order.
pub(crate) fn arguments(invocation: &Invocation, script: &Path, result: &Path) -> Vec<String> {
    let mut arguments: Vec<String> = SWITCHES.iter().map(|switch| switch.to_string()).collect();
    arguments.push(script.display().to_string());
    arguments.push("-ResultPath".to_string());
    arguments.push(result.display().to_string());
    for (name, value) in &invocation.params {
        arguments.push(format!("-{name}"));
        if let Some(value) = value {
            arguments.push(value.clone());
        }
    }
    arguments
}

/// Quote one argument for the Microsoft C runtime's command-line parser,
/// which `powershell.exe` uses: backslashes are literal except before a
/// quote, where they are doubled; a quote is escaped; an argument holding
/// whitespace or a quote, or none at all, is wrapped in quotes — with any
/// trailing backslashes doubled so they do not escape the closing one.
pub(crate) fn quote(argument: &str) -> String {
    if !argument.is_empty() && !argument.contains([' ', '\t', '\n', '\u{b}', '"']) {
        return argument.to_string();
    }
    let mut quoted = String::from('"');
    let mut backslashes = 0usize;
    for character in argument.chars() {
        match character {
            '\\' => backslashes += 1,
            '"' => {
                quoted.push_str(&"\\".repeat(backslashes * 2 + 1));
                quoted.push('"');
                backslashes = 0;
            }
            other => {
                quoted.push_str(&"\\".repeat(backslashes));
                quoted.push(other);
                backslashes = 0;
            }
        }
    }
    quoted.push_str(&"\\".repeat(backslashes * 2));
    quoted.push('"');
    quoted
}

/// The command line the interpreter receives after its own path.
pub(crate) fn command_line(arguments: &[String]) -> String {
    arguments
        .iter()
        .map(|argument| quote(argument))
        .collect::<Vec<_>>()
        .join(" ")
}

/// How a run ended, before its document is interpreted.
pub(crate) struct Finished {
    /// What the interpreter was given, as it received it: diagnosis for a
    /// run that left no document.
    pub(crate) command_line: String,
    pub(crate) exit_code: Option<i32>,
    pub(crate) document: Option<Value>,
    pub(crate) stdout_tail: String,
    pub(crate) stderr_tail: String,
}

/// Run one invocation with Windows PowerShell 5.1.
pub(crate) fn execute(invocation: &Invocation) -> Result<Finished, Failure> {
    execute_with(&powershell(), &std::env::temp_dir(), invocation)
}

/// Run one invocation with the given interpreter, extracting the bundle
/// below `temp`. The interpreter is a parameter only so the whole boundary —
/// extraction, arguments, the wait, the document — can be exercised off
/// Windows with a stand-in; the verbs always pass [`powershell`].
pub(crate) fn execute_with(
    interpreter: &Path,
    temp: &Path,
    invocation: &Invocation,
) -> Result<Finished, Failure> {
    let bundle = bundle::extract(temp)?;
    let result = bundle.root().join(RESULT_FILE);
    let stdout_path = bundle.root().join("ds-stdout.txt");
    let stderr_path = bundle.root().join("ds-stderr.txt");
    let script = bundle.path(invocation.entry.file());
    let arguments = arguments(invocation, &script, &result);

    let mut child = spawn(interpreter, &arguments, &stdout_path, &stderr_path)?;
    let exit_code = wait(&mut child, invocation.timeout, &bundle)?;
    Ok(Finished {
        command_line: command_line(&arguments),
        exit_code,
        document: read_document(&result),
        stdout_tail: tail(&stdout_path),
        stderr_tail: tail(&stderr_path),
    })
}

fn spawn(
    interpreter: &Path,
    arguments: &[String],
    stdout: &Path,
    stderr: &Path,
) -> Result<Child, Failure> {
    let open = |path: &Path| {
        File::create(path).map_err(|error| {
            Failure::failed(
                BUNDLE_FAILED.code,
                format!("could not create {}", path.display()),
            )
            .remedy(BUNDLE_FAILED.remedy)
            .detail(json!({ "detail": error.kind().to_string() }))
        })
    };
    let mut command = Command::new(interpreter);
    command
        .stdin(Stdio::null())
        .stdout(open(stdout)?)
        .stderr(open(stderr)?);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt as _;
        for argument in arguments {
            command.raw_arg(quote(argument));
        }
    }
    #[cfg(not(windows))]
    command.args(arguments);
    command.spawn().map_err(|error| {
        Failure::unavailable(
            super::POWERSHELL_NOT_FOUND.code,
            format!("{} could not be started", interpreter.display()),
        )
        .remedy(super::POWERSHELL_NOT_FOUND.remedy)
        .detail(json!({ "detail": error.kind().to_string() }))
    })
}

/// Wait for the run, or stop waiting at its bound. On the bound PowerShell is
/// killed; PLS-CADD, started by the drivers as its own process, is left for
/// the operator to inspect — closing it blind could discard the evidence.
fn wait(child: &mut Child, timeout: Duration, bundle: &Extracted) -> Result<Option<i32>, Failure> {
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Ok(status.code()),
            Ok(None) if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(Failure::failed(
                    RUN_TIMED_OUT.code,
                    format!("the run did not finish within {}s", timeout.as_secs()),
                )
                .remedy(RUN_TIMED_OUT.remedy)
                .detail(json!({
                    "timeout_s": timeout.as_secs(),
                    "stderr": tail(&bundle.root().join("ds-stderr.txt")),
                })));
            }
            Ok(None) => std::thread::sleep(POLL),
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(
                    Failure::failed(RUN_TIMED_OUT.code, "the run could not be waited for")
                        .remedy(RUN_TIMED_OUT.remedy)
                        .detail(json!({ "detail": error.kind().to_string() })),
                );
            }
        }
    }
}

fn read_document(path: &Path) -> Option<Value> {
    let file = File::open(path).ok()?;
    if file.metadata().ok()?.len() > MAX_RESULT_BYTES {
        return None;
    }
    let mut bytes = Vec::new();
    file.take(MAX_RESULT_BYTES).read_to_end(&mut bytes).ok()?;
    super::parse_document(&bytes)
}

/// The last lines of a console stream: a gist for a human, never parsed.
/// Windows PowerShell writes redirected output in the console code page, so
/// the bytes are read lossily.
fn tail(path: &Path) -> String {
    let Ok(mut file) = File::open(path) else {
        return String::new();
    };
    let length = file.metadata().map(|meta| meta.len()).unwrap_or(0);
    let _ = file.seek(SeekFrom::Start(length.saturating_sub(TAIL_BYTES)));
    let mut bytes = Vec::new();
    let _ = file.read_to_end(&mut bytes);
    let text = String::from_utf8_lossy(&bytes);
    let lines: Vec<&str> = text
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect();
    lines[lines.len().saturating_sub(12)..]
        .iter()
        .map(|line| line.chars().take(300).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// The parameter names an entry script declares in its `param(...)` block.
    /// A verb that passes a name its entry does not declare makes PowerShell
    /// refuse the whole run on the desktop; each verb's tests hold its
    /// invocation to this list.
    pub(crate) fn declared_parameters(entry: Entry) -> Vec<String> {
        let text = bundle::text(entry.file()).expect("an entry is embedded");
        let start = text.find("param(").expect("a param block");
        let mut depth = 0usize;
        let mut end = start;
        for (offset, character) in text[start..].char_indices() {
            match character {
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        end = start + offset;
                        break;
                    }
                }
                _ => {}
            }
        }
        let block = &text[start..end];
        block
            .match_indices('$')
            .map(|(at, _)| {
                block[at + 1..]
                    .chars()
                    .take_while(|c| c.is_ascii_alphanumeric())
                    .collect::<String>()
            })
            .filter(|name| !name.is_empty() && name != "true" && name != "false")
            .collect()
    }

    /// The Microsoft C runtime's parser (`CommandLineToArgvW` rules), written
    /// out so quoting can be proven by a round trip rather than by eye.
    fn parse_command_line(line: &str) -> Vec<String> {
        let chars: Vec<char> = line.chars().collect();
        let mut arguments = Vec::new();
        let mut at = 0;
        while at < chars.len() {
            while at < chars.len() && (chars[at] == ' ' || chars[at] == '\t') {
                at += 1;
            }
            if at >= chars.len() {
                break;
            }
            let mut argument = String::new();
            let mut quoted = false;
            while at < chars.len() && (quoted || (chars[at] != ' ' && chars[at] != '\t')) {
                let mut backslashes = 0;
                while at < chars.len() && chars[at] == '\\' {
                    backslashes += 1;
                    at += 1;
                }
                if at < chars.len() && chars[at] == '"' {
                    argument.push_str(&"\\".repeat(backslashes / 2));
                    if backslashes % 2 == 1 {
                        argument.push('"');
                    } else {
                        quoted = !quoted;
                    }
                    at += 1;
                } else {
                    argument.push_str(&"\\".repeat(backslashes));
                    if at < chars.len() && (quoted || (chars[at] != ' ' && chars[at] != '\t')) {
                        argument.push(chars[at]);
                        at += 1;
                    }
                }
            }
            arguments.push(argument);
        }
        arguments
    }

    #[test]
    fn quoting_round_trips_the_paths_a_delivery_uses() {
        let cases = [
            r"G:\Shared drives\Pro\TBEA\Nyamagabe Nyaruguru Project\Working\deliver-v19",
            r"G:\Shared drives\Pro\TBEA\Final Huye Gisagara.bak",
            r"G:\folder with trailing backslash\",
            r"C:\Users\ops\AppData\Local\Temp\ds-pls-desktop-1-2-0\ds-desktop-deliver.ps1",
            r"\\server\share\run 1",
            "a\"quote",
            r#"ends\with\quote\""#,
            "Kigali-Rev_A.2",
            "100.5",
            "c10e63656a3fa03c96be8c04800512bcce1ec4020f5f44e2ce7fa8894401bb81",
            "",
            "tab\tinside",
            "unicode é — path",
        ];
        let arguments: Vec<String> = cases.iter().map(|case| case.to_string()).collect();
        assert_eq!(parse_command_line(&command_line(&arguments)), arguments);
    }

    #[test]
    fn quoting_leaves_plain_tokens_alone_and_protects_trailing_backslashes() {
        assert_eq!(quote("-NoProfile"), "-NoProfile");
        assert_eq!(quote(r"G:\run"), r"G:\run");
        assert_eq!(quote(r"G:\a b"), r#""G:\a b""#);
        assert_eq!(quote(r"G:\a b\"), r#""G:\a b\\""#);
        assert_eq!(quote(""), r#""""#);
        assert_eq!(quote(r#"say "hi""#), r#""say \"hi\"""#);
    }

    #[test]
    fn arguments_are_the_literal_switches_then_typed_named_values() {
        let invocation = Invocation::new(Entry::Deliver, Duration::from_secs(1))
            .value("BackupPath", r"G:\Shared drives\in\cap6.bak")
            .value("AlignmentGap", "100")
            .optional("SourceRoot", None)
            .optional("Label", Some("cap6".to_string()))
            .switch("NoSheets", true)
            .switch("Execute", false);
        let arguments = arguments(
            &invocation,
            Path::new(r"T:\tmp\run\ds-desktop-deliver.ps1"),
            Path::new(r"T:\tmp\run\ds-result.json"),
        );
        assert_eq!(
            arguments,
            [
                "-NoLogo",
                "-NoProfile",
                "-NonInteractive",
                "-ExecutionPolicy",
                "Bypass",
                "-File",
                r"T:\tmp\run\ds-desktop-deliver.ps1",
                "-ResultPath",
                r"T:\tmp\run\ds-result.json",
                "-BackupPath",
                r"G:\Shared drives\in\cap6.bak",
                "-AlignmentGap",
                "100",
                "-Label",
                "cap6",
                "-NoSheets",
            ]
        );
        assert_eq!(
            command_line(&arguments),
            r#"-NoLogo -NoProfile -NonInteractive -ExecutionPolicy Bypass -File T:\tmp\run\ds-desktop-deliver.ps1 -ResultPath T:\tmp\run\ds-result.json -BackupPath "G:\Shared drives\in\cap6.bak" -AlignmentGap 100 -Label cap6 -NoSheets"#
        );
    }

    #[test]
    fn powershell_is_found_under_system_root_not_path() {
        let path = powershell();
        assert!(
            path.ends_with(
                Path::new("System32")
                    .join("WindowsPowerShell")
                    .join("v1.0")
                    .join("powershell.exe")
            )
        );
    }

    #[cfg(unix)]
    mod stand_in {
        use std::os::unix::fs::PermissionsExt as _;

        use super::*;

        /// A stand-in interpreter: records its arguments and the script it
        /// was handed, then writes `document` where `-ResultPath` says.
        fn interpreter(root: &Path, document: &str, exit: i32, sleep: u32) -> PathBuf {
            std::fs::create_dir_all(root).unwrap();
            let path = root.join("powershell-stand-in");
            let record = root.join("argv.txt");
            let body = format!(
                "#!/bin/sh\n\
                 for a in \"$@\"; do printf '%s\\n' \"$a\"; done > '{record}'\n\
                 result=''; script=''\n\
                 while [ $# -gt 0 ]; do case \"$1\" in\n\
                   -File) script=\"$2\"; shift 2;;\n\
                   -ResultPath) result=\"$2\"; shift 2;;\n\
                   *) shift;;\n\
                 esac; done\n\
                 [ -f \"$script\" ] || exit 9\n\
                 echo \"running $script\"\n\
                 echo 'a driver line on stderr' >&2\n\
                 sleep {sleep}\n\
                 printf '%s' '{document}' > \"$result\"\n\
                 exit {exit}\n",
                record = record.display(),
            );
            std::fs::write(&path, body).unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
            path
        }

        fn scratch(label: &str) -> PathBuf {
            let root =
                std::env::temp_dir().join(format!("ds-cli-pls-run-{label}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&root);
            root
        }

        #[test]
        fn a_run_extracts_passes_typed_values_reads_its_document_and_cleans_up() {
            let root = scratch("ok");
            let stand_in = interpreter(
                &root,
                r#"{"schema":"ds.pls.desktop_entry.v1","verb":"deliver","status":"ok","result":{"receipt":"G:\\run\\deliver.json"}}"#,
                0,
                0,
            );
            let temp = root.join("temp");
            std::fs::create_dir_all(&temp).unwrap();
            let invocation = Invocation::new(Entry::Deliver, Duration::from_secs(20))
                .value("BackupPath", "/drive/in put/cap6.bak")
                .value("Label", "cap6")
                .switch("NoSheets", true);
            let finished = execute_with(&stand_in, &temp, &invocation).expect("the run completes");

            assert_eq!(finished.exit_code, Some(0));
            assert_eq!(
                finished.document.unwrap()["result"]["receipt"],
                r"G:\run\deliver.json"
            );
            assert!(finished.stdout_tail.contains("ds-desktop-deliver.ps1"));
            assert_eq!(finished.stderr_tail, "a driver line on stderr");

            let argv = std::fs::read_to_string(root.join("argv.txt")).unwrap();
            let argv: Vec<&str> = argv.lines().collect();
            assert_eq!(&argv[..6], SWITCHES);
            assert!(argv[6].ends_with("ds-desktop-deliver.ps1"));
            assert_eq!(argv[7], "-ResultPath");
            assert_eq!(
                &argv[9..],
                [
                    "-BackupPath",
                    "/drive/in put/cap6.bak",
                    "-Label",
                    "cap6",
                    "-NoSheets"
                ]
            );
            let run_folder = Path::new(argv[6]).parent().unwrap();
            assert!(run_folder.starts_with(&temp));
            assert!(
                !run_folder.exists(),
                "the extracted drivers are removed with the run"
            );
            std::fs::remove_dir_all(root).unwrap();
        }

        #[test]
        fn a_run_past_its_bound_is_stopped_and_named() {
            let root = scratch("timeout");
            let stand_in = interpreter(&root, "{}", 0, 30);
            let temp = root.join("temp");
            std::fs::create_dir_all(&temp).unwrap();
            let started = Instant::now();
            let refusal = execute_with(
                &stand_in,
                &temp,
                &Invocation::new(Entry::Check, Duration::from_millis(600)),
            )
            .err()
            .expect("the bound stops the run");
            assert_eq!(refusal.code(), RUN_TIMED_OUT.code);
            assert!(started.elapsed() < Duration::from_secs(10));
            assert_eq!(
                std::fs::read_dir(&temp).unwrap().count(),
                0,
                "the extracted drivers are removed after a timeout too"
            );
            std::fs::remove_dir_all(root).unwrap();
        }

        #[test]
        fn a_run_without_a_document_reports_none_rather_than_guessing() {
            let root = scratch("nodoc");
            std::fs::create_dir_all(&root).unwrap();
            let path = root.join("crashing-stand-in");
            std::fs::write(&path, "#!/bin/sh\necho 'ParserError: boom' >&2\nexit 1\n").unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
            let temp = root.join("temp");
            std::fs::create_dir_all(&temp).unwrap();
            let finished = execute_with(
                &path,
                &temp,
                &Invocation::new(Entry::Check, Duration::from_secs(20)),
            )
            .unwrap();
            assert_eq!(finished.exit_code, Some(1));
            assert!(finished.document.is_none());
            assert_eq!(finished.stderr_tail, "ParserError: boom");
            std::fs::remove_dir_all(root).unwrap();
        }
    }
}
