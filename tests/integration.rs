use cargo_arc::cli::CommonArgs;
use cargo_arc::{ArcCommand, run};
use serde_json::Value;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};

/// Helper: build `ArcCommand` for a fixture with common defaults.
fn fixture_args(fixture: &str, include_tests: bool) -> (tempfile::NamedTempFile, ArcCommand) {
    let fixture_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(format!("tests/fixtures/{fixture}/Cargo.toml"));
    let temp = tempfile::NamedTempFile::new().unwrap();
    let cmd = ArcCommand {
        command: None,
        common: CommonArgs {
            manifest_path: fixture_path,
            features: vec![],
            all_features: false,
            no_default_features: false,
            include_tests,
            include_reexports: false,
            debug: false,
        },
        output: Some(temp.path().to_path_buf()),
        volatility: false,
        no_volatility: true,
        volatility_months: 6,
        volatility_low: 2,
        volatility_high: 10,
        externals: false,
        transitive_deps: false,
        expand_level: None,
        theme: None,
        #[cfg(feature = "hir")]
        hir: false,
    };
    (temp, cmd)
}

/// Helper: build `ArcCommand` for self-analysis (cargo-arc's own Cargo.toml).
fn self_args() -> (tempfile::NamedTempFile, ArcCommand) {
    let temp = tempfile::NamedTempFile::new().unwrap();
    let cmd = ArcCommand {
        command: None,
        common: CommonArgs {
            manifest_path: PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml"),
            features: vec![],
            all_features: false,
            no_default_features: false,
            include_tests: false,
            include_reexports: false,
            debug: false,
        },
        output: Some(temp.path().to_path_buf()),
        volatility: false,
        no_volatility: true,
        volatility_months: 6,
        volatility_low: 2,
        volatility_high: 10,
        externals: false,
        transitive_deps: false,
        expand_level: None,
        theme: None,
        #[cfg(feature = "hir")]
        hir: false,
    };
    (temp, cmd)
}

/// Parse `STATIC_DATA` JSON from SVG output.
fn parse_static_data(svg: &str) -> Value {
    let json_str = svg
        .split("const STATIC_DATA = ")
        .nth(1)
        .expect("SVG should contain STATIC_DATA")
        .split(";\n")
        .next()
        .unwrap();
    serde_json::from_str(json_str).expect("STATIC_DATA should be valid JSON")
}

/// Extract crate names that appear as nodes in the SVG `STATIC_DATA`.
fn extract_crate_names(svg: &str) -> Vec<String> {
    let data = parse_static_data(svg);
    let nodes = data["nodes"].as_object().expect("nodes is object");
    nodes
        .values()
        .filter(|n| n["type"] == "crate")
        .map(|n| n["name"].as_str().unwrap().to_string())
        .collect()
}

/// Extract arc entries from `STATIC_DATA` (from→to with `is_test` derived from context.kind).
fn extract_arcs(svg: &str) -> Vec<(String, String, bool)> {
    let data = parse_static_data(svg);
    let arcs = data["arcs"].as_object().expect("arcs is object");
    arcs.values()
        .map(|a| {
            let from = a["from"].as_str().unwrap().to_string();
            let to = a["to"].as_str().unwrap().to_string();
            let is_test = a["context"]["kind"].as_str() == Some("test");
            (from, to, is_test)
        })
        .collect()
}

/// Extract node-id → name mapping from `STATIC_DATA`.
fn extract_node_names(svg: &str) -> std::collections::HashMap<String, String> {
    let data = parse_static_data(svg);
    let nodes = data["nodes"].as_object().expect("nodes is object");
    nodes
        .iter()
        .map(|(id, n)| (id.clone(), n["name"].as_str().unwrap().to_string()))
        .collect()
}

/// Resolve arc (`from_id`, `to_id`) to (`from_name`, `to_name`).
fn resolve_arc_names(
    arcs: &[(String, String, bool)],
    nodes: &std::collections::HashMap<String, String>,
) -> Vec<(String, String, bool)> {
    arcs.iter()
        .filter_map(|(from, to, is_test)| {
            Some((nodes.get(from)?.clone(), nodes.get(to)?.clone(), *is_test))
        })
        .collect()
}

/// The symbols an arc carries, keyed by (`from_name`, `to_name`) and sorted.
fn extract_arc_symbols(svg: &str) -> std::collections::HashMap<(String, String), Vec<String>> {
    let nodes = extract_node_names(svg);
    let data = parse_static_data(svg);
    data["arcs"]
        .as_object()
        .expect("arcs is object")
        .values()
        .filter_map(|a| {
            let from = nodes.get(a["from"].as_str()?)?.clone();
            let to = nodes.get(a["to"].as_str()?)?.clone();
            let mut symbols: Vec<String> = a["usages"]
                .as_array()?
                .iter()
                .filter_map(|u| Some(u["symbol"].as_str()?.to_string()))
                .collect();
            symbols.sort();
            Some(((from, to), symbols))
        })
        .collect()
}

#[test]
fn test_multi_crate_fixture() {
    let (temp, cmd) = fixture_args("multi_crate", false);

    let result = run(cmd);
    assert!(result.is_ok(), "run() should succeed: {result:?}");

    let svg = std::fs::read_to_string(temp.path()).unwrap();

    // Valid SVG structure
    assert!(svg.contains("<svg"), "should have svg element");

    // Both crates visible
    assert!(svg.contains("crate_a"), "should show crate_a");
    assert!(svg.contains("crate_b"), "should show crate_b");

    // Modules visible
    assert!(svg.contains("alpha"), "should show alpha module");
    assert!(svg.contains("beta"), "should show beta module");
    assert!(svg.contains("gamma"), "should show gamma module");
}

#[test]
fn test_custom_target_paths_fixture() {
    let (temp, cmd) = fixture_args("custom_root_workspace", false);

    let result = run(cmd);
    assert!(result.is_ok(), "run() should succeed: {result:?}");

    let svg = std::fs::read_to_string(temp.path()).unwrap();
    let names: Vec<String> = extract_node_names(&svg).into_values().collect();

    for expected in [
        "renamed-lib",
        "outside-src",
        "custom-bin",
        // Children of `[lib] path = "src/entry.rs"` — resolved in src/, not src/entry/
        "alpha",
        "beta",
        // Child of a lib root outside src/
        "util",
        // Child of `[[bin]] path = "src/tool.rs"`
        "runner",
        // Child of a lib root that has a conventional src/main.rs beside it —
        // picking the root by file name would find the binary and miss this.
        "gamma",
    ] {
        assert!(
            names.iter().any(|n| n == expected),
            "should show '{expected}', found: {names:?}"
        );
    }

    let arcs = resolve_arc_names(&extract_arcs(&svg), &extract_node_names(&svg));
    assert!(
        arcs.iter()
            .any(|(from, to, _)| from == "runner" && to == "alpha"),
        "runner should depend on renamed_lib::alpha, found: {arcs:?}"
    );
}

/// Crates whose root file declares no submodules carry no `Contains` edge and
/// therefore reach the layout only through a production crate dependency.
#[test]
fn test_single_file_crates_stay_in_layout() {
    let (temp, cmd) = fixture_args("single_file_crates", false);

    let result = run(cmd);
    assert!(result.is_ok(), "run() should succeed: {result:?}");

    let svg = std::fs::read_to_string(temp.path()).unwrap();
    let nodes = extract_node_names(&svg);
    let crates = extract_crate_names(&svg);

    for expected in ["app-core", "app-leaf", "app-tool"] {
        assert!(
            crates.iter().any(|c| c == expected),
            "should show '{expected}', found: {crates:?}"
        );
    }

    let arcs = resolve_arc_names(&extract_arcs(&svg), &nodes);
    for target in ["app-leaf", "app-tool"] {
        assert!(
            arcs.iter()
                .any(|(from, to, _)| from == "app-core" && to == target),
            "app-core should depend on {target}, found: {arcs:?}"
        );
    }
}

/// A binary at the top of a workspace: no submodules, nobody depends on it.
/// Without the dev edges in the graph it is indistinguishable from a test
/// helper, and pruning takes the workspace's entry point with it.
#[test]
fn test_leaf_binary_stays_in_layout() {
    let (temp, cmd) = fixture_args("consumer_crate", false);

    let result = run(cmd);
    assert!(result.is_ok(), "run() should succeed: {result:?}");

    let svg = std::fs::read_to_string(temp.path()).unwrap();
    let crates = extract_crate_names(&svg);
    assert!(
        crates.iter().any(|c| c == "zebra-app"),
        "should show 'zebra-app', found: {crates:?}"
    );

    // The crate-level arcs are suppressed by the module-level ones they duplicate.
    let arcs = resolve_arc_names(&extract_arcs(&svg), &extract_node_names(&svg));
    assert!(
        arcs.iter()
            .any(|(from, to, _)| from == "zebra-app" && to == "models"),
        "zebra-app should depend on alpha-core::models, found: {arcs:?}"
    );
}

#[test]
fn test_self_analysis() {
    let (temp, cmd) = self_args();

    let result = run(cmd);
    assert!(result.is_ok(), "run() should succeed: {result:?}");

    let svg = std::fs::read_to_string(temp.path()).unwrap();

    // Valid SVG structure
    assert!(svg.contains("<?xml"), "should have XML declaration");
    assert!(svg.contains("<svg"), "should have svg element");
    assert!(svg.contains("</svg>"), "should close svg element");

    // All cargo-arc modules visible
    assert!(svg.contains("analyze"), "should show analyze module");
    assert!(svg.contains("graph"), "should show graph module");
    assert!(svg.contains("layout"), "should show layout module");
    assert!(svg.contains("render"), "should show render module");
}

#[test]
fn test_cfg_test_excluded_by_default() {
    let (temp, cmd) = fixture_args("multi_crate", false);

    let result = run(cmd);
    assert!(result.is_ok(), "run() should succeed: {result:?}");

    let svg = std::fs::read_to_string(temp.path()).unwrap();

    // test_utils module should NOT be visible (cfg(test) is excluded by default)
    assert!(
        !svg.contains("test_utils"),
        "test_utils should be hidden by default (cfg(test) excluded)"
    );
}

#[test]
fn test_cfg_test_included_with_flag() {
    let fixture_path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/multi_crate/Cargo.toml");

    let temp = tempfile::NamedTempFile::new().unwrap();
    let cmd = ArcCommand {
        command: None,
        common: CommonArgs {
            manifest_path: fixture_path,
            features: vec![],
            all_features: false,
            no_default_features: false,
            include_tests: true,
            include_reexports: false,
            debug: false,
        },
        output: Some(temp.path().to_path_buf()),
        volatility: false,
        no_volatility: false,
        volatility_months: 6,
        volatility_low: 2,
        volatility_high: 10,
        externals: false,
        transitive_deps: false,
        expand_level: None,
        theme: None,
        #[cfg(feature = "hir")]
        hir: false,
    };

    let result = run(cmd);
    assert!(result.is_ok(), "run() should succeed: {result:?}");

    let svg = std::fs::read_to_string(temp.path()).unwrap();

    // test_utils module SHOULD be visible when --cfg test is passed
    assert!(
        svg.contains("test_utils"),
        "test_utils should be visible with --cfg test"
    );
}

