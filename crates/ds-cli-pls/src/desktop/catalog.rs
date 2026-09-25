//! The dialog catalogue the drivers decide by, read from the embedded bundle.
//!
//! `pls-dialog-catalog.psd1` is the one record of every PLS-CADD 16.81 modal
//! the drivers have met: when it fires, how it is recognised, and the
//! decision. `pls-dialog-watch.ps1` acts on it; anything it does not match is
//! UNKNOWN, and the run stops. This module only reads it, so a caller can see
//! the decisions before a run and look one up after a refusal. It never
//! decides anything the watcher decides.
//!
//! The file is a PowerShell data file. The reader below accepts exactly the
//! subset the catalogue is written in — hashtables, arrays, quoted strings and
//! integers, with comments — and refuses anything else, so a catalogue edit
//! that steps outside it fails the bundle test instead of being misread.

use serde_json::{Map, Value, json};

use super::bundle;

pub const CATALOG_PATH: &str = "pls-dialog-catalog.psd1";

/// Every decision the watcher's `switch ($entry.Action)` implements.
pub const ACTIONS: &[&str] = &[
    "wait",
    "click",
    "click_any_ok",
    "options",
    "flow",
    "stop",
    "ignore",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DialogEntry {
    pub name: String,
    pub when: String,
    /// Case-insensitive regex on the dialog title; empty matches any.
    pub title: String,
    /// Case-insensitive regex on the dialog's static text; empty matches any.
    pub text: String,
    pub action: String,
    pub control_id: i64,
    pub note: String,
}

impl DialogEntry {
    pub fn summary(&self) -> Value {
        json!({
            "name": self.name,
            "action": self.action,
            "control_id": self.control_id,
            "when": self.when,
        })
    }

    pub fn full(&self) -> Value {
        json!({
            "name": self.name,
            "action": self.action,
            "control_id": self.control_id,
            "when": self.when,
            "title_pattern": self.title,
            "text_pattern": self.text,
            "note": self.note,
        })
    }
}

#[derive(Debug, Clone)]
pub struct Catalog {
    pub version: String,
    pub entries: Vec<DialogEntry>,
}

/// The embedded catalogue.
pub fn embedded() -> Result<Catalog, String> {
    let text = bundle::text(CATALOG_PATH).ok_or("the catalogue is not in the bundle")?;
    parse_catalog(text)
}

pub fn parse_catalog(text: &str) -> Result<Catalog, String> {
    let root = parse_data_file(text)?;
    let version = string_field(&root, "Version")?;
    let entries = root
        .get("Entries")
        .and_then(Value::as_array)
        .ok_or("Entries is not an array")?
        .iter()
        .map(|entry| {
            let entry = entry.as_object().ok_or("an entry is not a hashtable")?;
            Ok(DialogEntry {
                name: string_field(entry, "Name")?,
                when: string_field(entry, "When")?,
                title: string_field(entry, "Title")?,
                text: string_field(entry, "Text")?,
                action: string_field(entry, "Action")?,
                control_id: entry
                    .get("ControlId")
                    .and_then(Value::as_i64)
                    .ok_or("ControlId is not an integer")?,
                note: string_field(entry, "Note")?,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    Ok(Catalog { version, entries })
}

fn string_field(map: &Map<String, Value>, key: &str) -> Result<String, String> {
    map.get(key)
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| format!("`{key}` is not a string"))
}

/// Parse a PowerShell data file into JSON: `@{}` → object, `@()` → array,
/// `'…'` / `"…"` → string, digits → integer, `$true`/`$false`/`$null`.
pub fn parse_data_file(text: &str) -> Result<Map<String, Value>, String> {
    let mut reader = Reader {
        chars: text.trim_start_matches('\u{feff}').chars().collect(),
        at: 0,
    };
    reader.skip_trivia();
    let value = reader.value()?;
    reader.skip_trivia();
    if reader.at != reader.chars.len() {
        return Err(reader.error("text after the root hashtable"));
    }
    match value {
        Value::Object(map) => Ok(map),
        _ => Err("the root is not a hashtable".to_string()),
    }
}

struct Reader {
    chars: Vec<char>,
    at: usize,
}

impl Reader {
    fn peek(&self) -> Option<char> {
        self.chars.get(self.at).copied()
    }

    fn error(&self, what: &str) -> String {
        let line = self.chars[..self.at.min(self.chars.len())]
            .iter()
            .filter(|c| **c == '\n')
            .count()
            + 1;
        format!("{what} at line {line}")
    }

    /// Whitespace, newlines and `#` comments.
    fn skip_trivia(&mut self) {
        while let Some(c) = self.peek() {
            if c.is_whitespace() {
                self.at += 1;
            } else if c == '#' {
                while self.peek().is_some_and(|c| c != '\n') {
                    self.at += 1;
                }
            } else {
                break;
            }
        }
    }

    /// Trivia plus the separators PowerShell accepts between elements.
    fn skip_separators(&mut self) {
        loop {
            self.skip_trivia();
            if matches!(self.peek(), Some(';' | ',')) {
                self.at += 1;
            } else {
                break;
            }
        }
    }

    fn expect(&mut self, literal: &str) -> Result<(), String> {
        for expected in literal.chars() {
            if self.peek() != Some(expected) {
                return Err(self.error(&format!("expected `{literal}`")));
            }
            self.at += 1;
        }
        Ok(())
    }

    fn value(&mut self) -> Result<Value, String> {
        match (self.peek(), self.chars.get(self.at + 1).copied()) {
            (Some('@'), Some('{')) => self.hashtable(),
            (Some('@'), Some('(')) => self.array(),
            (Some('\''), _) => self.single_quoted().map(Value::String),
            (Some('"'), _) => self.double_quoted().map(Value::String),
            (Some('$'), _) => self.constant(),
            (Some(c), _) if c == '-' || c.is_ascii_digit() => self.integer(),
            _ => Err(self.error("unsupported value")),
        }
    }

    fn hashtable(&mut self) -> Result<Value, String> {
        self.expect("@{")?;
        let mut map = Map::new();
        loop {
            self.skip_separators();
            if self.peek() == Some('}') {
                self.at += 1;
                return Ok(Value::Object(map));
            }
            let start = self.at;
            while self
                .peek()
                .is_some_and(|c| c.is_ascii_alphanumeric() || c == '_')
            {
                self.at += 1;
            }
            if start == self.at {
                return Err(self.error("expected a key"));
            }
            let key: String = self.chars[start..self.at].iter().collect();
            self.skip_trivia();
            self.expect("=")?;
            self.skip_trivia();
            let value = self.value()?;
            if map.insert(key.clone(), value).is_some() {
                return Err(self.error(&format!("duplicate key `{key}`")));
            }
        }
    }

    fn array(&mut self) -> Result<Value, String> {
        self.expect("@(")?;
        let mut items = Vec::new();
        loop {
            self.skip_separators();
            if self.peek() == Some(')') {
                self.at += 1;
                return Ok(Value::Array(items));
            }
            items.push(self.value()?);
        }
    }

    /// `'…'`, where `''` is one quote. Nothing inside is expanded.
    fn single_quoted(&mut self) -> Result<String, String> {
        self.expect("'")?;
        let mut text = String::new();
        loop {
            match self.peek() {
                None => return Err(self.error("unterminated string")),
                Some('\'') if self.chars.get(self.at + 1) == Some(&'\'') => {
                    text.push('\'');
                    self.at += 2;
                }
                Some('\'') => {
                    self.at += 1;
                    return Ok(text);
                }
                Some(c) => {
                    text.push(c);
                    self.at += 1;
                }
            }
        }
    }

    /// `"…"` holding no expansion or escape. PowerShell would expand `$name`
    /// and read a backtick as an escape; refusing both keeps this a reader of
    /// what the file literally says rather than a second interpreter. A `$`
    /// that cannot begin a variable — a regex anchor before the closing quote
    /// or a space — is literal in PowerShell too, and is read as such.
    fn double_quoted(&mut self) -> Result<String, String> {
        self.expect("\"")?;
        let mut text = String::new();
        loop {
            match self.peek() {
                None => return Err(self.error("unterminated string")),
                Some('"') => {
                    self.at += 1;
                    return Ok(text);
                }
                Some('$')
                    if self.chars.get(self.at + 1).is_some_and(|next| {
                        *next == '"' || *next == ')' || next.is_whitespace()
                    }) =>
                {
                    text.push('$');
                    self.at += 1;
                }
                Some('$' | '`') => {
                    return Err(self.error("expansion or escape in a double-quoted string"));
                }
                Some(c) => {
                    text.push(c);
                    self.at += 1;
                }
            }
        }
    }

    fn constant(&mut self) -> Result<Value, String> {
        for (literal, value) in [
            ("$true", Value::Bool(true)),
            ("$false", Value::Bool(false)),
            ("$null", Value::Null),
        ] {
            let end = self.at + literal.len();
            if end <= self.chars.len()
                && self.chars[self.at..end]
                    .iter()
                    .collect::<String>()
                    .eq_ignore_ascii_case(literal)
            {
                self.at = end;
                return Ok(value);
            }
        }
        Err(self.error("unsupported variable"))
    }

    fn integer(&mut self) -> Result<Value, String> {
        let start = self.at;
        if self.peek() == Some('-') {
            self.at += 1;
        }
        while self.peek().is_some_and(|c| c.is_ascii_digit()) {
            self.at += 1;
        }
        let digits: String = self.chars[start..self.at].iter().collect();
        digits
            .parse::<i64>()
            .map(Value::from)
            .map_err(|_| self.error("not an integer"))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    #[test]
    fn the_embedded_catalogue_reads_completely() {
        let catalog = embedded().expect("the catalogue parses");
        assert_eq!(catalog.version, "2026-09-24");
        // Pinned: a catalogue entry is a decision, and adding one is as
        // deliberate as the new digest the bundle test then asks for.
        assert_eq!(catalog.entries.len(), 58);
        let names: BTreeSet<&str> = catalog.entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names.len(), catalog.entries.len(), "entry names are unique");
    }

    /// The decisions the drivers depend on, read back exactly. These are the
    /// catalogue's facts the verbs rely on: Exit answers "Save changes" No,
    /// AutoSag goes through the Section Table, the Repair Wizard stops a run,
    /// progress boxes are waited out, never clicked.
    #[test]
    fn the_decisions_the_verbs_rely_on_are_the_catalogue_s() {
        let catalog = embedded().unwrap();
        let entry = |name: &str| {
            catalog
                .entries
                .iter()
                .find(|entry| entry.name == name)
                .unwrap_or_else(|| panic!("{name} is catalogued"))
        };
        let save = entry("save_changes");
        assert_eq!((save.action.as_str(), save.control_id), ("click", 7));
        assert_eq!(save.title, "^PLS-CADD$");
        assert_eq!(save.text, "Save changes to");
        assert_eq!(entry("section_table").action, "flow");
        assert!(entry("section_table").note.contains("THE AutoSag ROUTE"));
        assert_eq!(entry("repair_wizard").action, "stop");
        assert_eq!(entry("progress").action, "wait");
        assert_eq!(entry("table_clipboard_paste").action, "stop");
        // `''` inside a single-quoted note is one quote, and the one
        // double-quoted text reads literally.
        assert!(
            entry("backup_done")
                .note
                .contains("'41 files backed up from 1 project'")
        );
        assert_eq!(
            entry("criteria_problem").text,
            "doesn't exist|Continue displaying warning messages"
        );
    }

    /// The watcher switches on `$entry.Action` with no default arm: an action
    /// it does not implement would be skipped silently, and the dialog left
    /// up. Every catalogued action must be one of its arms.
    #[test]
    fn every_catalogued_action_is_one_the_watcher_implements() {
        let watcher = bundle::text("pls-dialog-watch.ps1").unwrap();
        for action in ACTIONS {
            let arm = format!("'{action}'");
            assert!(
                watcher
                    .lines()
                    .any(|line| line.trim_start().starts_with(&arm)
                        && line[line.find(&arm).unwrap() + arm.len()..]
                            .trim_start()
                            .starts_with('{')),
                "the watcher has no `{arm}` arm"
            );
        }
        for entry in embedded().unwrap().entries {
            assert!(
                ACTIONS.contains(&entry.action.as_str()),
                "{} has action `{}`, which the watcher does not implement",
                entry.name,
                entry.action
            );
        }
    }

    #[test]
    fn the_profile_pins_pls_cadd_16_81() {
        let profile =
            parse_data_file(bundle::text("pls-backup-restore-profile.psd1").unwrap()).unwrap();
        assert_eq!(profile["ProductVersion"], "16.81");
        assert_eq!(
            profile["ExecutableSha256"],
            "bf5cc5c3cde126ed2119303b5530f81d80222508e2815253a29709879c858650"
        );
        assert_eq!(profile["Commands"]["Restore"], 33348);
        assert_eq!(
            profile["RepairTitlePattern"],
            "^PLS-CADD Project Repair Wizard(?: for '[^']+')?$"
        );
    }

    #[test]
    fn the_reader_refuses_what_it_would_misread() {
        assert!(parse_data_file("@{ A = \"$env:x\" }").is_err());
        assert!(parse_data_file("@{ A = \"a`nb\" }").is_err());
        assert!(parse_data_file("@{ A = 1; A = 2 }").is_err());
        assert!(parse_data_file("@{ A = Get-Date }").is_err());
        assert!(parse_data_file("@{ A = 'x' } trailing").is_err());
        assert!(parse_data_file("@{ A = 'unterminated }").is_err());
        let ok = parse_data_file("@{ # c\n A = 'it''s'; B = @(1, -2\n 3); C = $TRUE }").unwrap();
        assert_eq!(ok["A"], "it's");
        assert_eq!(ok["B"], json!([1, -2, 3]));
        assert_eq!(ok["C"], true);
    }
}
