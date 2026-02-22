use cargo_arc::{Args, run};
use serde_json::Value;
use std::path::PathBuf;

/// Helper: build Args for a fixture with common defaults.
fn fixture_args(fixture: &str, include_tests: bool) -> (tempfile::NamedTempFile, Args) {
    let fixture_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(format!("tests/fixtures/{fixture}/Cargo.toml"));
    let temp = tempfile::NamedTempFile::new().unwrap();
    let args = Args {
        output: Some(temp.path().to_path_buf()),
        manifest_path: fixture_path,
        features: vec![],
        all_features: false,
        no_default_features: false,
        include_tests,
        debug: false,
        volatility: false,
        no_volatility: true,
        volatility_months: 6,
        volatility_low: 2,
        volatility_high: 10,
        #[cfg(feature = "hir")]
        hir: false,
    };
    (temp, args)
}

/// Helper: build Args for self-analysis (cargo-arc's own Cargo.toml).
fn self_args() -> (tempfile::NamedTempFile, Args) {
    let temp = tempfile::NamedTempFile::new().unwrap();
    let args = Args {
        output: Some(temp.path().to_path_buf()),
        manifest_path: PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml"),
        features: vec![],
        all_features: false,
        no_default_features: false,
        include_tests: false,
        debug: false,
        volatility: false,
        no_volatility: true,
        volatility_months: 6,
        volatility_low: 2,
        volatility_high: 10,
        #[cfg(feature = "hir")]
        hir: false,
    };
    (temp, args)
}

/// Parse STATIC_DATA JSON from SVG output.
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

/// Extract crate names that appear as nodes in the SVG STATIC_DATA.
fn extract_crate_names(svg: &str) -> Vec<String> {
    let data = parse_static_data(svg);
    let nodes = data["nodes"].as_object().expect("nodes is object");
    nodes
        .values()
        .filter(|n| n["type"] == "crate")
        .map(|n| n["name"].as_str().unwrap().to_string())
        .collect()
}

/// Extract arc entries from STATIC_DATA (from→to with is_test derived from context.kind).
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

/// Extract node-id → name mapping from STATIC_DATA.
fn extract_node_names(svg: &str) -> std::collections::HashMap<String, String> {
    let data = parse_static_data(svg);
    let nodes = data["nodes"].as_object().expect("nodes is object");
    nodes
        .iter()
        .map(|(id, n)| (id.clone(), n["name"].as_str().unwrap().to_string()))
        .collect()
}

/// Resolve arc (from_id, to_id) to (from_name, to_name).
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

#[test]
fn test_multi_crate_fixture() {
    let (temp, args) = fixture_args("multi_crate", false);

    let result = run(args);
    assert!(result.is_ok(), "run() should succeed: {:?}", result);

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
fn test_self_analysis() {
    let (temp, args) = self_args();

    let result = run(args);
    assert!(result.is_ok(), "run() should succeed: {:?}", result);

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
    let (temp, args) = fixture_args("multi_crate", false);

    let result = run(args);
    assert!(result.is_ok(), "run() should succeed: {:?}", result);

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
    let args = Args {
        output: Some(temp.path().to_path_buf()),
        manifest_path: fixture_path,
        features: vec![],
        all_features: false,
        no_default_features: false,
        include_tests: true, // --include-tests flag
        debug: false,
        volatility: false,
        no_volatility: false,
        volatility_months: 6,
        volatility_low: 2,
        volatility_high: 10,
        #[cfg(feature = "hir")]
        hir: false,
    };

    let result = run(args);
    assert!(result.is_ok(), "run() should succeed: {:?}", result);

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
    let args = Args {
        output: Some(temp.path().to_path_buf()),
        manifest_path: fixture_path,
        features: vec![],
        all_features: false,
        no_default_features: false,
        include_tests: false,
        debug: false,
        volatility: false,
        no_volatility: false,
        volatility_months: 6,
        volatility_low: 2,
        volatility_high: 10,
        #[cfg(feature = "hir")]
        hir: false,
    };

    let result = run(args);
    assert!(result.is_ok(), "run() should succeed: {:?}", result);

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

/// ca-0213: Dev-dependency crate appears as phantom node without --include-tests.
///
/// Fixture topology (dev_dep_sorting):
///   foundation  — production crate with modules (handler, service, models, common, test_support)
///   consumer    — only dev-depends on foundation + test_helper
///   test_helper — standalone test utility, no production deps
///
/// Without --include-tests:
///   - CrateDep edges from dev-dependencies should NOT appear
///   - test_helper should NOT appear (no production path)
///   - consumer should NOT appear (no production path)
///   - Only foundation with its internal module structure should remain
///
/// With --include-tests:
///   - All three crates visible
///   - consumer→foundation and consumer→test_helper arcs present
///   - foundation→test_helper arc present
#[test]
fn test_dev_dep_crate_hidden_without_include_tests() {
    let (temp, args) = fixture_args("dev_dep_sorting", false);
    let result = run(args);
    assert!(result.is_ok(), "run() should succeed: {:?}", result);

    let svg = std::fs::read_to_string(temp.path()).unwrap();
    let crates = extract_crate_names(&svg);
    let nodes = extract_node_names(&svg);
    let arcs = extract_arcs(&svg);
    let named_arcs = resolve_arc_names(&arcs, &nodes);

    // test_helper has no production consumers → should be hidden
    assert!(
        !crates.contains(&"test_helper".to_string()),
        "ca-0213: test_helper should NOT appear without --include-tests (phantom node), but found crates: {crates:?}"
    );

    // shared_lib is only reachable via test_helper's prod dep → transitive test infra → should be hidden
    assert!(
        !crates.contains(&"shared_lib".to_string()),
        "ca-0213: shared_lib should NOT appear without --include-tests (transitive dev-dep), but found crates: {crates:?}"
    );

    // consumer only has dev-deps → should be hidden too
    assert!(
        !crates.contains(&"consumer".to_string()),
        "ca-0213: consumer should NOT appear without --include-tests (only dev-deps), but found crates: {crates:?}"
    );

    // No test-context arcs should exist
    let test_arcs: Vec<_> = named_arcs
        .iter()
        .filter(|(_, _, is_test)| *is_test)
        .collect();
    assert!(
        test_arcs.is_empty(),
        "ca-0213: no test arcs should appear without --include-tests, but found: {test_arcs:?}"
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
    let (temp, args) = fixture_args("dev_dep_sorting", true);
    let result = run(args);
    assert!(result.is_ok(), "run() should succeed: {:?}", result);

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