#[test]
fn test_entry_point_imports() {
    let fixture_path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/entry_point/Cargo.toml");

    let temp = tempfile::NamedTempFile::new().unwrap();
    let cmd = ArcCommand {
        command: None,
        common: CommonArgs {
            manifest_path: fixture_path,
            features: vec![],
            all_features: false,
            no_default_features: false,
            include_tests: false,
            include_reexports: false,
            debug: false,
        },
        output: Some(temp.path().to_path_buf()),
        volatility: false,
        no_volatility: false,
        volatility_months: 6,
        volatility_low: 2,
        volatility_high: 10,
        externals: false,
        transitive_deps: false,
        expand_level: None,
        theme: None,
        #[cfg(feature = "hir")]
        hir: false,
    };

    let result = run(cmd);
    assert!(result.is_ok(), "run() should succeed: {result:?}");

    let svg = std::fs::read_to_string(temp.path()).unwrap();

    // Valid SVG structure
    assert!(svg.contains("<svg"), "should have svg element");

    // Both crates visible
    assert!(svg.contains("crate_a"), "should show crate_a");
    assert!(svg.contains("crate_b"), "should show crate_b");

    // Modules visible
    assert!(svg.contains("sub"), "should show sub module in crate_a");
    assert!(svg.contains("mod_b"), "should show mod_b module in crate_b");

    // Entry-point imports should create arcs with source locations (shown in STATIC_DATA).
    // Helper is imported from crate_a's entry point in crate_b's lib.rs,
    // Exported is imported from crate_a's entry point in crate_b's mod_b.rs.
    assert!(
        svg.contains("Helper") || svg.contains("Exported"),
        "SVG should contain entry-point symbol names in STATIC_DATA usages"
    );
}

/// Dev-dependency crate appears as phantom node without --include-tests.
///
/// Fixture topology (`dev_dep_sorting)`:
///   foundation  — production crate with modules (handler, service, models, common, `test_support`)
///   consumer    — only dev-depends on foundation + `test_helper`
///   `test_helper` — standalone test utility, no production deps
///
/// Without --include-tests:
///   - `CrateDep` edges from dev-dependencies should NOT appear
///   - `test_helper` should NOT appear (no production path)
///   - consumer should NOT appear (no production path)
///   - Only foundation with its internal module structure should remain
///
/// With --include-tests:
///   - All three crates visible
///   - consumer→foundation and `consumer→test_helper` arcs present
///   - `foundation→test_helper` arc present
#[test]
fn test_reexport_resolution() {
    let (temp, cmd) = fixture_args("reexport_workspace", false);

    let result = run(cmd);
    assert!(result.is_ok(), "run() should succeed: {result:?}");

    let svg = std::fs::read_to_string(temp.path()).unwrap();
    let arcs = extract_arcs(&svg);
    let nodes = extract_node_names(&svg);
    let named_arcs = resolve_arc_names(&arcs, &nodes);

    // Re-export resolved: child -> sibling (via Widget defined in sibling)
    let has_child_to_sibling = named_arcs
        .iter()
        .any(|(from, to, _)| from == "child" && to == "sibling");
    assert!(
        has_child_to_sibling,
        "child -> sibling arc should exist (re-export resolved), found arcs: {named_arcs:?}"
    );

    // Re-export resolved means NO child -> parent arc (Widget is not defined in parent)
    let has_child_to_parent = named_arcs
        .iter()
        .any(|(from, to, _)| from == "child" && to == "parent");
    assert!(
        !has_child_to_parent,
        "child -> parent arc should NOT exist (re-export should be resolved to sibling), found arcs: {named_arcs:?}"
    );
}

/// A glob edge must carry the names it imports, not the `*` that spells them.
/// Covers the full pipeline: only a populated re-export map can name the payload,
/// so a map that never reaches the resolver would leave the `*` in place here.
/// The glob's module also exports `Extra`, which the file never writes.
#[test]
fn test_glob_import_carries_the_names_used() {
    let (temp, cmd) = fixture_args("reexport_workspace", false);

    let result = run(cmd);
    assert!(result.is_ok(), "run() should succeed: {result:?}");

    let svg = std::fs::read_to_string(temp.path()).unwrap();
    let symbols = extract_arc_symbols(&svg);

    let key = ("glob_user".to_string(), "sibling".to_string());
    assert_eq!(
        symbols.get(&key).map(Vec::as_slice),
        Some(["Widget".to_string()].as_slice()),
        "glob_user -> sibling should carry the names glob_user uses, found: {symbols:?}"
    );
}

/// A private glob in a parent module is visible to its children. A child that
/// takes a name through `use super::*` depends on the module that defines it,
/// not on the parent; the parent's own edge keeps the `*` it never uses.
#[test]
fn test_child_resolves_through_the_parents_private_glob() {
    let (temp, cmd) = fixture_args("reexport_workspace", false);

    let result = run(cmd);
    assert!(result.is_ok(), "run() should succeed: {result:?}");

    let svg = std::fs::read_to_string(temp.path()).unwrap();
    let symbols = extract_arc_symbols(&svg);

    let child = ("buffer".to_string(), "actions".to_string());
    assert_eq!(
        symbols.get(&child).map(Vec::as_slice),
        Some(["Mapping".to_string()].as_slice()),
        "buffer -> actions should carry the name buffer uses, found: {symbols:?}"
    );
    let parent = ("forwarder".to_string(), "actions".to_string());
    assert_eq!(
        symbols.get(&parent).map(Vec::as_slice),
        Some(["*".to_string()].as_slice()),
        "forwarder -> actions should keep the unused glob's `*`, found: {symbols:?}"
    );
    assert!(
        !symbols.contains_key(&("buffer".to_string(), "forwarder".to_string())),
        "buffer -> forwarder should not exist, found: {symbols:?}"
    );
}

/// Every use on an edge, with its category, goes to stderr under `--debug`,
/// so the walker's result can be read without a rule that consumes it.
/// A subprocess, because `run` installs the debug log on the process's own
/// stderr, which an in-process test cannot capture.
#[test]
fn debug_lists_the_uses_of_an_edge_with_category() {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/reexport_workspace/Cargo.toml");
    let temp = tempfile::NamedTempFile::new().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_cargo-arc"))
        .arg("arc")
        .arg("--manifest-path")
        .arg(&manifest)
        .arg("--debug")
        .arg("-o")
        .arg(temp.path())
        .output()
        .expect("failed to execute cargo-arc");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains(
            "use my_crate::parent::glob_user -> my_crate::parent::sibling: Widget types \
             at my_crate/src/parent/glob_user.rs:3"
        ),
        "stderr: {stderr}"
    );
    assert!(
        stderr.contains(
            "use my_crate::parent::glob_user -> my_crate::parent::sibling: Widget types \
             at my_crate/src/parent/glob_user.rs:4"
        ),
        "the constructed value is a second use: {stderr}"
    );
}

#[test]
fn test_dev_dep_crate_hidden_without_include_tests() {
    let (temp, cmd) = fixture_args("dev_dep_sorting", false);
    let result = run(cmd);
    assert!(result.is_ok(), "run() should succeed: {result:?}");

    let svg = std::fs::read_to_string(temp.path()).unwrap();
    let crates = extract_crate_names(&svg);
    let nodes = extract_node_names(&svg);
    let arcs = extract_arcs(&svg);
    let named_arcs = resolve_arc_names(&arcs, &nodes);

    // test_helper has no production consumers → should be hidden
    assert!(
        !crates.contains(&"test_helper".to_string()),
        "test_helper should NOT appear without --include-tests (phantom node), but found crates: {crates:?}"
    );

    // shared_lib is only reachable via test_helper's prod dep → transitive test infra → should be hidden
    assert!(
        !crates.contains(&"shared_lib".to_string()),
        "shared_lib should NOT appear without --include-tests (transitive dev-dep), but found crates: {crates:?}"
    );

    // consumer only has dev-deps → should be hidden too
    assert!(
        !crates.contains(&"consumer".to_string()),
        "consumer should NOT appear without --include-tests (only dev-deps), but found crates: {crates:?}"
    );

    // No test-context arcs should exist
    let test_arcs: Vec<_> = named_arcs
        .iter()
        .filter(|(_, _, is_test)| *is_test)
        .collect();
    assert!(
        test_arcs.is_empty(),
        "no test arcs should appear without --include-tests, but found: {test_arcs:?}"
    );

    // foundation should still be visible with its production modules
    assert!(
        crates.contains(&"foundation".to_string()),
        "foundation should remain visible (production crate)"
    );
    assert!(
        svg.contains("handler"),
        "foundation::handler should be visible"
    );
    assert!(
        svg.contains("service"),
        "foundation::service should be visible"
    );
    assert!(
        svg.contains("models"),
        "foundation::models should be visible"
    );
    assert!(
        svg.contains("common"),
        "foundation::common should be visible"
    );
}

#[test]
fn test_dev_dep_crate_visible_with_include_tests() {
    let (temp, cmd) = fixture_args("dev_dep_sorting", true);
    let result = run(cmd);
    assert!(result.is_ok(), "run() should succeed: {result:?}");

    let svg = std::fs::read_to_string(temp.path()).unwrap();
    let crates = extract_crate_names(&svg);

    // All four crates should be visible with --include-tests
    assert!(
        crates.contains(&"foundation".to_string()),
        "foundation should be visible with --include-tests"
    );
    assert!(
        crates.contains(&"consumer".to_string()),
        "consumer should be visible with --include-tests"
    );
    assert!(
        crates.contains(&"test_helper".to_string()),
        "test_helper should be visible with --include-tests"
    );
    assert!(
        crates.contains(&"shared_lib".to_string()),
        "shared_lib should be visible with --include-tests"
    );
}

/// Success criterion: a default run's `STATIC_DATA` carries neither `targets`
/// (crate/module jump targets) nor `jump` (usage-location jump ids).
#[test]
fn default_static_data_carries_no_jump_fields() {
    let (temp, cmd) = fixture_args("multi_crate", false);

    let result = run(cmd);
    assert!(result.is_ok(), "run() should succeed: {result:?}");

    let svg = std::fs::read_to_string(temp.path()).unwrap();
    let json = serde_json::to_string(&parse_static_data(&svg)).unwrap();

    assert!(
        !json.contains("\"targets\""),
        "default STATIC_DATA should not carry jump targets, got: {json}"
    );
    assert!(
        !json.contains("\"jump\""),
        "default STATIC_DATA should not carry jump ids, got: {json}"
    );
}

// ===== Phase 4: check subcommand integration tests =====

/// Copies `<fixture>/<rules_file>` into a fresh tempdir as `arc-rules.toml`,
/// so a `--generate-baseline` run writes `arc-baseline.toml` there instead of
/// into the checked-in fixture.
fn isolated_rules_copy(fixture: &str, rules_file: &str) -> (tempfile::TempDir, PathBuf) {
    let src = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(format!("tests/fixtures/{fixture}/{rules_file}"));
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("arc-rules.toml");
    std::fs::copy(&src, &dest).unwrap();
    (dir, dest)
}

/// Run `cargo-arc arc --manifest-path <fixture>/Cargo.toml check [check_args...]` as subprocess.
/// Returns (`exit_code`, stderr).
fn cargo_arc_check(fixture: &str, check_args: &[&str]) -> (i32, String) {
    let (code, _stdout, stderr) = cargo_arc_check_streams(fixture, check_args);
    (code, stderr)
}

