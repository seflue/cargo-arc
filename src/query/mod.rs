//! The output of `cargo arc query`: header lines that say what the data
//! describes, then one record per line, as TSV or as JSON Lines. The format
//! is described in `docs/QUERY.md`.

pub(crate) mod edges;
pub(crate) mod hotspots;
pub(crate) mod usages;

use serde_json::{Map, Value};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

/// Raise it when a column or a header field is added, removed, or changes its
/// meaning, so a reader can tell which output it parses.
pub const FORMAT_VERSION: u32 = 1;

/// What the records describe: which cargo-arc wrote them in which format,
/// from which workspace at which commit, under which flags, and what the
/// subcommand counts.
#[derive(Debug, Clone, PartialEq)]
pub struct Header {
    pub cargo_arc_version: String,
    pub format_version: u32,
    pub workspace_root: PathBuf,
    /// `None` when the workspace is not in a git repository.
    pub git_commit: Option<String>,
    /// The flags that shape the data, by their long name without dashes.
    pub flags: Map<String, Value>,
    /// The fields the subcommand adds after the flags, in output order: its
    /// selection, counting rules and window.
    pub subcommand_fields: Vec<(&'static str, Value)>,
}

impl Header {
    /// Describe the data of `workspace_root` under `flags`, as written by
    /// this build of cargo-arc in the current format.
    #[must_use]
    pub fn for_workspace(workspace_root: &Path, flags: Map<String, Value>) -> Self {
        Self {
            cargo_arc_version: env!("CARGO_PKG_VERSION").to_string(),
            format_version: FORMAT_VERSION,
            workspace_root: workspace_root.to_path_buf(),
            git_commit: head_commit(workspace_root),
            flags,
            subcommand_fields: Vec::new(),
        }
    }

    /// Return the header fields in output order, keyed as both formats print
    /// them, so the TSV and the JSON header cannot drift apart.
    fn fields(&self) -> Vec<(&'static str, Value)> {
        let mut fields = vec![
            (
                "cargo-arc-version",
                Value::from(self.cargo_arc_version.as_str()),
            ),
            ("format-version", Value::from(self.format_version)),
            (
                "workspace-root",
                Value::from(self.workspace_root.display().to_string()),
            ),
            (
                "git-commit",
                self.git_commit.as_deref().map_or(Value::Null, Value::from),
            ),
            ("flags", Value::Object(self.flags.clone())),
        ];
        fields.extend(self.subcommand_fields.iter().cloned());
        fields
    }
}

/// Return the commit `HEAD` names in the repository holding `dir`, suffixed
/// `-dirty` when a tracked file differs from it, as `git describe --dirty`
/// marks it.
/// `None` outside a repository, in one without commits, and where git cannot
/// run: the header then says there is no commit instead of failing the query.
fn head_commit(dir: &Path) -> Option<String> {
    let commit = git_stdout(dir, &["rev-parse", "--verify", "--quiet", "HEAD"])?;
    let changes = git_stdout(dir, &["status", "--porcelain", "--untracked-files=no"])?;
    if changes.is_empty() {
        Some(commit)
    } else {
        Some(format!("{commit}-dirty"))
    }
}

/// Return the trimmed stdout of `git args` in `dir`; `None` when git fails.
fn git_stdout(dir: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8(output.stdout).ok()?;
    Some(text.trim().to_string())
}

/// Write `records` as TSV: a `# key: value` line per header field, the line
/// of `columns`, then one tab-separated line per record.
///
/// # Errors
/// Fails when `out` fails.
pub fn write_tsv(
    out: &mut impl Write,
    header: &Header,
    columns: &[&str],
    records: &[Vec<Value>],
) -> io::Result<()> {
    for (key, value) in header.fields() {
        writeln!(out, "# {key}: {}", tsv_cell(&value))?;
    }
    writeln!(out, "{}", columns.join("\t"))?;
    for record in records {
        let cells: Vec<String> = record.iter().map(tsv_cell).collect();
        writeln!(out, "{}", cells.join("\t"))?;
    }
    Ok(())
}

/// Write `records` as JSON Lines: one object with the header fields and
/// `columns`, then one object per record keyed by `columns`. Both are
/// written key by key, so the keys keep the order of the TSV header and of
/// `columns`; a `serde_json::Map` would sort them.
///
/// # Errors
/// Fails when `out` fails.
pub fn write_json_lines(
    out: &mut impl Write,
    header: &Header,
    columns: &[&str],
    records: &[Vec<Value>],
) -> io::Result<()> {
    let mut head = header.fields();
    head.push(("columns", Value::from(columns)));
    writeln!(out, "{}", json_object(head.iter().map(|(k, v)| (*k, v))))?;
    for record in records {
        writeln!(out, "{}", json_object(columns.iter().copied().zip(record)))?;
    }
    Ok(())
}

/// Render `entries` as one JSON object in their order.
fn json_object<'a>(entries: impl Iterator<Item = (&'a str, &'a Value)>) -> String {
    let members: Vec<String> = entries
        .map(|(key, value)| format!("{}:{value}", Value::from(key)))
        .collect();
    format!("{{{}}}", members.join(","))
}

/// Render `value` as TSV text: a string as it is, a missing value as `none`,
/// a list comma-joined, and an object as space-separated `key=value` pairs.
/// A backslash, tab, newline or carriage return in the text is written as
/// `\\`, `\t`, `\n` or `\r`, so no field can split its line.
fn tsv_cell(value: &Value) -> String {
    let text = match value {
        Value::Null => "none".to_string(),
        Value::String(text) => text.clone(),
        Value::Bool(_) | Value::Number(_) => value.to_string(),
        Value::Array(items) => items.iter().map(tsv_cell).collect::<Vec<_>>().join(","),
        Value::Object(entries) => entries
            .iter()
            .map(|(key, value)| format!("{key}={}", tsv_cell(value)))
            .collect::<Vec<_>>()
            .join(" "),
    };
    escape_tsv(&text)
}

