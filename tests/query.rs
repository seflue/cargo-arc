//! `cargo arc query` run as a subprocess, so the header is checked as the
//! run builds it from the command line.

use serde_json::Value;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// The `GIT_TEST_DATE_NOW` most tests run under: 2023-11-14T22:13:20Z.
const NOW: &str = "1700000000";

/// Run `git` in `dir`, cut off from any repository the environment points
/// at, so a test never writes into another one.
fn git(dir: &Path, args: &[&str], date: &str) -> String {
    let output = Command::new("git")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .env("GIT_AUTHOR_DATE", format!("@{date} +0000"))
        .env("GIT_COMMITTER_DATE", format!("@{date} +0000"))
        .args([
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .current_dir(dir)
        .output()
        .expect("git should be on PATH");
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().to_string()
}

/// The modules of the fixture with their code lines.
const MODULES: [(&str, usize); 7] = [
    ("alpha", 8),
    ("beta", 6),
    ("gamma", 5),
    ("delta", 4),
    ("epsilon", 3),
    ("zeta", 2),
    ("quiet", 7),
];

/// Write `module` with its code lines and a comment naming `revision`; the
/// comment changes the file without changing its code lines.
fn write_module(root: &Path, module: &str, revision: usize) {
    let lines = MODULES.iter().find(|(name, _)| *name == module).unwrap().1;
    let mut text = format!("// revision {revision}\n");
    for i in 0..lines {
        writeln!(text, "pub fn f{i}() {{}}").unwrap();
    }
    std::fs::write(root.join(format!("src/{module}.rs")), text).unwrap();
}

/// Commit `modules` in their revision `revision` at `date`.
fn commit(root: &Path, modules: &[&str], revision: usize, date: &str) {
    for module in modules {
        write_module(root, module, revision);
    }
    git(root, &["add", "."], date);
    git(root, &["commit", "-q", "-m", "change"], date);
}

/// A one-package workspace in its own repository. Under `NOW` its window of
/// six months holds three commits: alpha has 8 code lines and 3 commits,
/// beta 6 and 2, gamma to zeta their lines and 1, and lib.rs and quiet.rs
/// none, so they score zero.
fn workspace() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir(root.join("src")).unwrap();
    std::fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"app\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    std::fs::write(root.join(".gitignore"), "Cargo.lock\ntarget\n").unwrap();
    let mut declarations = String::new();
    for (name, _) in MODULES {
        writeln!(declarations, "pub mod {name};").unwrap();
    }
    std::fs::write(root.join("src/lib.rs"), declarations).unwrap();

    git(root, &["init", "-q"], "1600000000");
    let all: Vec<&str> = MODULES.iter().map(|(name, _)| *name).collect();
    commit(root, &all, 0, "1600000000");
    commit(
        root,
        &["alpha", "beta", "gamma", "delta", "epsilon", "zeta"],
        1,
        "1690000000",
    );
    commit(root, &["alpha", "beta"], 2, "1695000000");
    commit(root, &["alpha"], 3, "1699000000");
    dir
}

/// Run `cargo-arc arc --manifest-path <root> <args...>` under `now`.
fn arc_at(root: &Path, now: &str, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_cargo-arc"))
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .env("GIT_TEST_DATE_NOW", now)
        .env("TZ", "UTC")
        .arg("arc")
        .arg("--manifest-path")
        .arg(root)
        .args(args)
        .output()
        .expect("cargo-arc should run")
}

fn arc(root: &Path, args: &[&str]) -> Output {
    arc_at(root, NOW, args)
}

fn stdout_of(output: &Output) -> String {
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout.clone()).unwrap()
}

/// The `# key: value` header lines of TSV `out`, in order.
fn header_of(out: &str) -> Vec<(String, String)> {
    out.lines()
        .filter_map(|line| line.strip_prefix("# "))
        .map(|line| {
            let (key, value) = line.split_once(": ").unwrap_or((line, ""));
            (key.to_string(), value.to_string())
        })
        .collect()
}