/// Like [`cargo_arc_check`], but keeps stdout apart from stderr. The two carry
/// different things: stdout judges, stderr reports.
/// Returns (`exit_code`, stdout, stderr).
fn cargo_arc_check_streams(fixture: &str, check_args: &[&str]) -> (i32, String, String) {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(format!("tests/fixtures/{fixture}/Cargo.toml"));
    let output = Command::new(env!("CARGO_BIN_EXE_cargo-arc"))
        .arg("arc")
        .arg("--manifest-path")
        .arg(&manifest)
        .arg("check")
        .args(check_args)
        .output()
        .expect("failed to execute cargo-arc");
    let code = output.status.code().unwrap_or(-1);
    (
        code,
        String::from_utf8_lossy(&output.stdout).to_string(),
        String::from_utf8_lossy(&output.stderr).to_string(),
    )
}

/// An overlap is refused while the rules run, not while the file loads, so the
/// path has to be attached on the way out rather than coming from the loader.
#[test]
fn overlapping_layer_positions_name_the_rules_file() {
    let dir = tempfile::tempdir().unwrap();
    let rules = dir.path().join("arc-rules.toml");
    std::fs::write(
        &rules,
        "[[rules]]\n\
         type = \"layers\"\n\
         name = \"overlapping layers\"\n\
         layers = [[\"domain*\"], [\"*main\"]]\n\
         direction = \"top-down\"\n",
    )
    .unwrap();
    let (code, stderr) = cargo_arc_check(
        "arch_violation_workspace",
        &["--rules", rules.to_str().unwrap()],
    );
    assert_ne!(code, 0, "an overlap must fail the run, stderr: {stderr}");
    assert!(
        stderr.contains(&rules.display().to_string()),
        "the message must name the rules file, stderr: {stderr}"
    );
    assert!(
        stderr.contains("overlapping layers"),
        "the message must name the rule, stderr: {stderr}"
    );
}

/// Run `check`, optionally with `--include-reexports`. That flag is a
/// common-level argument, so it precedes the subcommand on the command line.
fn cargo_arc_check_reexports(fixture: &str, include_reexports: bool) -> (i32, String) {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(format!("tests/fixtures/{fixture}/Cargo.toml"));
    let mut command = Command::new(env!("CARGO_BIN_EXE_cargo-arc"));
    command.arg("arc").arg("--manifest-path").arg(&manifest);
    if include_reexports {
        command.arg("--include-reexports");
    }
    command.arg("check");
    let output = command.output().expect("failed to execute cargo-arc");
    let code = output.status.code().unwrap_or(-1);
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    (code, stderr)
}

#[test]
fn test_pure_reexport_cycle_excluded_by_default() {
    // Fixture has two independent module cycles: a pure re-export cycle
    // (alpha <-> beta) and a real logic cycle (gamma <-> delta).

    // Default (logic subgraph): the re-export cycle is filtered out; only the
    // real cycle is reported.
    let (code, stderr) = cargo_arc_check_reexports("reexport_cycle_workspace", false);
    assert_eq!(
        code, 1,
        "real cycle should still fail the check, stderr: {stderr}"
    );
    assert!(
        stderr.contains("gamma") && stderr.contains("delta"),
        "real logic cycle (gamma <-> delta) should be reported, stderr: {stderr}"
    );
    assert!(
        !stderr.contains("alpha") && !stderr.contains("beta"),
        "pure re-export cycle (alpha <-> beta) should be excluded by default, stderr: {stderr}"
    );

    // --include-reexports (full graph): the re-export cycle reappears.
    let (code, stderr) = cargo_arc_check_reexports("reexport_cycle_workspace", true);
    assert_eq!(
        code, 1,
        "both cycles should fail the check, stderr: {stderr}"
    );
    assert!(
        stderr.contains("alpha") && stderr.contains("beta"),
        "pure re-export cycle should reappear with --include-reexports, stderr: {stderr}"
    );
    assert!(
        stderr.contains("gamma") && stderr.contains("delta"),
        "real cycle should also be present with --include-reexports, stderr: {stderr}"
    );
}

/// Run `cargo-arc arc --manifest-path <manifest> <shared_args...> hotspots
/// <hotspots_args...>` as a subprocess. Returns (`exit_code`, stdout, stderr).
fn cargo_arc_hotspots(
    manifest: &PathBuf,
    shared_args: &[&str],
    hotspots_args: &[&str],
) -> (i32, String, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_cargo-arc"))
        .arg("arc")
        .arg("--manifest-path")
        .arg(manifest)
        .args(shared_args)
        .arg("hotspots")
        .args(hotspots_args)
        .output()
        .expect("failed to execute cargo-arc");
    let code = output.status.code().unwrap_or(-1);
    (
        code,
        String::from_utf8_lossy(&output.stdout).to_string(),
        String::from_utf8_lossy(&output.stderr).to_string(),
    )
}

/// `--no-volatility` greys the whole map: sizes are still drawn (the crate
/// and file nodes are all present), but nothing carries a rank and the
/// hotspot list is empty. No error either way.
#[test]
fn hotspots_no_volatility_greys_the_map_with_no_ranks() {
    let manifest =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/multi_crate/Cargo.toml");
    let temp = tempfile::Builder::new().suffix(".html").tempfile().unwrap();
    let (code, _stdout, stderr) = cargo_arc_hotspots(
        &manifest,
        &["--no-volatility"],
        &["-o", temp.path().to_str().unwrap()],
    );
    assert_eq!(code, 0, "hotspots run should succeed, stderr: {stderr}");

    let html = std::fs::read_to_string(temp.path()).unwrap();
    let data = parse_static_data(&html);
    let nodes = data["nodes"].as_object().expect("nodes is object");
    let keys: Vec<&String> = nodes.keys().collect();

    for expected in [
        "crate_a/Cargo.toml",
        "crate_b/Cargo.toml",
        "crate_a/src/alpha.rs",
        "crate_a/src/beta.rs",
        "crate_b/src/gamma.rs",
    ] {
        assert!(
            nodes.contains_key(expected),
            "expected node key {expected:?}, got: {keys:?}"
        );
    }

    assert!(
        nodes.values().all(|n| n["rank"].is_null()),
        "no node should carry a rank without volatility data, got: {data}"
    );
    assert!(
        data["hotspots"]
            .as_array()
            .expect("hotspots is an array")
            .is_empty(),
        "hotspot list must be empty without volatility data, got: {data}"
    );
    assert!(
        html.contains("hotspot-grey"),
        "every circle must carry the grey class, got: {html}"
    );
    assert!(
        html.contains("id=\"hotspot-volatility-note\""),
        "the sidebar must show why the map is grey, got: {html}"
    );
    assert_eq!(
        data["volatility"],
        "Volatility disabled: run without --no-volatility to see commit activity.",
        "the note must name the flag, not a generic message, got: {data}"
    );
}

/// With volatility on, the hotspot list has at most `--hotspots` entries and
/// their ranks run `1..=N` in order: the top-N leaves workspace-wide by
/// lines × commits.
#[test]
fn hotspots_with_volatility_ranks_at_most_n_leaves() {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
    let temp = tempfile::Builder::new().suffix(".html").tempfile().unwrap();
    let n = 5;
    let (code, _stdout, stderr) = cargo_arc_hotspots(
        &manifest,
        &[],
        &[
            "-o",
            temp.path().to_str().unwrap(),
            "--hotspots",
            &n.to_string(),
        ],
    );
    assert_eq!(code, 0, "hotspots run should succeed, stderr: {stderr}");

    let html = std::fs::read_to_string(temp.path()).unwrap();
    let data = parse_static_data(&html);
    assert!(
        data["volatility"].is_null(),
        "a map with commits carries no grey note, got: {data}"
    );
    let hotspots = data["hotspots"].as_array().expect("hotspots is an array");
    assert!(
        hotspots.len() <= n,
        "hotspot list must have at most {n} entries, got {}: {hotspots:?}",
        hotspots.len()
    );

    let nodes = data["nodes"].as_object().expect("nodes is object");
    let mut ranks: Vec<i64> = hotspots
        .iter()
        .map(|key| {
            let key = key.as_str().expect("hotspot key is a string");
            nodes[key]["rank"]
                .as_i64()
                .unwrap_or_else(|| panic!("hotspot {key} has a rank"))
        })
        .collect();
    ranks.sort_unstable();
    let expected: Vec<i64> = (1..=i64::try_from(hotspots.len()).unwrap()).collect();
    assert_eq!(
        ranks, expected,
        "ranks must run 1..=N in order, got: {data}"
    );
}

/// Reads the `stderr` layout of `format::rule_block`: one block per rule,
/// separated by a blank line, each violation carrying its edge on a `  = ` line.
fn layers_violation_edges(stderr: &str) -> Vec<String> {
    stderr
        .split("\n\n")
        .filter(|block| block.starts_with("error[layers]"))
        .flat_map(|block| {
            block
                .lines()
                .filter_map(|line| line.strip_prefix("  = ").map(str::to_string))
        })
        .collect()
}

#[test]
fn test_check_with_violations() {
    let (code, stderr) = cargo_arc_check("arch_violation_workspace", &[]);
    assert_eq!(code, 1, "should exit 1 on violations, stderr: {stderr}");
    assert!(
        stderr.contains("error[forbidden-dependency]"),
        "should report forbidden-dependency violation, stderr: {stderr}"
    );
    assert!(
        stderr.contains("error[layers]"),
        "should report layers violation, stderr: {stderr}"
    );
    assert!(
        stderr.contains("error[no-cycles]"),
        "should report no-cycles violation, stderr: {stderr}"
    );

    // The fixture orders its layers outside-in, so only an edge pointing back
    // out is a violation. Checked per edge because `forbidden-dependency`
    // reports the same pair and a plain substring search would not tell them
    // apart.
    let layers_edges = layers_violation_edges(&stderr);
    assert!(
        layers_edges.contains(&"domain::service → infra::db".to_string()),
        "layers rule should flag domain::service → infra::db, got: {layers_edges:?}"
    );
    assert!(
        !layers_edges.contains(&"application::handler → domain::service".to_string()),
        "layers rule should not flag application::handler → domain::service, got: {layers_edges:?}"
    );
}

/// A location prints relative to the workspace root, not to the crate that
/// owns the file: it opens from wherever the manifest given to `-m` sits.
#[test]
fn locations_are_relative_to_the_workspace_root() {
    let (code, stderr) = cargo_arc_check("arch_violation_workspace", &[]);
    assert_eq!(code, 1, "should exit 1 on violations, stderr: {stderr}");
    assert!(
        stderr.contains("--> domain/src/"),
        "a location should be relative to the workspace root, got: {stderr}"
    );
    assert!(
        !stderr.contains("--> src/"),
        "a location should not be relative to its own crate root, got: {stderr}"
    );
}

/// `-m` on a member manifest still anchors locations at the workspace root:
/// `cargo_metadata` resolves the same `workspace_root` no matter which
/// member's manifest is named, and the anchor follows it, not `-m`.
#[test]
fn locations_stay_workspace_relative_for_a_member_manifest() {
    let rules = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/arch_violation_workspace/arc-rules.toml");
    let rules_arg = format!("--rules={}", rules.display());
    let (code, stderr) = cargo_arc_check("arch_violation_workspace/domain", &[&rules_arg]);
    assert_eq!(code, 1, "should exit 1 on violations, stderr: {stderr}");
    assert!(
        stderr.contains("--> domain/src/"),
        "a location should stay relative to the workspace root, got: {stderr}"
    );
    assert!(
        !stderr.contains("--> src/"),
        "a location should not be relative to the member manifest's directory, got: {stderr}"
    );
}