fn escape_tsv(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '\\' => escaped.push_str("\\\\"),
            '\t' => escaped.push_str("\\t"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            other => escaped.push(other),
        }
    }
    escaped
}

/// Format the Unix timestamp `seconds` as its UTC date, `YYYY-MM-DD`.
#[must_use]
pub(super) fn utc_date(seconds: i64) -> String {
    // Howard Hinnant's civil_from_days: days since 1970-01-01 to a
    // proleptic Gregorian date, counted in 400-year eras from 0000-03-01.
    let days = seconds.div_euclid(86_400) + 719_468;
    let era = days.div_euclid(146_097);
    let day_of_era = days.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_index + 2) / 5 + 1;
    let month = if month_index < 10 {
        month_index + 3
    } else {
        month_index - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::git;
    use serde_json::{Value, json};

    fn header() -> Header {
        let mut flags = serde_json::Map::new();
        flags.insert("include-tests".into(), json!(true));
        flags.insert("features".into(), json!(["a", "b"]));
        Header {
            cargo_arc_version: "1.2.3".into(),
            format_version: FORMAT_VERSION,
            workspace_root: PathBuf::from("/ws"),
            git_commit: None,
            flags,
            subcommand_fields: vec![("top", json!(5)), ("ranking", json!("by size"))],
        }
    }

    #[test]
    fn header_of_a_workspace_in_git_names_its_head_commit() {
        let repo = tempfile::tempdir().unwrap();
        git(repo.path(), &["init", "-q"]);
        std::fs::write(repo.path().join("Cargo.toml"), "").unwrap();
        git(repo.path(), &["add", "."]);
        git(repo.path(), &["commit", "-q", "-m", "c"]);
        let head = git(repo.path(), &["rev-parse", "HEAD"]);

        let header = Header::for_workspace(repo.path(), serde_json::Map::new());

        assert_eq!(header.git_commit, Some(head));
        assert_eq!(header.workspace_root, repo.path());
        assert_eq!(header.cargo_arc_version, env!("CARGO_PKG_VERSION"));
        assert_eq!(header.format_version, 1);
    }

    #[test]
    fn header_of_a_workspace_outside_git_has_no_commit() {
        let dir = tempfile::tempdir().unwrap();

        let header = Header::for_workspace(dir.path(), serde_json::Map::new());

        assert_eq!(header.git_commit, None);
    }

    #[test]
    fn tsv_with_no_records_prints_the_header_and_the_column_line() {
        let mut out = Vec::new();
        write_tsv(&mut out, &header(), &["path", "count"], &[]).unwrap();

        assert_eq!(
            String::from_utf8(out).unwrap(),
            "# cargo-arc-version: 1.2.3\n\
             # format-version: 1\n\
             # workspace-root: /ws\n\
             # git-commit: none\n\
             # flags: features=a,b include-tests=true\n\
             # top: 5\n\
             # ranking: by size\n\
             path\tcount\n"
        );
    }

    #[test]
    fn a_tab_newline_or_backslash_in_a_field_is_escaped_in_tsv() {
        let records = [vec![json!("a\tb\nc\rd\\e"), json!(1)]];
        let mut out = Vec::new();
        write_tsv(&mut out, &header(), &["path", "count"], &records).unwrap();

        let out = String::from_utf8(out).unwrap();
        assert_eq!(out.lines().last(), Some("a\\tb\\nc\\rd\\\\e\t1"));
    }

    #[test]
    fn json_lines_parse_line_by_line_with_the_header_keys_in_tsv_order() {
        let records = [vec![json!("src/a.rs"), json!(3)]];
        let mut jsonl = Vec::new();
        write_json_lines(&mut jsonl, &header(), &["path", "count"], &records).unwrap();
        let jsonl = String::from_utf8(jsonl).unwrap();

        let lines: Vec<Value> = jsonl
            .lines()
            .map(|line| serde_json::from_str(line).expect("each line is JSON"))
            .collect();
        assert_eq!(
            lines,
            [
                json!({
                    "cargo-arc-version": "1.2.3",
                    "format-version": 1,
                    "workspace-root": "/ws",
                    "git-commit": null,
                    "flags": {"include-tests": true, "features": ["a", "b"]},
                    "top": 5,
                    "ranking": "by size",
                    "columns": ["path", "count"],
                }),
                json!({"path": "src/a.rs", "count": 3}),
            ]
        );
        assert!(
            jsonl.starts_with(
                "{\"cargo-arc-version\":\"1.2.3\",\"format-version\":1,\"workspace-root\":\"/ws\",\
                 \"git-commit\":null,\"flags\":"
            ),
            "{jsonl}"
        );
        assert!(
            jsonl.contains("\"top\":5,\"ranking\":\"by size\",\"columns\":"),
            "{jsonl}"
        );
        assert_eq!(
            jsonl.lines().nth(1),
            Some("{\"path\":\"src/a.rs\",\"count\":3}")
        );
    }

    #[rstest::rstest]
    #[case::epoch(0, "1970-01-01")]
    #[case::before_a_leap_day(951_782_399, "2000-02-28")]
    #[case::leap_day(951_782_400, "2000-02-29")]
    #[case::late_in_the_day(1_700_000_000, "2023-11-14")]
    #[case::new_year(1_704_067_200, "2024-01-01")]
    #[case::before_1970(-1, "1969-12-31")]
    fn utc_date_names_the_day_of_a_timestamp(#[case] seconds: i64, #[case] date: &str) {
        assert_eq!(utc_date(seconds), date);
    }
}