fn header_value(out: &str, key: &str) -> String {
    header_of(out)
        .into_iter()
        .find(|(k, _)| k == key)
        .unwrap_or_else(|| panic!("no header field {key} in\n{out}"))
        .1
}

/// The lines after the header: the column line, then the records.
fn body_of(out: &str) -> Vec<&str> {
    out.lines().filter(|line| !line.starts_with('#')).collect()
}

#[test]
fn the_header_names_the_workspace_of_manifest_path_and_the_flags_given() {
    let ws = workspace();
    let out = stdout_of(&arc(
        ws.path(),
        &[
            "--include-tests",
            "--volatility-months",
            "6",
            "query",
            "hotspots",
        ],
    ));

    let root = ws.path().canonicalize().unwrap();
    assert_eq!(
        header_value(&out, "workspace-root"),
        root.display().to_string()
    );
    assert_eq!(
        header_value(&out, "cargo-arc-version"),
        env!("CARGO_PKG_VERSION")
    );
    assert_eq!(header_value(&out, "format-version"), "1");
    let flags = header_value(&out, "flags");
    assert!(flags.contains("include-tests=true"), "{flags}");
    assert!(flags.contains("volatility-months=6"), "{flags}");
    assert!(flags.contains("include-reexports=false"), "{flags}");
}

#[test]
fn the_git_commit_is_head_and_marked_dirty_after_an_uncommitted_change() {
    let ws = workspace();
    let head = git(ws.path(), &["rev-parse", "HEAD"], NOW);

    let clean = stdout_of(&arc(ws.path(), &["query", "hotspots"]));
    assert_eq!(header_value(&clean, "git-commit"), head);

    write_module(ws.path(), "beta", 9);
    let dirty = stdout_of(&arc(ws.path(), &["query", "hotspots"]));
    assert_eq!(header_value(&dirty, "git-commit"), format!("{head}-dirty"));
}

#[test]
fn hotspots_without_top_lists_five_in_ranking_order() {
    let ws = workspace();
    let out = stdout_of(&arc(ws.path(), &["query", "hotspots"]));

    assert_eq!(
        body_of(&out),
        [
            "rank\tpath\tcode-lines\tcommits",
            "1\tsrc/alpha.rs\t8\t3",
            "2\tsrc/beta.rs\t6\t2",
            "3\tsrc/gamma.rs\t5\t1",
            "4\tsrc/delta.rs\t4\t1",
            "5\tsrc/epsilon.rs\t3\t1",
        ]
    );
    assert_eq!(header_value(&out, "top"), "5");
}

#[test]
fn hotspots_top_sets_how_many_are_listed() {
    let ws = workspace();
    let out = stdout_of(&arc(ws.path(), &["query", "hotspots", "-n", "2"]));

    assert_eq!(
        body_of(&out)[1..],
        ["1\tsrc/alpha.rs\t8\t3", "2\tsrc/beta.rs\t6\t2"]
    );
    assert_eq!(header_value(&out, "top"), "2");
}

#[test]
fn hotspots_never_lists_a_file_without_score_in_the_top() {
    let ws = workspace();
    let out = stdout_of(&arc(ws.path(), &["query", "hotspots", "-n", "20"]));

    let paths: Vec<&str> = body_of(&out)[1..]
        .iter()
        .map(|line| line.split('\t').nth(1).unwrap())
        .collect();
    assert_eq!(
        paths,
        [
            "src/alpha.rs",
            "src/beta.rs",
            "src/gamma.rs",
            "src/delta.rs",
            "src/epsilon.rs",
            "src/zeta.rs"
        ]
    );
}

#[test]
fn hotspots_with_paths_lists_each_named_file_with_its_rank_among_all_files() {
    let ws = workspace();
    let out = stdout_of(&arc(
        ws.path(),
        &[
            "query",
            "hotspots",
            "src/quiet.rs",
            "src/zeta.rs",
            "src/alpha.rs",
        ],
    ));

    assert_eq!(
        body_of(&out),
        [
            "rank\tpath\tcode-lines\tcommits",
            "1\tsrc/alpha.rs\t8\t3",
            "6\tsrc/zeta.rs\t2\t1",
            "8\tsrc/quiet.rs\t7\t0",
        ]
    );
    assert_eq!(header_value(&out, "top"), "none");
}