/// Two violations from different crates whose files sit at the same path
/// under their own crate root must not print the same location: the anchor
/// has to be the workspace root, or the two would be indistinguishable.
#[test]
fn locations_from_different_crates_do_not_collide() {
    let (_dir, manifest) = writable_fixture_copy("arch_violation_workspace");
    let root = manifest.parent().unwrap();

    // Only the two forbidden-dependency rules: the fixture's `layers` rule
    // would flag the same edges again under its own block, which is a second
    // report of one violation, not a second violation.
    let rules_path = root.join("arc-rules.toml");
    std::fs::write(
        &rules_path,
        "[config]\n\
         version = 1\n\
         \n\
         [[rules]]\n\
         type = \"forbidden-dependency\"\n\
         name = \"no infra in domain\"\n\
         from = \"domain\"\n\
         to = \"infra::**\"\n\
         severity = \"error\"\n\
         \n\
         [[rules]]\n\
         type = \"forbidden-dependency\"\n\
         name = \"no infra in application\"\n\
         from = \"application\"\n\
         to = \"infra::**\"\n\
         severity = \"error\"\n",
    )
    .unwrap();

    // Both crate roots gain the same forbidden import on the same line, so a
    // path relative to the crate itself would read identically for both.
    for member in ["application", "domain"] {
        let lib = root.join(member).join("src/lib.rs");
        let existing = std::fs::read_to_string(&lib).unwrap();
        std::fs::write(&lib, format!("use infra::db;\n{existing}")).unwrap();
    }

    let rules_arg = format!("--rules={}", rules_path.display());
    let (code, stderr) = cargo_arc_check_at(&manifest, &[&rules_arg]);
    assert_eq!(code, 1, "should exit 1 on violations, stderr: {stderr}");

    let locations: Vec<&str> = stderr
        .lines()
        .filter_map(|l| l.strip_prefix("    --> "))
        .collect();
    let unique: std::collections::HashSet<&str> = locations.iter().copied().collect();
    assert_eq!(
        locations.len(),
        unique.len(),
        "two violations from different crates must not print the same location, got: {locations:?}"
    );
    assert!(
        locations.iter().any(|l| l.starts_with("domain/src/lib.rs")),
        "expected a domain-prefixed location, got: {locations:?}"
    );
    assert!(
        locations
            .iter()
            .any(|l| l.starts_with("application/src/lib.rs")),
        "expected an application-prefixed location, got: {locations:?}"
    );
}

/// The diagram side of the same anchor: a `UsageLocation.file` in
/// `STATIC_DATA` carries the crate prefix too.
#[test]
fn static_data_usage_locations_carry_the_crate_prefix() {
    let (temp, cmd) = fixture_args("arch_violation_workspace", false);

    let result = run(cmd);
    assert!(result.is_ok(), "run() should succeed: {result:?}");

    let svg = std::fs::read_to_string(temp.path()).unwrap();
    let data = parse_static_data(&svg);
    let files: Vec<String> = data["arcs"]
        .as_object()
        .expect("arcs is object")
        .values()
        .flat_map(|a| a["usages"].as_array().into_iter().flatten())
        .flat_map(|u| u["locations"].as_array().into_iter().flatten())
        .filter_map(|l| l["file"].as_str().map(str::to_string))
        .collect();
    assert!(
        files.iter().any(|f| f.starts_with("domain/src/")),
        "a UsageLocation.file should carry the crate prefix, got: {files:?}"
    );
}

/// Runs `check` on the transitive fixture (`a → b → c → d`, one dependency
/// each) with one of its rules files. None of them positions `c`, so every
/// dependency between two positioned crates runs through an unpositioned one.
fn transitive_layers_check(rules_file: &str) -> (i32, Vec<String>) {
    let rules = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!(
        "tests/fixtures/transitive_layers_workspace/{rules_file}"
    ));
    let rules_arg = format!("--rules={}", rules.display());
    let (code, stderr) = cargo_arc_check("transitive_layers_workspace", &[&rules_arg]);
    (code, layers_violation_edges(&stderr))
}

#[test]
fn layers_hold_when_the_order_follows_the_code() {
    let (code, edges) = transitive_layers_check("layers-a-b-d.toml");
    assert_eq!(code, 0, "the order follows the code, edges: {edges:?}");
    assert!(edges.is_empty(), "nothing to report, got: {edges:?}");
}

/// Both orders put `d` above `b`, and `b` reaches `d` through `c`. The pair is
/// reported, not the edges that carry it.
#[test]
fn layers_report_a_dependency_running_through_an_unpositioned_crate() {
    for rules_file in ["layers-d-a-b.toml", "layers-a-d-b.toml"] {
        let (code, edges) = transitive_layers_check(rules_file);
        assert_eq!(code, 1, "{rules_file} puts d above b, edges: {edges:?}");
        assert_eq!(edges, ["b → d"], "from {rules_file}");
    }
}

/// The written edge keeps its own report. `a → b` stands against the order,
/// `b → d` runs downward under it.
#[test]
fn layers_report_a_written_edge_as_before() {
    let (code, edges) = transitive_layers_check("layers-b-a-d.toml");
    assert_eq!(code, 1, "b above a contradicts a → b, edges: {edges:?}");
    assert_eq!(edges, ["a → b"]);
}

/// The baseline key is the pair, so a generated entry names `b` and `d` and
/// nothing about the edges between them.
#[test]
fn a_transitive_dependency_freezes_on_its_pair() {
    let (dir, rules_path) = isolated_rules_copy("transitive_layers_workspace", "layers-a-d-b.toml");
    let rules_arg = format!("--rules={}", rules_path.display());

    let (code, stderr) = cargo_arc_check(
        "transitive_layers_workspace",
        &[&rules_arg, "--generate-baseline"],
    );
    assert_eq!(code, 0, "generate should exit 0, stderr: {stderr}");
    let written = std::fs::read_to_string(dir.path().join("arc-baseline.toml")).unwrap();
    assert!(
        written.contains("from = \"b\"") && written.contains("to = \"d\""),
        "the entry should name the pair, baseline: {written}"
    );

    let (code, stderr) = cargo_arc_check("transitive_layers_workspace", &[&rules_arg]);
    assert_eq!(code, 0, "the frozen pair is not reported, stderr: {stderr}");
}

/// `allow` works on the pair as well. `unmatched-allow` asks whether the
/// patterns resolve to nodes, not whether an edge is written between them, so
/// an entry on a pair with no edge is not reported as dead.
#[test]
fn an_allow_on_the_pair_allows_a_transitive_dependency() {
    let dir = tempfile::tempdir().unwrap();
    let rules_path = dir.path().join("arc-rules.toml");
    std::fs::write(
        &rules_path,
        r#"
[config]
version = 1

[[rules]]
type = "layers"
name = "architecture layers"
layers = ["a", "d", "b"]
direction = "top-down"
allow = [{ from = "b", to = "d" }]

[diagnostics]
unmatched-allow = "deny"
"#,
    )
    .unwrap();
    let rules_arg = format!("--rules={}", rules_path.display());

    let (code, stderr) = cargo_arc_check("transitive_layers_workspace", &[&rules_arg]);
    assert_eq!(code, 0, "the pair is allowed, stderr: {stderr}");
    assert!(
        !stderr.contains("unmatched-allow"),
        "the allow entry matched, stderr: {stderr}"
    );
}

#[test]
fn test_check_without_violations() {
    // multi_crate has no cycles and no arc-rules.toml, use a rules file with
    // a no-cycles rule — multi_crate has no module cycles so this should pass.
    let rules = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(
        rules.path(),
        r#"
[config]
version = 1

[[rules]]
type = "no-cycles"
name = "global no-cycles"
scope = "**"
"#,
    )
    .unwrap();

    let rules_arg = format!("--rules={}", rules.path().display());
    let (code, stderr) = cargo_arc_check("multi_crate", &[&rules_arg]);
    assert_eq!(
        code, 0,
        "should exit 0 with no violations, stderr: {stderr}"
    );
}

#[test]
fn test_check_no_config_fallback() {
    // multi_crate has no arc-rules.toml → implicit cycle rule → exit 0 (no cycles)
    let (code, stderr) = cargo_arc_check("multi_crate", &[]);
    assert_eq!(
        code, 0,
        "should exit 0 under the implicit rule (no cycles), stderr: {stderr}"
    );
}

/// Without a rules file the run judges under the implicit cycle rule, so its
/// report names that rule the way it names a written one.
#[test]
fn test_check_without_a_rules_file_reports_under_the_implicit_rule() {
    let (code, stderr) = cargo_arc_check("reexport_cycle_workspace", &[]);
    assert_eq!(
        code, 1,
        "the logic cycle should fail the run, stderr: {stderr}"
    );
    assert!(
        stderr.contains("error[no-cycles]: no cycles"),
        "report should name the implicit rule, stderr: {stderr}"
    );
    // The single logic cycle gamma <-> delta gets the edge table, not one
    // edge singled out as the fewest-symbols pick.
    assert!(
        stderr.contains("cycle: delta -> gamma -> delta")
            || stderr.contains("cycle: gamma -> delta -> gamma"),
        "report should print the cycle line, stderr: {stderr}"
    );
    assert!(
        stderr.contains("edges:"),
        "report should head the edge table, stderr: {stderr}"
    );
    assert!(
        stderr.contains("gamma -> delta (on 1 cycle, 1 symbol)"),
        "report should list the gamma -> delta edge, stderr: {stderr}"
    );
    assert!(
        stderr.contains("delta -> gamma (on 1 cycle, 1 symbol)"),
        "report should list the delta -> gamma edge, stderr: {stderr}"
    );
}

/// Without this line a clean run is silent, and so is a run that never
/// happened.
#[test]
fn test_check_prints_a_status_line_for_a_rule_that_found_nothing() {
    let rules = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(
        rules.path(),
        r#"
[config]
version = 1

[[rules]]
type = "no-cycles"
name = "global no-cycles"
scope = "**"
"#,
    )
    .unwrap();

    let rules_arg = format!("--rules={}", rules.path().display());
    let (code, stdout, stderr) = cargo_arc_check_streams("multi_crate", &[&rules_arg]);
    assert_eq!(code, 0, "multi_crate has no cycles, stderr: {stderr}");
    assert!(
        stdout.contains("global no-cycles ok: 0 errors, 0 warnings, 0 allowed, 0 frozen"),
        "stdout should carry the rule's status line, stdout: {stdout}"
    );
}

#[test]
fn test_check_says_warn_for_a_warning_rule_and_still_exits_0() {
    let rules = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(
        rules.path(),
        r#"
[config]
version = 1

[[rules]]
type = "forbidden-dependency"
name = "no infra in domain"
from = "domain::**"
to = "infra::**"
severity = "warn"

# The fixture has cycles under domain and infra. This rule exists so the
# implicit one is not hung in and does not turn the run red behind the
# rule under test.
[[rules]]
type = "no-cycles"
name = "no cycles in application"
scope = "application::**"
"#,
    )
    .unwrap();

    let rules_arg = format!("--rules={}", rules.path().display());
    let (code, stdout, stderr) = cargo_arc_check_streams("arch_violation_workspace", &[&rules_arg]);
    assert_eq!(
        code, 0,
        "a warning is not a negative judgment, stderr: {stderr}"
    );
    assert!(
        stdout.contains("no infra in domain WARN: 0 errors, 1 warnings, 0 allowed, 0 frozen"),
        "stdout should state the rule warned, stdout: {stdout}"
    );
}

