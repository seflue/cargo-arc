use cargo_arc::cli::CommonArgs;
use cargo_arc::{ArcCommand, run};
use serde_json::Value;
use std::path::PathBuf;
use std::process::Command;

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
        check: false,
        output: Some(temp.path().to_path_buf()),
        volatility: false,
        no_volatility: true,
        volatility_months: 6,
        volatility_low: 2,
        volatility_high: 10,
        externals: false,
        transitive_deps: false,
        expand_level: None,
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
        check: false,
        output: Some(temp.path().to_path_buf()),
        volatility: false,
        no_volatility: true,
        volatility_months: 6,
        volatility_low: 2,
        volatility_high: 10,
        externals: false,
        transitive_deps: false,
        expand_level: None,
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
        check: false,
        output: Some(temp.path().to_path_buf()),
        volatility: false,
        no_volatility: false,
        volatility_months: 6,
        volatility_low: 2,
        volatility_high: 10,
        externals: false,
        transitive_deps: false,
        expand_level: None,
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
        check: false,
        output: Some(temp.path().to_path_buf()),
        volatility: false,
        no_volatility: false,
        volatility_months: 6,
        volatility_low: 2,
        volatility_high: 10,
        externals: false,
        transitive_deps: false,
        expand_level: None,
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
#[test]
fn test_glob_import_carries_payload() {
    let (temp, cmd) = fixture_args("reexport_workspace", false);

    let result = run(cmd);
    assert!(result.is_ok(), "run() should succeed: {result:?}");

    let svg = std::fs::read_to_string(temp.path()).unwrap();
    let symbols = extract_arc_symbols(&svg);

    let key = ("glob_user".to_string(), "sibling".to_string());
    assert_eq!(
        symbols.get(&key).map(Vec::as_slice),
        Some(["Extra".to_string(), "Widget".to_string()].as_slice()),
        "glob_user -> sibling should carry sibling's exports, found: {symbols:?}"
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

// ===== Phase 4: check subcommand integration tests =====

/// Copies `<fixture>/arc-rules.toml` into a fresh tempdir so a `--generate-baseline`
/// run writes `arc-baseline.toml` there instead of into the checked-in fixture.
fn isolated_rules_copy(fixture: &str) -> (tempfile::TempDir, PathBuf) {
    let src = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(format!("tests/fixtures/{fixture}/arc-rules.toml"));
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("arc-rules.toml");
    std::fs::copy(&src, &dest).unwrap();
    (dir, dest)
}

/// Run `cargo-arc arc --manifest-path <fixture>/Cargo.toml check [check_args...]` as subprocess.
/// Returns (`exit_code`, stderr).
fn cargo_arc_check(fixture: &str, check_args: &[&str]) -> (i32, String) {
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
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    (code, stderr)
}

/// Run `cargo-arc arc --manifest-path <fixture>/Cargo.toml --check` (legacy flag) as subprocess.
fn cargo_arc_legacy_check(fixture: &str) -> (i32, String) {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(format!("tests/fixtures/{fixture}/Cargo.toml"));
    let output = Command::new(env!("CARGO_BIN_EXE_cargo-arc"))
        .arg("arc")
        .arg("--manifest-path")
        .arg(&manifest)
        .arg("--check")
        .output()
        .expect("failed to execute cargo-arc");
    let code = output.status.code().unwrap_or(-1);
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    (code, stderr)
}

/// Run legacy `--check`, optionally with `--include-reexports`. That flag is a
/// common-level argument, so it precedes `--check` on the command line.
fn cargo_arc_legacy_check_opts(fixture: &str, include_reexports: bool) -> (i32, String) {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(format!("tests/fixtures/{fixture}/Cargo.toml"));
    let mut command = Command::new(env!("CARGO_BIN_EXE_cargo-arc"));
    command.arg("arc").arg("--manifest-path").arg(&manifest);
    if include_reexports {
        command.arg("--include-reexports");
    }
    command.arg("--check");
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
    let (code, stderr) = cargo_arc_legacy_check_opts("reexport_cycle_workspace", false);
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
    let (code, stderr) = cargo_arc_legacy_check_opts("reexport_cycle_workspace", true);
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

/// Reads the `stderr` layout of `format::violation_block`: diagnostic blocks
/// separated by a blank line, each carrying its edge on a `  = ` line.
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
    // multi_crate has no arc-rules.toml → legacy global cycle check → exit 0 (no cycles)
    let (code, stderr) = cargo_arc_check("multi_crate", &[]);
    assert_eq!(
        code, 0,
        "should exit 0 with legacy fallback (no cycles), stderr: {stderr}"
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
    let (dir, rules_path) = isolated_rules_copy("arch_violation_workspace");
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

#[test]
fn test_generate_baseline_refuses_dead_except() {
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
except = [
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
        "a dead except should refuse baseline generation, stderr: {stderr}"
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
/// `domain` is left unsorted. `diagnostics` is appended verbatim.
fn layers_without_domain(dir: &tempfile::TempDir, diagnostics: &str) -> PathBuf {
    let rules_path = dir.path().join("arc-rules.toml");
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
{diagnostics}
"#
        ),
    )
    .unwrap();
    rules_path
}

#[test]
fn test_check_reports_a_crate_no_layer_covers() {
    let dir = tempfile::tempdir().unwrap();
    let rules_path = layers_without_domain(&dir, "");
    let rules_arg = format!("--rules={}", rules_path.display());

    let (_code, stderr) = cargo_arc_check("arch_violation_workspace", &[&rules_arg]);
    assert_eq!(
        stderr.matches("unlayered-crate").count(),
        1,
        "the unsorted crate should be reported once, stderr: {stderr}"
    );
    assert!(
        stderr.contains("unlayered-crate: domain"),
        "stderr should name the unsorted crate, stderr: {stderr}"
    );
}

#[test]
fn test_check_stays_quiet_about_a_crate_on_the_except_list() {
    let dir = tempfile::tempdir().unwrap();
    let rules_path = layers_without_domain(
        &dir,
        "\n[diagnostics]\nunlayered-crate = { except = [\"domain\"] }\n",
    );
    let rules_arg = format!("--rules={}", rules_path.display());

    let (_code, stderr) = cargo_arc_check("arch_violation_workspace", &[&rules_arg]);
    assert!(
        !stderr.contains("unlayered-crate"),
        "a crate on the except list is deliberately outside, stderr: {stderr}"
    );
}

#[test]
fn test_check_fails_on_a_denied_diagnostic() {
    let dir = tempfile::tempdir().unwrap();
    let rules_path = layers_without_domain(&dir, "\n[diagnostics]\nunlayered-crate = \"deny\"\n");
    let rules_arg = format!("--rules={}", rules_path.display());

    let (code, stderr) = cargo_arc_check("arch_violation_workspace", &[&rules_arg]);
    assert_eq!(code, 1, "deny should fail the run, stderr: {stderr}");
    assert!(
        stderr.contains("error: configuration"),
        "stderr should head the block as an error, stderr: {stderr}"
    );
}

#[test]
fn test_check_reports_an_except_that_matches_nothing() {
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
except = [
  { from = "domain::lgacy", to = "infra::db" },
]
"#,
    )
    .unwrap();
    let rules_arg = format!("--rules={}", rules_path.display());

    let (_code, stderr) = cargo_arc_check("arch_violation_workspace", &[&rules_arg]);
    assert!(
        stderr.contains("unmatched-except: domain::lgacy"),
        "a dead except should be reported in the check run, stderr: {stderr}"
    );
}

#[test]
fn test_legacy_check_flag() {
    // --check flag on a fixture without cycles → exit 0
    let (code, stderr) = cargo_arc_legacy_check("multi_crate");
    assert_eq!(
        code, 0,
        "legacy --check should work (no cycles → exit 0), stderr: {stderr}"
    );
}