#[test]
fn hotspots_with_a_leading_dot_slash_finds_the_file() {
    let ws = workspace();
    let out = stdout_of(&arc(ws.path(), &["query", "hotspots", "./src/alpha.rs"]));

    assert_eq!(body_of(&out)[1..], ["1\tsrc/alpha.rs\t8\t3"]);
}

#[test]
fn hotspots_with_a_path_outside_the_ranking_fails_naming_it() {
    let ws = workspace();
    std::fs::write(ws.path().join("build.rs"), "fn main() {}\n").unwrap();
    let output = arc(
        ws.path(),
        &[
            "query",
            "hotspots",
            "src/alpha.rs",
            "build.rs",
            "src/nope.rs",
        ],
    );

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains(
            "not in the ranking (files of the module tree; test files only with \
             --include-tests): build.rs, src/nope.rs"
        ),
        "stderr: {stderr}"
    );
    assert!(!stderr.contains("src/alpha.rs"), "stderr: {stderr}");
    assert!(output.stdout.is_empty());
}

#[test]
fn hotspots_with_top_and_paths_is_a_usage_error() {
    let ws = workspace();
    let output = arc(ws.path(), &["query", "hotspots", "-n", "2", "src/alpha.rs"]);

    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("cannot be used with"), "stderr: {stderr}");
}

#[test]
fn hotspots_header_states_the_ranking_and_counting_rules() {
    let ws = workspace();
    let out = stdout_of(&arc(ws.path(), &["query", "hotspots"]));

    assert_eq!(
        header_value(&out, "ranking"),
        "code lines times commits, highest first; a file scoring zero is never among the top N"
    );
    assert_eq!(
        header_value(&out, "ties"),
        "by path, relative to the workspace root"
    );
    assert_eq!(
        header_value(&out, "code-lines"),
        "tokei's code count: source lines without comments and blank lines"
    );
    assert_eq!(
        header_value(&out, "commits"),
        "distinct commits in the window that add, modify, rename or copy the file \
         (git log --diff-filter=AMRC); merge commits and deletions do not count"
    );
}

#[test]
fn hotspots_header_states_the_window_and_the_commits_follow_it() {
    let ws = workspace();

    let now = stdout_of(&arc(ws.path(), &["query", "hotspots", "-n", "2"]));
    assert_eq!(header_value(&now, "window-start"), "2023-05-14");
    assert_eq!(header_value(&now, "window-end"), "2023-11-14");

    // 2023-08-14T08:00:00Z: only the commit of 1690000000 lies in the window,
    // the two later ones lie after its end.
    let earlier = stdout_of(&arc_at(
        ws.path(),
        "1692000000",
        &["query", "hotspots", "-n", "2"],
    ));
    assert_eq!(header_value(&earlier, "window-start"), "2023-02-14");
    assert_eq!(header_value(&earlier, "window-end"), "2023-08-14");
    assert_eq!(
        body_of(&earlier)[1..],
        ["1\tsrc/alpha.rs\t8\t1", "2\tsrc/beta.rs\t6\t1"]
    );
}

#[test]
fn hotspots_without_volatility_print_the_header_and_columns_only() {
    let ws = workspace();
    let out = stdout_of(&arc(ws.path(), &["--no-volatility", "query", "hotspots"]));

    assert_eq!(body_of(&out), ["rank\tpath\tcode-lines\tcommits"]);
    assert_eq!(header_value(&out, "window-start"), "none");
    assert_eq!(header_value(&out, "window-end"), "none");
}

#[test]
fn two_runs_give_the_same_bytes() {
    let ws = workspace();
    let first = stdout_of(&arc(ws.path(), &["query", "hotspots"]));
    let second = stdout_of(&arc(ws.path(), &["query", "hotspots"]));
    assert_eq!(first, second);
}