#[test]
fn test_check_gives_the_configuration_its_own_status_line() {
    let dir = tempfile::tempdir().unwrap();
    let rules_path = dir.path().join("arc-rules.toml");
    std::fs::write(
        &rules_path,
        r#"
[config]
version = 1

# multi_crate has no cycles, so the rule comes out clean. Its allow entry names
# a module that does not exist, and that gap is what fails the run.
[[rules]]
type = "no-cycles"
name = "global no-cycles"
scope = "**"
allow = [
  { from = "crate_a::no_such_module", to = "crate_b::gamma" },
]

[diagnostics]
unmatched-allow = "deny"
"#,
    )
    .unwrap();

    let rules_arg = format!("--rules={}", rules_path.display());
    let (code, stdout, stderr) = cargo_arc_check_streams("multi_crate", &[&rules_arg]);
    assert_eq!(
        code, 1,
        "a denied diagnostic fails the run, stderr: {stderr}"
    );
    assert!(
        stdout.contains("global no-cycles ok: 0 errors, 0 warnings, 0 allowed, 0 frozen"),
        "the rule itself came out clean, stdout: {stdout}"
    );
    assert!(
        stdout.contains("config FAILED: 1 errors, 0 warnings"),
        "stdout should state that the configuration is what failed, stdout: {stdout}"
    );
}

/// Severity is configured and status is produced, so neither reads off the
/// other.
#[test]
fn test_check_says_ok_for_an_error_rule_whose_violations_are_all_frozen() {
    let (dir, rules_path) = isolated_rules_copy("arch_violation_workspace", "arc-rules.toml");
    let rules_arg = format!("--rules={}", rules_path.display());

    let (code, stderr) = cargo_arc_check(
        "arch_violation_workspace",
        &[&rules_arg, "--generate-baseline"],
    );
    assert_eq!(code, 0, "generate should exit 0, stderr: {stderr}");
    assert!(dir.path().join("arc-baseline.toml").exists());

    let (code, stdout, stderr) = cargo_arc_check_streams("arch_violation_workspace", &[&rules_arg]);
    assert_eq!(code, 0, "everything is frozen, stderr: {stderr}");
    assert!(
        stdout.contains("no infra in domain ok: 0 errors, 0 warnings, 0 allowed, 1 frozen"),
        "an error rule with only frozen violations is ok, stdout: {stdout}"
    );
}

/// Writing a baseline records, it does not judge.
#[test]
fn test_generate_baseline_prints_no_status_line() {
    let (_dir, rules_path) = isolated_rules_copy("arch_violation_workspace", "arc-rules.toml");
    let rules_arg = format!("--rules={}", rules_path.display());

    let (code, stdout, stderr) = cargo_arc_check_streams(
        "arch_violation_workspace",
        &[&rules_arg, "--generate-baseline"],
    );
    assert_eq!(
        code, 0,
        "writing a baseline is not a judgment, stderr: {stderr}"
    );
    assert!(
        stdout.is_empty(),
        "nothing judged, so stdout stays empty, stdout: {stdout}"
    );
}

/// A rule at `ignore` is never checked, so there is no outcome to state.
#[test]
fn test_check_gives_an_ignored_rule_no_status_line() {
    let rules = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(
        rules.path(),
        r#"
[config]
version = 1

[[rules]]
type = "no-cycles"
name = "global no-cycles"
scope = "**"

[[rules]]
type = "forbidden-dependency"
name = "crate_b out of crate_a"
from = "crate_a::**"
to = "crate_b::**"
severity = "ignore"
"#,
    )
    .unwrap();

    let rules_arg = format!("--rules={}", rules.path().display());
    let (code, stdout, stderr) = cargo_arc_check_streams("multi_crate", &[&rules_arg]);
    assert_eq!(code, 0, "stderr: {stderr}");
    assert!(
        stdout.contains("global no-cycles ok:"),
        "the checked rule has its line, stdout: {stdout}"
    );
    assert!(
        !stdout.contains("crate_b out of crate_a"),
        "the ignored rule has none, stdout: {stdout}"
    );
}

#[test]
fn test_check_exits_2_when_the_graph_cannot_be_built() {
    let missing = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/no_such_workspace/Cargo.toml");
    let (code, stderr) = cargo_arc_check_at(&missing, &[]);
    assert_eq!(code, 2, "no judgment was possible, stderr: {stderr}");
}

#[test]
fn test_check_on_a_manifest_path_without_a_cargo_toml_names_the_path_and_forwards_cargos_error() {
    let dir = tempfile::tempdir().unwrap();
    let (code, stderr) = cargo_arc_check_at(dir.path(), &[]);
    assert_eq!(code, 2, "no judgment was possible, stderr: {stderr}");
    assert!(
        stderr.contains(&dir.path().display().to_string()),
        "the targeted manifest path should be named, stderr: {stderr}"
    );
    assert!(
        stderr.contains("manifest path"),
        "cargo's own message should be forwarded, stderr: {stderr}"
    );
}

#[test]
fn test_check_invalid_config() {
    let rules = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(rules.path(), "this is not valid { toml [").unwrap();

    let rules_arg = format!("--rules={}", rules.path().display());
    let (code, stderr) = cargo_arc_check("multi_crate", &[&rules_arg]);
    assert_eq!(code, 2, "should exit 2 on config error, stderr: {stderr}");
    assert!(
        stderr.contains("invalid config file"),
        "should report config parse error, stderr: {stderr}"
    );
}

// ===== Phase 5: baseline integration tests =====

#[test]
fn test_generate_baseline_then_check_reports_nothing() {
    let (dir, rules_path) = isolated_rules_copy("arch_violation_workspace", "arc-rules.toml");
    let rules_arg = format!("--rules={}", rules_path.display());
    let baseline_path = dir.path().join("arc-baseline.toml");

    let (code, stderr) = cargo_arc_check(
        "arch_violation_workspace",
        &[&rules_arg, "--generate-baseline"],
    );
    assert_eq!(code, 0, "generate should exit 0, stderr: {stderr}");
    assert!(baseline_path.exists(), "baseline file should be written");
    let baseline_before = std::fs::read_to_string(&baseline_path).unwrap();

    let (code, stderr) = cargo_arc_check("arch_violation_workspace", &[&rules_arg]);
    assert_eq!(
        code, 0,
        "check right after generate should report nothing, stderr: {stderr}"
    );
    assert!(
        !stderr.contains("error["),
        "all violations should be frozen by the baseline, stderr: {stderr}"
    );

    let baseline_after = std::fs::read_to_string(&baseline_path).unwrap();
    assert_eq!(
        baseline_before, baseline_after,
        "a normal check run must not rewrite the baseline"
    );
}

/// A baseline recording a format version this build does not speak halts an
/// ordinary run, but `--generate-baseline` rewrites it under its own version
/// rather than getting stuck on the file it is meant to replace.
#[test]
fn checking_an_unsupported_baseline_version_is_refused_but_regeneration_rewrites_it() {
    let (dir, rules_path) = isolated_rules_copy("arch_violation_workspace", "arc-rules.toml");
    let rules_arg = format!("--rules={}", rules_path.display());
    let baseline_path = dir.path().join("arc-baseline.toml");
    std::fs::write(&baseline_path, "[config]\nversion = 2\n").unwrap();

    let (code, stderr) = cargo_arc_check("arch_violation_workspace", &[&rules_arg]);
    assert_eq!(
        code, 2,
        "an unsupported baseline version must halt the run, stderr: {stderr}"
    );
    assert!(
        stderr.contains("unsupported baseline file"),
        "stderr: {stderr}"
    );

    let (code, stderr) = cargo_arc_check(
        "arch_violation_workspace",
        &[&rules_arg, "--generate-baseline"],
    );
    assert_eq!(
        code, 0,
        "regeneration must rewrite the file, stderr: {stderr}"
    );
    let baseline_after = std::fs::read_to_string(&baseline_path).unwrap();
    assert!(
        baseline_after.contains("version = 1"),
        "got:\n{baseline_after}"
    );
}

/// Turning a rule to `ignore` takes it out of the checked run, but a
/// regeneration must still keep what it had already frozen: the entries are
/// only stale once the rule is checked again and does not confirm them.
#[test]
fn regenerating_after_a_rule_turns_to_ignore_keeps_its_frozen_entries() {
    let (dir, rules_path) = isolated_rules_copy("arch_violation_workspace", "arc-rules.toml");
    let rules_arg = format!("--rules={}", rules_path.display());
    let baseline_path = dir.path().join("arc-baseline.toml");

    let (code, stderr) = cargo_arc_check(
        "arch_violation_workspace",
        &[&rules_arg, "--generate-baseline"],
    );
    assert_eq!(code, 0, "generate should exit 0, stderr: {stderr}");
    let baseline_before = std::fs::read_to_string(&baseline_path).unwrap();
    assert!(
        baseline_before.contains("no cycles in domain"),
        "precondition: the cycle rule must have frozen something, baseline:\n{baseline_before}"
    );

    let rules_content = std::fs::read_to_string(&rules_path).unwrap();
    std::fs::write(
        &rules_path,
        rules_content.replace(
            "name = \"no cycles in domain\"\nscope = \"domain::**\"\nseverity = \"error\"",
            "name = \"no cycles in domain\"\nscope = \"domain::**\"\nseverity = \"ignore\"",
        ),
    )
    .unwrap();

    let (code, stderr) = cargo_arc_check(
        "arch_violation_workspace",
        &[&rules_arg, "--generate-baseline"],
    );
    assert_eq!(
        code, 0,
        "the second generate should exit 0, stderr: {stderr}"
    );
    let baseline_after = std::fs::read_to_string(&baseline_path).unwrap();

    assert_eq!(
        baseline_before, baseline_after,
        "an ignored rule's frozen entries must survive regeneration"
    );

    let entries_written = baseline_after.matches("[[violations]]").count();
    let wrote_line = stderr
        .lines()
        .find(|line| line.starts_with("wrote "))
        .unwrap_or_else(|| panic!("expected a wrote line in stderr:\n{stderr}"));
    let wrote_count: usize = wrote_line
        .strip_prefix("wrote ")
        .and_then(|rest| rest.split_whitespace().next())
        .and_then(|n| n.parse().ok())
        .unwrap_or_else(|| panic!("could not parse the wrote line: {wrote_line:?}"));
    assert_eq!(
        wrote_count, entries_written,
        "the wrote line must count the entries actually written to the file, not the \
         violations the run collected before merging"
    );
}

/// A workspace without a rules file can freeze its cycle stock: the implicit
/// rule carries the baseline, where the earlier fallback path had none.
#[test]
fn test_generate_baseline_without_a_rules_file() {
    let (_dir, manifest) = writable_fixture_copy("reexport_cycle_workspace");
    let baseline_path = manifest.parent().unwrap().join("arc-baseline.toml");

    let (code, stderr) = cargo_arc_check_at(&manifest, &["--generate-baseline"]);
    assert_eq!(code, 0, "generate should exit 0, stderr: {stderr}");
    let baseline = std::fs::read_to_string(&baseline_path).unwrap();
    assert!(
        baseline.contains(r#"rule = "no cycles""#),
        "baseline should freeze under the implicit rule, got:\n{baseline}"
    );

    let (code, stderr) = cargo_arc_check_at(&manifest, &[]);
    assert_eq!(
        code, 0,
        "the frozen cycle stock should pass, stderr: {stderr}"
    );
}

/// `reexport_cycle_workspace` holds a `delta<->gamma` cycle whose edges the
/// baseline freezes wholesale: `--show-silenced` must still print it as one
/// tangle, not as the edges that make it up.
#[test]
fn test_show_silenced_prints_a_frozen_tangle_as_a_tangle() {
    let (_dir, manifest) = writable_fixture_copy("reexport_cycle_workspace");

    let (code, stderr) = cargo_arc_check_at(&manifest, &["--generate-baseline"]);
    assert_eq!(code, 0, "generate should exit 0, stderr: {stderr}");

    let (code, stderr) = cargo_arc_check_at(&manifest, &["--show-silenced"]);
    assert_eq!(code, 0, "the frozen tangle should pass, stderr: {stderr}");
    assert!(
        stderr.contains("silenced[no-cycles]: no cycles"),
        "got:\n{stderr}"
    );
    assert!(
        stderr
            .lines()
            .any(|line| line.contains("tangle 1/") && line.contains("(frozen)")),
        "got:\n{stderr}"
    );
    assert!(
        !stderr.lines().any(|line| line.starts_with("  = ")),
        "a frozen tangle prints as a cluster, not as a list of frozen edges, got:\n{stderr}"
    );
}

/// Copies a fixture workspace into a fresh tempdir so a test may edit its
/// sources. Returns the copy's `Cargo.toml`.
fn writable_fixture_copy(fixture: &str) -> (tempfile::TempDir, PathBuf) {
    fn copy_tree(from: &std::path::Path, to: &std::path::Path) {
        std::fs::create_dir_all(to).unwrap();
        for entry in std::fs::read_dir(from).unwrap() {
            let entry = entry.unwrap();
            let target = to.join(entry.file_name());
            if entry.file_type().unwrap().is_dir() {
                copy_tree(&entry.path(), &target);
            } else {
                std::fs::copy(entry.path(), &target).unwrap();
            }
        }
    }

    let src = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!("tests/fixtures/{fixture}"));
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join(fixture);
    copy_tree(&src, &root);
    (dir, root.join("Cargo.toml"))
}

/// Like [`cargo_arc_check`], but against a manifest anywhere on disk.
fn cargo_arc_check_at(manifest: &std::path::Path, check_args: &[&str]) -> (i32, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_cargo-arc"))
        .arg("arc")
        .arg("--manifest-path")
        .arg(manifest)
        .arg("check")
        .args(check_args)
        .output()
        .expect("failed to execute cargo-arc");
    (
        output.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&output.stderr).to_string(),
    )
}

#[test]
fn test_a_frozen_edge_turns_red_when_it_gains_a_symbol() {
    let (dir, manifest) = writable_fixture_copy("arch_violation_workspace");
    let root = manifest.parent().unwrap();
    let rules_arg = format!("--rules={}", root.join("arc-rules.toml").display());

    let (code, stderr) = cargo_arc_check_at(&manifest, &[&rules_arg, "--generate-baseline"]);
    assert_eq!(code, 0, "generate should exit 0, stderr: {stderr}");

    let (code, stderr) = cargo_arc_check_at(&manifest, &[&rules_arg]);
    assert_eq!(code, 0, "every violation is frozen, stderr: {stderr}");

    // One more symbol on the already-frozen domain::service -> infra::db edge.
    let db = root.join("infra/src/db.rs");
    let db_source = std::fs::read_to_string(&db).unwrap();
    std::fs::write(
        &db,
        format!("{db_source}\npub fn open() -> bool {{ true }}\n"),
    )
    .unwrap();
    let service = root.join("domain/src/service.rs");
    let source = std::fs::read_to_string(&service).unwrap();
    std::fs::write(&service, format!("use infra::db::open;\n{source}")).unwrap();

    let (code, stderr) = cargo_arc_check_at(&manifest, &[&rules_arg]);
    assert_eq!(
        code, 1,
        "the new symbol is outside what the entry froze, stderr: {stderr}"
    );
    assert!(
        stderr.contains("frozen for connect, an unnamed reference; now also carries open"),
        "the report should say what the entry froze and what came on top, stderr: {stderr}"
    );
    assert!(
        !stderr.contains("freezes nothing"),
        "the entry still matches its edge, stderr: {stderr}"
    );
    drop(dir);
}

/// A fixed violation must drop out of the next regeneration, and the entries
/// untouched by the fix must stay exactly as they were.
#[test]
fn regenerating_after_a_fix_drops_only_what_the_fix_touched() {
    let (_dir, manifest) = writable_fixture_copy("arch_violation_workspace");
    let root = manifest.parent().unwrap();
    let rules_arg = format!("--rules={}", root.join("arc-rules.toml").display());
    let baseline_path = root.join("arc-baseline.toml");

    let (code, stderr) = cargo_arc_check_at(&manifest, &[&rules_arg, "--generate-baseline"]);
    assert_eq!(code, 0, "generate should exit 0, stderr: {stderr}");
    let baseline_before = std::fs::read_to_string(&baseline_path).unwrap();
    assert!(
        baseline_before.contains("from = \"domain::service\"\nto = \"infra::db\""),
        "precondition: the fixed edge must start out frozen, baseline:\n{baseline_before}"
    );

    let service = root.join("domain/src/service.rs");
    let source = std::fs::read_to_string(&service).unwrap();
    std::fs::write(&service, source.replace("use infra::db;\n", "")).unwrap();

    let (code, stderr) = cargo_arc_check_at(&manifest, &[&rules_arg, "--generate-baseline"]);
    assert_eq!(
        code, 0,
        "the second generate should exit 0, stderr: {stderr}"
    );
    let baseline_after = std::fs::read_to_string(&baseline_path).unwrap();

    assert!(
        !baseline_after.contains("from = \"domain::service\"\nto = \"infra::db\""),
        "the fixed edge must be gone, baseline:\n{baseline_after}"
    );
    for line in baseline_before
        .lines()
        .filter(|l| l.contains("cycle_a") || l.contains("cycle_b"))
    {
        assert!(
            baseline_after.contains(line),
            "untouched cycle entry {line:?} must survive unchanged, baseline:\n{baseline_after}"
        );
    }
}

#[test]
fn test_generate_baseline_refuses_dead_allow() {
    let dir = tempfile::tempdir().unwrap();
    let rules_path = dir.path().join("arc-rules.toml");
    std::fs::write(
        &rules_path,
        r#"
[config]
version = 1

[[rules]]
type = "forbidden-dependency"
name = "no infra in domain"
from = "domain::**"
to = "infra::**"
allow = [
  { from = "domain::lgacy", to = "infra::db" },
]
"#,
    )
    .unwrap();
    let rules_arg = format!("--rules={}", rules_path.display());
    let baseline_path = dir.path().join("arc-baseline.toml");

    let (code, stderr) = cargo_arc_check(
        "arch_violation_workspace",
        &[&rules_arg, "--generate-baseline"],
    );
    assert_eq!(
        code, 2,
        "a dead allow entry should refuse baseline generation, stderr: {stderr}"
    );
    assert!(
        stderr.contains("domain::lgacy"),
        "stderr should name the dead pattern, stderr: {stderr}"
    );
    assert!(
        !baseline_path.exists(),
        "no baseline file should be written when generation is refused"
    );
}

/// [[id:ca-0460]] gave up on halting the run over an unreadable baseline;
/// `--generate-baseline` writing over it must not reintroduce that halt.
#[test]
fn generate_baseline_tolerates_an_unreadable_existing_baseline() {
    let dir = tempfile::tempdir().unwrap();
    let rules_path = dir.path().join("arc-rules.toml");
    std::fs::write(
        &rules_path,
        r#"
[config]
version = 1

[[rules]]
type = "forbidden-dependency"
name = "no infra in domain"
from = "domain::**"
to = "infra::**"
severity = "ignore"
"#,
    )
    .unwrap();
    let rules_arg = format!("--rules={}", rules_path.display());
    let baseline_path = dir.path().join("arc-baseline.toml");
    std::fs::write(&baseline_path, "this is not valid { toml [").unwrap();

    let (code, stderr) = cargo_arc_check(
        "arch_violation_workspace",
        &[&rules_arg, "--generate-baseline"],
    );
    assert_eq!(
        code, 0,
        "an unreadable existing baseline must not stop regeneration, stderr: {stderr}"
    );
    assert!(
        stderr.contains(&baseline_path.display().to_string()),
        "stderr should name the unreadable baseline, stderr: {stderr}"
    );

    let (code, stderr) = cargo_arc_check("arch_violation_workspace", &[&rules_arg]);
    assert_ne!(
        code, 2,
        "the ordinary run afterwards must still be able to judge, stderr: {stderr}"
    );
}

#[test]
fn test_check_duplicate_rule_names_exits_2() {
    let dir = tempfile::tempdir().unwrap();
    let rules_path = dir.path().join("arc-rules.toml");
    std::fs::write(
        &rules_path,
        r#"
[[rules]]
type = "forbidden-dependency"
name = "shared name"
from = "domain::**"
to = "infra::**"

[[rules]]
type = "no-cycles"
name = "shared name"
scope = "domain::**"
"#,
    )
    .unwrap();
    let rules_arg = format!("--rules={}", rules_path.display());

    let (code, stderr) = cargo_arc_check("arch_violation_workspace", &[&rules_arg]);
    assert_eq!(
        code, 2,
        "duplicate rule name should exit 2 at load, stderr: {stderr}"
    );
    assert!(
        stderr.contains("shared name"),
        "stderr should name the duplicate rule, stderr: {stderr}"
    );
}

// ===== Phase 6: configuration diagnostics =====

/// Rules file naming only two of the fixture's three crates as layers, so
/// `domain` is left unsorted. `exhaustive` writes `exhaustive = true` into the
/// rule; `diagnostics` is appended verbatim.
fn layers_without_domain(dir: &tempfile::TempDir, exhaustive: bool, diagnostics: &str) -> PathBuf {
    let rules_path = dir.path().join("arc-rules.toml");
    let exhaustive_line = if exhaustive {
        "exhaustive = true\n"
    } else {
        ""
    };
    std::fs::write(
        &rules_path,
        format!(
            r#"
[config]
version = 1

[[rules]]
type = "layers"
name = "architecture layers"
layers = ["infra", "application"]
direction = "top-down"
{exhaustive_line}{diagnostics}
"#
        ),
    )
    .unwrap();
    rules_path
}

#[test]
fn test_check_reports_a_crate_an_exhaustive_rule_leaves_unsorted() {
    let dir = tempfile::tempdir().unwrap();
    let rules_path = layers_without_domain(&dir, true, "");
    let rules_arg = format!("--rules={}", rules_path.display());

    let (_code, stderr) = cargo_arc_check("arch_violation_workspace", &[&rules_arg]);
    assert_eq!(
        stderr.matches("unlayered-node").count(),
        1,
        "the unsorted crate should be reported once, stderr: {stderr}"
    );
    assert!(
        stderr.contains("unlayered-node (1): domain"),
        "stderr should name the unsorted crate, stderr: {stderr}"
    );
}

#[test]
fn test_check_says_nothing_about_a_crate_when_the_rule_is_not_exhaustive() {
    let dir = tempfile::tempdir().unwrap();
    let rules_path = layers_without_domain(&dir, false, "");
    let rules_arg = format!("--rules={}", rules_path.display());

    // `arch_violation_workspace` carries other, unrelated violations of its
    // own, so the exit code stays 1 regardless; only the diagnostic is ours.
    let (_code, stderr) = cargo_arc_check("arch_violation_workspace", &[&rules_arg]);
    assert!(
        !stderr.contains("unlayered-node"),
        "stderr should not warn about a crate the rule never asserted, stderr: {stderr}"
    );
}