#[rstest::rstest]
#[case::before(&["query", "--json", "hotspots"])]
#[case::after(&["query", "hotspots", "--json"])]
fn json_lines_carry_the_tsv_header_in_its_order_and_the_same_records(#[case] args: &[&str]) {
    let ws = workspace();
    let tsv = stdout_of(&arc(ws.path(), &["query", "hotspots"]));
    let json = stdout_of(&arc(ws.path(), args));

    let lines: Vec<Value> = json
        .lines()
        .map(|line| serde_json::from_str(line).expect("each line is JSON"))
        .collect();

    // The key order of the first line as written, not as a map reorders it.
    let head_line = json.lines().next().unwrap();
    let json_keys: Vec<String> = header_of(&tsv)
        .into_iter()
        .map(|(key, _)| key)
        .filter(|key| head_line.contains(&format!("\"{key}\":")))
        .collect();
    let tsv_keys: Vec<String> = header_of(&tsv).into_iter().map(|(key, _)| key).collect();
    assert_eq!(
        json_keys, tsv_keys,
        "every TSV header key is in the JSON header"
    );
    let positions: Vec<usize> = tsv_keys
        .iter()
        .map(|key| head_line.find(&format!("\"{key}\":")).unwrap())
        .collect();
    assert!(
        positions.windows(2).all(|pair| pair[0] < pair[1]),
        "JSON header keys follow the TSV order: {head_line}"
    );

    let head = &lines[0];
    assert_eq!(head["top"], 5);
    assert_eq!(
        head["columns"],
        serde_json::json!(["rank", "path", "code-lines", "commits"])
    );
    assert_eq!(
        head["workspace-root"],
        header_value(&tsv, "workspace-root").as_str()
    );
    assert_eq!(
        lines[1],
        serde_json::json!({"rank": 1, "path": "src/alpha.rs", "code-lines": 8, "commits": 3})
    );
    assert_eq!(lines.len(), 6);
}

#[test]
fn an_unknown_query_subcommand_exits_nonzero_naming_the_valid_ones() {
    let ws = workspace();
    let output = arc(ws.path(), &["query", "bogus"]);

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("'bogus' is no subcommand of query; its subcommands: hotspots"),
        "stderr: {stderr}"
    );
}

/// Return the files of `hotspots` in the `STATIC_DATA` of a hotspot map, in
/// rank order, each with its code lines and commits.
fn ranked_in_map(svg: &str) -> Vec<(String, u64, u64)> {
    let json = svg
        .split("const STATIC_DATA = ")
        .nth(1)
        .expect("the map carries STATIC_DATA")
        .split(";\n")
        .next()
        .unwrap();
    let data: Value = serde_json::from_str(json).unwrap();
    data["hotspots"]
        .as_array()
        .unwrap()
        .iter()
        .map(|key| {
            let node = &data["nodes"][key.as_str().unwrap()];
            (
                node["file"].as_str().unwrap().to_string(),
                node["lines"].as_u64().unwrap(),
                node["commits"].as_u64().unwrap(),
            )
        })
        .collect()
}

#[test]
fn hotspots_top_ten_on_cargo_arc_itself_equal_the_map_of_arc_hotspots() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    // One moment for both runs, so both see the same window.
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
        .to_string();
    let svg = stdout_of(&arc_at(&root, &now, &["hotspots"]));
    let out = stdout_of(&arc_at(&root, &now, &["query", "hotspots", "-n", "10"]));

    let queried: Vec<(String, u64, u64)> = body_of(&out)[1..]
        .iter()
        .map(|line| {
            let cells: Vec<&str> = line.split('\t').collect();
            (
                cells[1].to_string(),
                cells[2].parse().unwrap(),
                cells[3].parse().unwrap(),
            )
        })
        .collect();
    let mapped = ranked_in_map(&svg);
    assert!(!mapped.is_empty(), "the map ranks hotspots");
    assert_eq!(queried, mapped);
}