#[test]
fn test_check_stays_quiet_about_a_crate_on_the_except_list() {
    let dir = tempfile::tempdir().unwrap();
    let rules_path = layers_without_domain(
        &dir,
        true,
        "\n[diagnostics]\nunlayered-node = { except = [\"domain\"] }\n",
    );
    let rules_arg = format!("--rules={}", rules_path.display());

    let (_code, stderr) = cargo_arc_check("arch_violation_workspace", &[&rules_arg]);
    assert!(
        !stderr.contains("unlayered-node"),
        "a crate on the except list is deliberately outside, stderr: {stderr}"
    );
}

#[test]
fn test_check_names_a_dead_except_entry() {
    let dir = tempfile::tempdir().unwrap();
    let rules_path = layers_without_domain(
        &dir,
        true,
        "\n[diagnostics]\nunlayered-node = { except = [\"no-such-crate\"] }\n",
    );
    let rules_arg = format!("--rules={}", rules_path.display());

    let (_code, stderr) = cargo_arc_check("arch_violation_workspace", &[&rules_arg]);
    assert!(
        stderr.contains("no-such-crate"),
        "a dead except entry must be named, stderr: {stderr}"
    );
}

#[test]
fn test_check_fails_on_a_denied_diagnostic() {
    let dir = tempfile::tempdir().unwrap();
    let rules_path =
        layers_without_domain(&dir, true, "\n[diagnostics]\nunlayered-node = \"deny\"\n");
    let rules_arg = format!("--rules={}", rules_path.display());

    let (code, stderr) = cargo_arc_check("arch_violation_workspace", &[&rules_arg]);
    assert_eq!(code, 1, "deny should fail the run, stderr: {stderr}");
    assert!(
        stderr.contains("error: configuration"),
        "stderr should head the block as an error, stderr: {stderr}"
    );
}

#[test]
fn test_check_reports_an_allow_that_matches_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let rules_path = dir.path().join("arc-rules.toml");
    std::fs::write(
        &rules_path,
        r#"
[config]
version = 1

[[rules]]
type = "forbidden-dependency"
name = "no infra in domain"
from = "domain::**"
to = "infra::**"
allow = [
  { from = "domain::lgacy", to = "infra::db" },
]
"#,
    )
    .unwrap();
    let rules_arg = format!("--rules={}", rules_path.display());

    let (_code, stderr) = cargo_arc_check("arch_violation_workspace", &[&rules_arg]);
    assert!(
        stderr.contains("unmatched-allow (1): domain::lgacy"),
        "a dead allow entry should be reported in the check run, stderr: {stderr}"
    );
}

#[test]
fn test_check_refuses_a_rules_file_with_a_retired_key() {
    let dir = tempfile::tempdir().unwrap();
    let rules_path = dir.path().join("arc-rules.toml");
    std::fs::write(
        &rules_path,
        r#"
[config]
version = 1

[[rules]]
type = "no-cycles"
name = "no cycles"
scope = "**"
except = [{ from = "core::a", to = "core::b" }]
"#,
    )
    .unwrap();
    let rules_arg = format!("--rules={}", rules_path.display());

    let (code, stderr) = cargo_arc_check("arch_violation_workspace", &[&rules_arg]);
    assert_eq!(code, 2, "stderr: {stderr}");
    assert!(
        stderr.contains("`except`") && stderr.contains("allow = ["),
        "the message names the key and its replacement, stderr: {stderr}"
    );
}

#[test]
fn test_check_reports_contradictory_allow_entries() {
    let dir = tempfile::tempdir().unwrap();
    let rules_path = dir.path().join("arc-rules.toml");
    std::fs::write(
        &rules_path,
        r#"
[config]
version = 1

[[rules]]
type = "no-cycles"
name = "no cycles"
scope = "**"
allow = [
  { from = "**", to = "super" },
  { from = "**", to = "self::*" },
]
"#,
    )
    .unwrap();
    let rules_arg = format!("--rules={}", rules_path.display());

    let (code, stderr) = cargo_arc_check("arch_violation_workspace", &[&rules_arg]);
    assert_eq!(code, 1, "stderr: {stderr}");
    assert!(
        stderr.contains("contradictory-allow (3):") && stderr.contains("`** -> self::*`"),
        "stderr: {stderr}"
    );
}

#[test]
fn test_check_denies_a_rule_pattern_that_matches_nothing() {
    // `crate::` is not a prefix a rules file can use: it names no crate of the
    // workspace, so the rule reaches nothing and the run must not stay green.
    let dir = tempfile::tempdir().unwrap();
    let rules_path = dir.path().join("arc-rules.toml");
    std::fs::write(
        &rules_path,
        r#"
[config]
version = 1

[[rules]]
type = "forbidden-dependency"
name = "no infra in domain"
from = "crate::domain"
to = "infra::**"
"#,
    )
    .unwrap();
    let rules_arg = format!("--rules={}", rules_path.display());

    let (code, stderr) = cargo_arc_check("arch_violation_workspace", &[&rules_arg]);
    assert!(
        stderr.contains("unmatched-pattern (1): crate::domain"),
        "a rule pattern matching nothing should be reported, stderr: {stderr}"
    );
    assert_eq!(
        code, 1,
        "the diagnostic denies by default, so the run fails, stderr: {stderr}"
    );
}

#[test]
fn test_binding_in_a_function_body_keeps_the_neighbour_module_edge() {
    // Only `consumer::stats` binds `shared` to something else. `consumer::draw`
    // depends on the module beside it, and that edge closes a cycle with it.
    let (code, stderr) = cargo_arc_check("scoped_binding", &[]);
    assert_eq!(
        code, 1,
        "the dependency of draw on the neighbour module closes a cycle, stderr: {stderr}"
    );
    assert!(
        stderr.contains("consumer") && stderr.contains("shared"),
        "the cycle names both modules, stderr: {stderr}"
    );
}

#[test]
fn test_import_from_outside_does_not_cycle_through_a_same_named_module() {
    // Every import in my_crate::consumer names a module beside consumer.rs, and
    // each of those modules depends on consumer, so mistaking one invents a cycle.
    let (code, stderr) = cargo_arc_check("foreign_name_collision", &[]);
    assert_eq!(
        code, 0,
        "neither import is a dependency on the module beside the file, stderr: {stderr}"
    );
}

// ===== ui subcommand integration test =====

/// Kills and reaps the spawned `ui` process on drop, so a failed assertion
/// still frees the port instead of leaking the process past the test.
struct ChildGuard(Child);

impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// The first jump id under any node's `targets`, to exercise `/jump?id=`
/// against a real entry from a live service's own page.
fn first_jump_id(static_data: &Value) -> Option<u64> {
    static_data["nodes"]
        .as_object()?
        .values()
        .find_map(|node| node["targets"].as_array()?.first()?["jump"].as_u64())
}

fn http_get(port: u16, path: &str) -> (u16, String) {
    http_send(port, "GET", path, "")
}

/// Sends a raw HTTP/1.1 request over `TcpStream` and returns the status
/// code and body, mirroring `ui::server`'s own test helper of the same
/// shape.
fn http_send(port: u16, method: &str, path: &str, body: &str) -> (u16, String) {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    write!(
        stream,
        "{method} {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    )
    .unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    let status = response
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|code| code.parse().ok())
        .unwrap_or_else(|| panic!("no status line in response: {response:?}"));
    let body = response.split("\r\n\r\n").nth(1).unwrap_or("").to_string();
    (status, body)
}

#[test]
fn ui_serves_the_page_and_resolves_a_jump_id_over_http() {
    let manifest =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/multi_crate/Cargo.toml");
    let fixture_dir = manifest.parent().unwrap();

    let mut child = Command::new(env!("CARGO_BIN_EXE_cargo-arc"))
        .arg("arc")
        .arg("--manifest-path")
        .arg(&manifest)
        .arg("ui")
        .stdout(Stdio::piped())
        .spawn()
        .expect("failed to spawn cargo-arc ui");
    let mut stdout = BufReader::new(child.stdout.take().unwrap());
    let guard = ChildGuard(child);

    let mut ready_line = String::new();
    stdout.read_line(&mut ready_line).unwrap();
    let ready: Vec<&str> = ready_line.trim_end().split(' ').collect();
    assert_eq!(
        ready[..3],
        ["arc", "ready", env!("CARGO_PKG_VERSION")],
        "unexpected ready line: {ready_line:?}"
    );
    let port: u16 = ready[3].parse().expect("ready line carries a port");
    // The switches the run used follow the ready line, so the plugin knows
    // them without parsing its own command line.
    let mut analysis_line = String::new();
    stdout.read_line(&mut analysis_line).unwrap();
    assert_eq!(analysis_line, "arc analysis externals=off tests=off\n");

    let (status, body) = http_get(port, "/");
    assert_eq!(status, 200);
    let static_data = parse_static_data(&body);
    let id = first_jump_id(&static_data).expect("STATIC_DATA carries a jump target");
    for node in static_data["nodes"].as_object().unwrap().values() {
        for target in node["targets"].as_array().into_iter().flatten() {
            assert!(
                target["name"].as_str().is_some_and(|n| !n.is_empty()),
                "target should carry a non-empty name: {target:?}"
            );
        }
    }

    let (status, _) = http_get(port, &format!("/jump?id={id}"));
    assert_eq!(status, 200);

    let mut jump_line = String::new();
    stdout.read_line(&mut jump_line).unwrap();
    let jump: Vec<&str> = jump_line.trim_end().splitn(4, ' ').collect();
    assert_eq!(
        jump[..2],
        ["arc", "jump"],
        "unexpected jump line: {jump_line:?}"
    );
    let path = PathBuf::from(jump[3]);
    assert!(path.is_absolute(), "jump path is not absolute: {path:?}");
    assert!(
        path.starts_with(fixture_dir),
        "jump path {path:?} is not under the fixture directory {fixture_dir:?}"
    );

    let (status, _) = http_get(port, "/jump?id=999999");
    assert_eq!(status, 404);

    drop(guard);
    let mut rest = String::new();
    stdout.read_to_string(&mut rest).unwrap();
    assert!(
        rest.is_empty(),
        "an unknown id must not write to stdout, got: {rest:?}"
    );
}

/// Decision 7's end-to-end path: `run_ui` wires the hotspot map's own page
/// next to the arc diagram, not just `ui::server`'s own hand-built `Pages`
/// in its unit tests.
#[test]
fn ui_serves_the_hotspot_map_with_its_own_leaf_targets() {
    let manifest =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/multi_crate/Cargo.toml");

    let mut child = Command::new(env!("CARGO_BIN_EXE_cargo-arc"))
        .arg("arc")
        .arg("--manifest-path")
        .arg(&manifest)
        .arg("ui")
        .stdout(Stdio::piped())
        .spawn()
        .expect("failed to spawn cargo-arc ui");
    let mut stdout = BufReader::new(child.stdout.take().unwrap());
    let guard = ChildGuard(child);

    let mut ready_line = String::new();
    stdout.read_line(&mut ready_line).unwrap();
    let port: u16 = ready_line
        .trim_end()
        .split(' ')
        .nth(3)
        .expect("ready line carries a port")
        .parse()
        .expect("ready line carries a numeric port");
    let mut analysis_line = String::new();
    stdout.read_line(&mut analysis_line).unwrap();

    let (arc_status, arc_body) = http_get(port, "/");
    assert_eq!(arc_status, 200);
    let (hotspots_status, hotspots_body) = http_get(port, "/hotspots");
    assert_eq!(hotspots_status, 200);

    let arc_data = parse_static_data(&arc_body);
    let hotspots_data = parse_static_data(&hotspots_body);
    assert_ne!(
        arc_data, hotspots_data,
        "the hotspot map must carry its own STATIC_DATA, not the arc diagram's"
    );

    let has_leaf_target = hotspots_data["nodes"]
        .as_object()
        .expect("nodes is an object")
        .values()
        .any(|node| {
            node["kind"] == "file" && node["targets"].as_array().is_some_and(|t| !t.is_empty())
        });
    assert!(
        has_leaf_target,
        "a hotspot leaf must carry a jump target, got: {hotspots_data}"
    );

    drop(guard);
}

/// A posted switch command runs the analysis again in the same process:
/// the editor hears the new switches once the page is ready.
#[test]
fn ui_recomputes_on_a_posted_switch_command() {
    let manifest =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/multi_crate/Cargo.toml");

    let mut child = Command::new(env!("CARGO_BIN_EXE_cargo-arc"))
        .arg("arc")
        .arg("--manifest-path")
        .arg(&manifest)
        .arg("--include-tests")
        .arg("ui")
        .stdout(Stdio::piped())
        .spawn()
        .expect("failed to spawn cargo-arc ui");
    let mut stdout = BufReader::new(child.stdout.take().unwrap());
    let guard = ChildGuard(child);

    let mut ready_line = String::new();
    stdout.read_line(&mut ready_line).unwrap();
    let port: u16 = ready_line
        .trim_end()
        .split(' ')
        .nth(3)
        .unwrap()
        .parse()
        .unwrap();
    let mut analysis_line = String::new();
    stdout.read_line(&mut analysis_line).unwrap();
    assert_eq!(analysis_line, "arc analysis externals=off tests=on\n");
    let (_, body) = http_get(port, "/");
    let tests_toggle = |body: &str| {
        body.lines()
            .find(|line| line.contains("id=\"tests-toggle\""))
            .map(str::trim)
            .unwrap_or_default()
            .to_string()
    };
    assert!(
        tests_toggle(&body).contains("aria-pressed=\"true\""),
        "{}",
        tests_toggle(&body)
    );

    let (status, _) = http_send(port, "POST", "/command", "arc tests off\n");
    assert_eq!(status, 202);
    let mut analysis_line = String::new();
    stdout.read_line(&mut analysis_line).unwrap();
    assert_eq!(analysis_line, "arc analysis externals=off tests=off\n");

    let (status, body) = http_get(port, "/");
    assert_eq!(status, 200);
    assert!(
        tests_toggle(&body).contains("aria-pressed=\"false\""),
        "{}",
        tests_toggle(&body)
    );

    let (status, _) = http_send(port, "POST", "/command", "arc tests maybe\n");
    assert_eq!(status, 400);

    drop(guard);
}

/// `Command::Ui`'s help text promises that only `--output` has no effect on
/// the served page, so `--expand-level` must reach the same `STATIC_DATA`
/// field it reaches on the default (non-`ui`) path.
#[test]
fn ui_forwards_expand_level_into_static_data() {
    let manifest =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/multi_crate/Cargo.toml");

    let mut child = Command::new(env!("CARGO_BIN_EXE_cargo-arc"))
        .arg("arc")
        .arg("--manifest-path")
        .arg(&manifest)
        .arg("--expand-level")
        .arg("0")
        .arg("ui")
        .stdout(Stdio::piped())
        .spawn()
        .expect("failed to spawn cargo-arc ui");
    let mut stdout = BufReader::new(child.stdout.take().unwrap());
    let guard = ChildGuard(child);

    let mut ready_line = String::new();
    stdout.read_line(&mut ready_line).unwrap();
    let ready: Vec<&str> = ready_line.trim_end().split(' ').collect();
    let port: u16 = ready[3].parse().expect("ready line carries a port");

    let (status, body) = http_get(port, "/");
    assert_eq!(status, 200);
    let static_data = parse_static_data(&body);
    assert_eq!(
        static_data["expandLevel"],
        serde_json::json!(0),
        "--expand-level did not reach the ui page's STATIC_DATA"
    );

    drop(guard);
}

/// `reexport_mixed_cycle_workspace` has one mixed edge, `b -> a`: a pure
/// re-export (`AThing`) alongside a real, behavioral import (`value`). Only
/// `value` crosses the edge as a symbol; the re-export does not.
fn mixed_edge_entry(baseline: &str) -> &str {
    baseline
        .split("[[violations]]")
        .find(|block| {
            block.contains("from = \"my_crate::b\"") && block.contains("to = \"my_crate::a\"")
        })
        .unwrap_or_else(|| panic!("no baseline entry for the b -> a edge, baseline:\n{baseline}"))
}

#[test]
fn generate_baseline_on_a_mixed_edge_keeps_only_the_real_name() {
    let (_dir, manifest) = writable_fixture_copy("reexport_mixed_cycle_workspace");
    let baseline_path = manifest.parent().unwrap().join("arc-baseline.toml");

    let (code, stderr) = cargo_arc_check_at(&manifest, &["--generate-baseline"]);
    assert_eq!(code, 0, "generate should exit 0, stderr: {stderr}");

    let baseline = std::fs::read_to_string(&baseline_path).unwrap();
    let entry = mixed_edge_entry(&baseline);
    assert!(
        entry.contains("value"),
        "the real import must still count, baseline:\n{baseline}"
    );
    assert!(
        !entry.contains("AThing"),
        "the re-exported name must not count as a symbol crossing the edge, baseline:\n{baseline}"
    );
}

#[test]
fn a_frozen_mixed_edge_stays_green_when_a_reexported_name_is_added() {
    let (dir, manifest) = writable_fixture_copy("reexport_mixed_cycle_workspace");
    let root = manifest.parent().unwrap();

    let (code, stderr) = cargo_arc_check_at(&manifest, &["--generate-baseline"]);
    assert_eq!(code, 0, "generate should exit 0, stderr: {stderr}");

    let (code, stderr) = cargo_arc_check_at(&manifest, &[]);
    assert_eq!(
        code, 0,
        "the frozen mixed edge should pass, stderr: {stderr}"
    );

    // Add a second re-export to the already-frozen b -> a edge.
    let b = root.join("my_crate/src/b.rs");
    let source = std::fs::read_to_string(&b).unwrap();
    std::fs::write(
        &b,
        source.replace(
            "pub use crate::a::AThing;",
            "pub use crate::a::AThing;\npub use crate::a::AnotherThing;",
        ),
    )
    .unwrap();

    let (code, stderr) = cargo_arc_check_at(&manifest, &[]);
    assert_eq!(
        code, 0,
        "a re-exported name does not widen what the edge carries, stderr: {stderr}"
    );
    drop(dir);
}

/// `reexport_cycle_workspace`'s `alpha <-> beta` cycle is pure re-exports on
/// both edges (`AlphaThing`, `BetaThing`). Under `--include-reexports` an edge
/// carries only re-exports, so the exclusion in `EdgeSymbols::from_locations`
/// must not apply: both names stay.
#[test]
fn an_edge_made_only_of_reexports_keeps_every_name_with_include_reexports() {
    let (_dir, manifest) = writable_fixture_copy("reexport_cycle_workspace");
    let baseline_path = manifest.parent().unwrap().join("arc-baseline.toml");

    let output = Command::new(env!("CARGO_BIN_EXE_cargo-arc"))
        .arg("arc")
        .arg("--manifest-path")
        .arg(&manifest)
        .arg("--include-reexports")
        .arg("check")
        .arg("--generate-baseline")
        .output()
        .expect("failed to execute cargo-arc");
    let code = output.status.code().unwrap_or(-1);
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    assert_eq!(code, 0, "generate should exit 0, stderr: {stderr}");

    let baseline = std::fs::read_to_string(&baseline_path).unwrap();
    let alpha_to_beta = baseline
        .split("[[violations]]")
        .find(|block| {
            block.contains("from = \"my_crate::alpha\"")
                && block.contains("to = \"my_crate::beta\"")
        })
        .unwrap_or_else(|| {
            panic!("no baseline entry for the alpha -> beta edge, baseline:\n{baseline}")
        });
    assert!(
        alpha_to_beta.contains("BetaThing"),
        "a pure re-export edge keeps its names, baseline:\n{baseline}"
    );
}

/// `consumer::db` holds a private `use provider::store::Handle;`, and its
/// child `consumer::db::address` names the symbol through it with
/// `use super::Handle;`. The edge belongs to `provider::store`, the module
/// that defines `Handle`, the same rule already applied within one crate.
/// Today it lands on the importer's own crate instead.
#[test]
fn cross_crate_private_use_edge_attaches_to_the_definer() {
    let (temp, cmd) = fixture_args("cross_crate_reexport", false);

    let result = run(cmd);
    assert!(result.is_ok(), "run() should succeed: {result:?}");

    let svg = std::fs::read_to_string(temp.path()).unwrap();
    let arcs = extract_arcs(&svg);
    let nodes = extract_node_names(&svg);
    let named_arcs = resolve_arc_names(&arcs, &nodes);

    let has_address_to_store = named_arcs
        .iter()
        .any(|(from, to, _)| from == "address" && to == "store");
    assert!(
        has_address_to_store,
        "address -> store arc should exist (edge follows the private use to \
         the definer), found arcs: {named_arcs:?}"
    );

    let has_address_to_consumer = named_arcs
        .iter()
        .any(|(from, to, _)| from == "address" && to == "consumer");
    assert!(
        !has_address_to_consumer,
        "address -> consumer arc should NOT exist (Handle is not consumer's \
         to give), found arcs: {named_arcs:?}"
    );

    // The reference's uses move with the edge to its resolved target.
    let symbols = extract_arc_symbols(&svg);
    let key = ("address".to_string(), "store".to_string());
    assert_eq!(
        symbols.get(&key).map(Vec::as_slice),
        Some(["Handle".to_string()].as_slice()),
        "address -> store should carry the uses of Handle, found: {symbols:?}"
    );
}

/// `net` holds a private `use external_lib::Client;` to a crate outside the
/// workspace, and its child `net::endpoint` names the symbol through it with
/// `use super::Client;`. Re-export collection resolves without external
/// crates and cannot place `external_lib::Client`, so the reference is
/// neither the ancestor's binding nor the descendant's own: it produces no
/// edge. Today it stays attached to `net`, the ancestor.
#[test]
fn private_use_of_an_external_crate_gives_the_descendant_no_edge() {
    let (temp, cmd) = fixture_args("private_use_external_crate", false);

    let result = run(cmd);
    assert!(result.is_ok(), "run() should succeed: {result:?}");

    let svg = std::fs::read_to_string(temp.path()).unwrap();
    let arcs = extract_arcs(&svg);
    let nodes = extract_node_names(&svg);
    let named_arcs = resolve_arc_names(&arcs, &nodes);

    let has_endpoint_to_net = named_arcs
        .iter()
        .any(|(from, to, _)| from == "endpoint" && to == "net");
    assert!(
        !has_endpoint_to_net,
        "endpoint -> net arc should NOT exist (Client is bound elsewhere, \
         not net's to give), found arcs: {named_arcs:?}"
    );
}
