//! SVG and `STATIC_DATA` for the hotspot map. Both the circles and the data
//! come from the same `Vec<PackedCircle>` `hotspots::pack::pack` produced, the
//! way `render::render` feeds both paths from one `Vec<PositionedItem>`.

use super::constants::{CSS, LAYOUT, RenderConfig};
use super::css::render_styles;
use super::elements::escape_xml;
use super::static_data::{TargetData, ThemeData, script_element};
use super::theme::{ColorPalette, Theme};
use crate::hotspots::{
    GreyCause, HotspotKind, HotspotNode, HotspotTree, PackedCircle, sorted_children,
};
use crate::layout::{LocationId, TargetKind};
use serde::Serialize;
use std::collections::{BTreeMap, HashMap};
use std::fmt::Write as _;
use std::path::PathBuf;

/// The root circle's radius in SVG pixels; every other circle scales with it.
const TARGET_ROOT_RADIUS: f64 = 380.0;
const MAP_MARGIN: f64 = 20.0;
/// Width of the sidebar and the gap before it, a strip of their own beside
/// the packed circle so the sidebar never covers a circle.
const SIDEBAR_WIDTH: f64 = 280.0;
const SIDEBAR_GAP: f64 = 20.0;

// === Serialization structs ===

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct HotspotStaticData {
    nodes: BTreeMap<String, HotspotCircleData>,
    /// The workspace's top-N leaves, rank order, as keys into `nodes`.
    hotspots: Vec<String>,
    max_file_commits: usize,
    total_commits: usize,
    /// The sidebar's explanation line while the map is grey for lack of
    /// volatility data; absent once volatility is drawn.
    #[serde(skip_serializing_if = "Option::is_none")]
    volatility: Option<String>,
    theme: ThemeData,
    /// The hotspot CSS class names JS also builds markup with, one place
    /// instead of a literal string repeated in Rust and JS.
    classes: BTreeMap<String, String>,
    /// The sizes the JS layout resizes the map with, read here instead of
    /// repeated as literals in JS.
    layout: HotspotLayoutData,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct HotspotLayoutData {
    sidebar_width: f64,
    sidebar_gap: f64,
    map_margin: f64,
    toolbar_height: f32,
}

impl HotspotLayoutData {
    fn current() -> Self {
        Self {
            sidebar_width: SIDEBAR_WIDTH,
            sidebar_gap: SIDEBAR_GAP,
            map_margin: MAP_MARGIN,
            toolbar_height: LAYOUT.toolbar.height,
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct HotspotCircleData {
    kind: HotspotKind,
    name: String,
    /// Workspace-relative; a container's own declaring file for a `Module`
    /// or `Crate`, empty for the synthetic workspace root.
    file: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    parent: Option<String>,
    cx: f64,
    cy: f64,
    r: f64,
    lines: usize,
    commits: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    rank: Option<usize>,
    /// The percent `circle_style` fills this circle with; the sidebar bars
    /// read it instead of recomputing the scale.
    fill_percent: i32,
    /// The leaf's jump target, under `config.with_jump_ids` and only when
    /// `targets` has an id for `file`; JS draws the jump icon from it.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    targets: Vec<TargetData>,
}

// === Tree walk shared by the circles and the data ===

/// Return `tree`'s nodes in the order `pack` packed them, each with its
/// parent's index in the result (`None` for the root).
fn ordered_with_parents(tree: &HotspotTree) -> Vec<(&HotspotNode, Option<usize>)> {
    let mut out = Vec::new();
    walk(&tree.root, None, &mut out);
    out
}

fn walk<'a>(
    node: &'a HotspotNode,
    parent: Option<usize>,
    out: &mut Vec<(&'a HotspotNode, Option<usize>)>,
) {
    let index = out.len();
    out.push((node, parent));
    for child in sorted_children(node) {
        walk(child, Some(index), out);
    }
}

/// Return one `STATIC_DATA.nodes` key per node: its file, suffixed `#leaf`,
/// `#leaf2`, ... on repeats. A container comes before its own leaf.
fn node_keys(ordered: &[(&HotspotNode, Option<usize>)]) -> Vec<String> {
    let mut occurrences: HashMap<String, usize> = HashMap::new();
    ordered
        .iter()
        .map(|(node, _)| {
            let base = node.file.to_string_lossy().into_owned();
            let count = occurrences.entry(base.clone()).or_insert(0);
            *count += 1;
            match *count {
                1 => base,
                2 => format!("{base}#leaf"),
                n => format!("{base}#leaf{n}", n = n - 1),
            }
        })
        .collect()
}

/// Return what a `kind` node's commits are scaled against: the busiest file
/// for a file, the whole workspace for a container, which spans many files.
fn reference_commits(tree: &HotspotTree, kind: HotspotKind) -> usize {
    match kind {
        HotspotKind::File => tree.max_file_commits,
        HotspotKind::Workspace | HotspotKind::Crate | HotspotKind::Module => tree.total_commits,
    }
}

/// Return `commits` against `reference` on a `sqrt` scale as a rounded
/// percent; the circles and the sidebar bars both fill by it.
#[allow(clippy::cast_precision_loss)] // commit counts stay well below 2^52
fn fill_percent(commits: usize, reference: usize) -> i32 {
    let reference = reference.max(1);
    let t = ((commits as f64).sqrt() / (reference as f64).sqrt()).clamp(0.0, 1.0);
    #[allow(clippy::cast_possible_truncation)] // t in [0, 1], pct in [0, 100]
    let pct = (t * 100.0).round() as i32;
    pct
}

/// Return a fill that mixes the theme's cold and hot variables by
/// `fill_percent`, so it follows the light or dark theme.
fn circle_style(commits: usize, reference: usize) -> String {
    let pct = fill_percent(commits, reference);
    let hotspot = ColorPalette::VARS.hotspots;
    format!(
        "fill:color-mix(in srgb, {} {pct}%, {})",
        hotspot.hot, hotspot.cold
    )
}

/// Return the SVG root at 100% of its container, which `hotspot_layout.js`
/// fills to the window; `viewBox` keeps the size for a file opened without JS.
fn render_hotspot_header(width: f32, height: f32, theme: Option<&Theme>) -> String {
    let pinned = theme.map_or(String::new(), |theme| {
        format!(" data-theme=\"{}\"", theme.name)
    });
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<svg xmlns="http://www.w3.org/2000/svg"{pinned} class="{}" width="100%" height="100%" viewBox="0 0 {width} {height}">
"#,
        CSS.relation.cluster_mode_on
    )
}

/// Render the toolbar shared with the arc page, with a link to it and no
/// buttons of the map's own.
fn render_toolbar(width: f32, config: &RenderConfig) -> String {
    let content = super::toolbar::Content::default();
    // The toolbar's link to the arc page; JS updates its `href` to carry
    // the currently selected leaf as `?select=<file>`.
    let cross_link = super::toolbar::CrossLink {
        id: "arc-page-link",
        href: "/",
        label: "Arc diagram",
    };
    super::toolbar::render(width, config, &content, cross_link)
}

/// Return `node`'s `<circle>` with its classes, and its inline fill unless
/// the map is grey.
fn circle_svg(
    node: &HotspotNode,
    key: &str,
    cx: f64,
    cy: f64,
    r: f64,
    grey: bool,
    tree: &HotspotTree,
) -> String {
    let mut classes = vec![CSS.hotspots.circle];
    if node.kind == HotspotKind::File {
        classes.push(CSS.hotspots.leaf);
    }
    if grey {
        classes.push(CSS.hotspots.grey);
    }
    if tree.rank_of(node).is_some() {
        classes.push(CSS.hotspots.outline);
    }
    let style = if grey {
        String::new()
    } else {
        format!(
            " style=\"{}\"",
            circle_style(node.commits, reference_commits(tree, node.kind))
        )
    };
    format!(
        "    <circle class=\"{}\" data-file=\"{}\" cx=\"{cx:.2}\" cy=\"{cy:.2}\" r=\"{r:.2}\"{style}/>\n",
        classes.join(" "),
        escape_xml(key),
    )
}

/// Return `node`'s label `<text>`, tied to its circle by `data-label` since
/// labels sit in a group of their own.
fn label_svg(node: &HotspotNode, key: &str, cx: f64, cy: f64) -> String {
    format!(
        "    <text class=\"{}\" data-label=\"{}\" x=\"{cx:.2}\" y=\"{cy:.2}\">{}</text>\n",
        CSS.hotspots.label,
        escape_xml(key),
        escape_xml(&node.name),
    )
}

/// Return the jump target of a file `node` that `targets` has an id for, under
/// `config.with_jump_ids`. Its kind is `Module`: a leaf is never a lib or bin.
fn leaf_targets(
    node: &HotspotNode,
    targets: &HashMap<PathBuf, LocationId>,
    config: &RenderConfig,
) -> Vec<TargetData> {
    if !config.with_jump_ids || node.kind != HotspotKind::File {
        return Vec::new();
    }
    targets
        .get(&node.file)
        .map(|&jump| {
            vec![TargetData {
                kind: TargetKind::Module,
                name: node.name.clone(),
                jump,
            }]
        })
        .unwrap_or_default()
}

fn node_data(
    node: &HotspotNode,
    tree: &HotspotTree,
    parent: Option<&str>,
    cx: f64,
    cy: f64,
    r: f64,
    targets: Vec<TargetData>,
) -> HotspotCircleData {
    HotspotCircleData {
        kind: node.kind,
        name: node.name.clone(),
        file: node.file.to_string_lossy().into_owned(),
        parent: parent.map(str::to_string),
        cx,
        cy,
        r,
        lines: node.lines,
        commits: node.commits,
        rank: tree.rank_of(node),
        fill_percent: fill_percent(node.commits, reference_commits(tree, node.kind)),
        targets,
    }
}

/// Return the sidebar line explaining why the map is grey.
fn volatility_message(cause: GreyCause) -> String {
    match cause {
        GreyCause::Flag => {
            "Volatility disabled: run without --no-volatility to see commit activity."
                .to_string()
        }
        GreyCause::GitUnavailable => {
            "Volatility unavailable: this workspace is not a git repository, or git could not be run."
                .to_string()
        }
        GreyCause::NoCommitsInWindow { months } => format!(
            "Volatility unavailable: no commits touched this workspace in the last {months} months."
        ),
    }
}

/// Render the sidebar: details panel, ranked list, list/bars toggle and grey
/// note. `hotspot_script.js` fills in the details and drives the toggle.
fn render_hotspot_sidebar(
    width: f32,
    height: f32,
    nodes: &BTreeMap<String, HotspotCircleData>,
    hotspots: &[String],
    volatility: Option<&str>,
) -> String {
    let hs = &CSS.hotspots;
    let mut list = String::new();
    for (index, key) in hotspots.iter().enumerate() {
        let Some(node) = nodes.get(key) else { continue };
        let _ = writeln!(
            list,
            "        <li class=\"{}\" data-file=\"{}\">{}. {} — {}×{}</li>",
            hs.list_item,
            escape_xml(key),
            index + 1,
            escape_xml(&node.name),
            node.lines,
            node.commits,
        );
    }
    let note = volatility.map_or_else(String::new, |message| {
        format!(
            "      <div id=\"hotspot-volatility-note\" class=\"{}\">{}</div>\n",
            hs.note,
            escape_xml(message),
        )
    });
    // The ranking behind the toggle is empty in the same case the note
    // explains it (no churn data), so the toggle is omitted there too.
    let toggle = if volatility.is_none() {
        format!(
            "    <button id=\"hotspot-list-toggle\" class=\"{}\" aria-pressed=\"false\">Bars</button>\n",
            hs.list_toggle,
        )
    } else {
        String::new()
    };

    #[allow(clippy::cast_possible_truncation)] // SVG pixel coordinates fit in i32
    let sidebar_width = SIDEBAR_WIDTH as f32;
    #[allow(clippy::cast_possible_truncation)]
    let x = (width - sidebar_width) as i32;
    format!(
        concat!(
            "<foreignObject id=\"hotspot-sidebar\" x=\"{}\" y=\"0\" width=\"{}\" height=\"{}\"",
            " style=\"display:none; overflow:visible\">\n",
            "  <div class=\"{}\" xmlns=\"http://www.w3.org/1999/xhtml\">\n",
            "    <div id=\"hotspot-details\" class=\"{}\"></div>\n",
            "{}",
            "    <ul id=\"hotspot-list\" class=\"{}\">\n",
            "{}",
            "    </ul>\n",
            "{}",
            "  </div>\n",
            "</foreignObject>\n",
        ),
        x, sidebar_width, height, hs.sidebar, hs.details, toggle, hs.list, list, note,
    )
}

/// Render the map's circles and `STATIC_DATA`, both from `packed` (ADR-006).
/// `targets` maps a leaf's file to its jump id, read under `with_jump_ids`.
///
/// # Panics
/// In debug builds, if `packed` is not one circle per node of `tree` in
/// `pack`'s order.
#[must_use]
pub(crate) fn render(
    tree: &HotspotTree,
    packed: &[PackedCircle],
    targets: &HashMap<PathBuf, LocationId>,
    config: &RenderConfig,
) -> String {
    let ordered = ordered_with_parents(tree);
    debug_assert_eq!(
        ordered.len(),
        packed.len(),
        "pack() must yield one circle per tree node, in the same preorder"
    );
    let keys = node_keys(&ordered);
    let grey = config.grey_cause.is_some();

    let root_r = packed.first().map_or(1.0, |circle| circle.r.max(1.0));
    let scale = TARGET_ROOT_RADIUS / root_r;
    let cx0 = MAP_MARGIN + TARGET_ROOT_RADIUS;
    let cy0 = f64::from(LAYOUT.toolbar.height) + MAP_MARGIN + TARGET_ROOT_RADIUS;
    #[allow(clippy::cast_possible_truncation)] // canvas size stays well below 2^23
    let width = (cx0 + TARGET_ROOT_RADIUS + MAP_MARGIN + SIDEBAR_GAP + SIDEBAR_WIDTH) as f32;
    #[allow(clippy::cast_possible_truncation)]
    let height = (cy0 + TARGET_ROOT_RADIUS + MAP_MARGIN) as f32;

    let mut circles = String::new();
    let mut labels = String::new();
    let mut nodes = BTreeMap::new();
    let mut hotspot_ranks: Vec<(usize, String)> = Vec::new();

    for (index, (node, parent)) in ordered.iter().enumerate() {
        let circle = &packed[index];
        let cx = cx0 + circle.cx * scale;
        let cy = cy0 + circle.cy * scale;
        let r = circle.r * scale;
        let key = &keys[index];

        circles.push_str(&circle_svg(node, key, cx, cy, r, grey, tree));
        labels.push_str(&label_svg(node, key, cx, cy));

        if let Some(rank) = tree.rank_of(node) {
            hotspot_ranks.push((rank, key.clone()));
        }
        let parent_key = parent.map(|p| keys[p].as_str());
        nodes.insert(
            key.clone(),
            node_data(
                node,
                tree,
                parent_key,
                cx,
                cy,
                r,
                leaf_targets(node, targets, config),
            ),
        );
    }
    hotspot_ranks.sort_by_key(|(rank, _)| *rank);
    let hotspots: Vec<String> = hotspot_ranks.into_iter().map(|(_, key)| key).collect();

    let volatility = config.grey_cause.map(volatility_message);
    let sidebar = render_hotspot_sidebar(width, height, &nodes, &hotspots, volatility.as_deref());
    let hs = &CSS.hotspots;
    let classes: BTreeMap<String, String> = [
        ("circle", hs.circle),
        ("listItem", hs.list_item),
        ("hover", hs.hover),
        ("selected", hs.selected),
        ("barLabel", hs.bar_label),
        ("barTrack", hs.bar_track),
        ("barFill", hs.bar_fill),
        ("detailsTitle", hs.details_title),
        ("tooltip", hs.tooltip),
        ("jumpIcon", hs.jump_icon),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect();
    let static_data = HotspotStaticData {
        nodes,
        hotspots,
        max_file_commits: tree.max_file_commits,
        total_commits: tree.total_commits,
        volatility,
        theme: ThemeData::current(),
        classes,
        layout: HotspotLayoutData::current(),
    };
    let static_data_js = format!(
        "const STATIC_DATA = {};",
        serde_json::to_string(&static_data).expect("HotspotStaticData serialization cannot fail")
    );

    let mut svg = String::new();
    svg.push_str(&render_hotspot_header(width, height, config.theme));
    svg.push_str(&render_styles());
    svg.push_str("  <g id=\"map-content\">\n");
    svg.push_str(&circles);
    // Its own group, after every circle, so a container's label never sits
    // under a child's circle (a later group paints over an earlier one).
    svg.push_str("    <g id=\"map-labels\">\n");
    svg.push_str(&labels);
    svg.push_str("    </g>\n");
    svg.push_str("  </g>\n");
    svg.push_str(&render_toolbar(width, config));
    svg.push_str(&sidebar);
    svg.push_str(&script_element(static_data_js, "hotspot_script", config));
    svg.push_str("</svg>\n");
    svg
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::{ArcGraph, EdgeWeight};
    use crate::hotspots::pack;
    use crate::model::TargetRoots;
    use crate::test_support::{crate_node_with_targets, module_node_with_file};
    use std::collections::BTreeSet;
    use std::path::{Path, PathBuf};

    fn static_data_of(svg: &str) -> serde_json::Value {
        let json_str = svg
            .split("const STATIC_DATA = ")
            .nth(1)
            .unwrap()
            .split(";\n")
            .next()
            .unwrap();
        serde_json::from_str(json_str).expect("valid JSON")
    }

    fn single_crate_tree() -> HotspotTree {
        let mut graph = ArcGraph::new();
        let crate_idx = graph.add_node(crate_node_with_targets(
            "app",
            TargetRoots {
                lib_root: Some(PathBuf::from("/ws/app/src/lib.rs")),
                bin_roots: vec![],
            },
            "/ws/app/Cargo.toml",
        ));
        let hot_idx = graph.add_node(module_node_with_file(
            "hot",
            crate_idx,
            "/ws/app/src/hot.rs",
        ));
        graph.add_edge(crate_idx, hot_idx, EdgeWeight::Contains);
        crate::hotspots::build(&graph, Path::new("/ws"), |_| 100, |_| BTreeSet::new(), 10)
    }

    /// `single_crate_tree()`, packed and rendered with `HashMap::new()`
    /// targets and `RenderConfig::default()`: the setup most tests in this
    /// module need before inspecting the tree, the packed circles or the
    /// SVG.
    fn rendered_default() -> (HotspotTree, Vec<PackedCircle>, String) {
        let tree = single_crate_tree();
        let packed = pack(&tree);
        let svg = render(&tree, &packed, &HashMap::new(), &RenderConfig::default());
        (tree, packed, svg)
    }

    /// `single_crate_tree()` rendered grey, as `--no-volatility` would.
    fn rendered_grey() -> String {
        let tree = single_crate_tree();
        let config = RenderConfig {
            grey_cause: Some(GreyCause::Flag),
            ..RenderConfig::default()
        };
        render(&tree, &pack(&tree), &HashMap::new(), &config)
    }

    /// The root element sizes itself to its container so JS can grow the
    /// map to the browser window; `viewBox` keeps the content-derived pixel
    /// size as the fallback for a file opened without JS.
    #[test]
    fn the_svg_root_fills_its_container_and_keeps_a_pixel_viewbox() {
        let (_, _, svg) = rendered_default();

        assert!(
            svg.contains("width=\"100%\" height=\"100%\" viewBox=\"0 0 1100 840\">"),
            "got: {svg}"
        );
    }

    /// The circle geometry this work must leave alone: the root always
    /// scales to `TARGET_ROOT_RADIUS` and sits at `(MAP_MARGIN +
    /// TARGET_ROOT_RADIUS, toolbar height + MAP_MARGIN + TARGET_ROOT_RADIUS)`
    /// regardless of the browser window - only the viewBox, the furniture
    /// and the view transform become dynamic.
    #[test]
    fn the_root_circles_emitted_geometry_is_unchanged() {
        let (_, _, svg) = rendered_default();
        let data = static_data_of(&svg);

        let root = &data["nodes"]["app/Cargo.toml"];
        assert_eq!(root["cx"], 400.0);
        assert_eq!(root["cy"], 440.0);
        assert_eq!(root["r"], 380.0);
    }

    #[test]
    fn render_draws_one_circle_per_packed_node() {
        let (_, packed, svg) = rendered_default();

        assert_eq!(
            svg.matches("<circle").count(),
            packed.len(),
            "one <circle> per packed node"
        );
    }

    /// A container's label must never sit under a child's circle: every
    /// `<text>` label is drawn after (so, in SVG paint order, on top of)
    /// every `<circle>`, regardless of tree depth.
    #[test]
    fn every_label_is_drawn_above_every_circle() {
        let (_, _, svg) = rendered_default();

        let last_circle = svg.rfind("<circle").expect("a circle exists");
        let first_label = svg.find("<text").expect("a label exists");
        assert!(
            first_label > last_circle,
            "every label must follow every circle in document order, got: {svg}"
        );
    }

    /// The `<circle>` tag whose `data-file` is `key`, for asserting on its
    /// own class list independent of the other classes it may also carry.
    fn circle_tag_for<'a>(svg: &'a str, key: &str) -> &'a str {
        let marker = format!("data-file=\"{key}\"");
        let end = svg.find(&marker).expect("a circle with this data-file");
        let start = svg[..end].rfind("<circle").expect("the tag's own start");
        &svg[start..end]
    }

    /// A leaf's circle carries the `leaf` class so its fill-opacity can
    /// stand out from a container's, distinct from `outline` (a ranked
    /// hotspot only) and `grey` (no volatility data).
    #[test]
    fn a_leafs_circle_carries_the_leaf_class_a_containers_does_not() {
        let (_, _, svg) = rendered_default();

        assert!(
            circle_tag_for(&svg, "app/src/hot.rs").contains(CSS.hotspots.leaf),
            "the leaf's circle must carry the leaf class, got: {svg}"
        );
        assert!(
            !circle_tag_for(&svg, "app/Cargo.toml").contains(CSS.hotspots.leaf),
            "the container's circle must not carry the leaf class, got: {svg}"
        );
    }

    #[test]
    fn a_labels_data_label_matches_its_circles_data_file() {
        let (_, _, svg) = rendered_default();

        assert!(
            svg.contains("data-label=\"app/Cargo.toml\""),
            "the crate's own label carries its node key, got: {svg}"
        );
        assert!(
            svg.contains("data-label=\"app/src/hot.rs\""),
            "the leaf's label carries its node key, got: {svg}"
        );
    }

    #[test]
    fn render_keys_static_data_nodes_by_workspace_relative_file() {
        let (_, _, svg) = rendered_default();
        let data = static_data_of(&svg);

        let crate_node = &data["nodes"]["app/Cargo.toml"];
        assert_eq!(crate_node["kind"], "crate");
        assert_eq!(crate_node["name"], "app");

        let leaf = &data["nodes"]["app/src/hot.rs"];
        assert_eq!(leaf["kind"], "file");
        assert_eq!(leaf["parent"], "app/Cargo.toml");
        assert_eq!(leaf["lines"], 100);
    }

    /// The JS layout module reads the sidebar, gap, margin and toolbar
    /// height off `STATIC_DATA` instead of repeating them as literals
    /// beside their Rust constants (the two-path rule, ADR-006).
    #[test]
    fn static_data_carries_the_layout_constants_js_needs_to_resize() {
        let (_, _, svg) = rendered_default();
        let data = static_data_of(&svg);

        assert_eq!(data["layout"]["sidebarWidth"], SIDEBAR_WIDTH);
        assert_eq!(data["layout"]["sidebarGap"], SIDEBAR_GAP);
        assert_eq!(data["layout"]["mapMargin"], MAP_MARGIN);
        assert_eq!(
            data["layout"]["toolbarHeight"],
            f64::from(LAYOUT.toolbar.height)
        );
    }

    #[test]
    fn a_containers_own_self_leaf_gets_a_disambiguated_key() {
        let mut graph = ArcGraph::new();
        let crate_idx = graph.add_node(crate_node_with_targets(
            "app",
            TargetRoots::default(),
            "/ws/app/Cargo.toml",
        ));
        let outer_idx = graph.add_node(module_node_with_file(
            "outer",
            crate_idx,
            "/ws/app/src/outer/mod.rs",
        ));
        graph.add_edge(crate_idx, outer_idx, EdgeWeight::Contains);
        let inner_idx = graph.add_node(module_node_with_file(
            "inner",
            crate_idx,
            "/ws/app/src/outer/inner.rs",
        ));
        graph.add_edge(outer_idx, inner_idx, EdgeWeight::Contains);
        let tree = crate::hotspots::build(&graph, Path::new("/ws"), |_| 5, |_| BTreeSet::new(), 10);
        let packed = pack(&tree);
        let svg = render(&tree, &packed, &HashMap::new(), &RenderConfig::default());
        let data = static_data_of(&svg);

        let container = &data["nodes"]["app/src/outer/mod.rs"];
        assert_eq!(container["kind"], "module");
        let self_leaf = &data["nodes"]["app/src/outer/mod.rs#leaf"];
        assert_eq!(self_leaf["kind"], "file");
        assert_eq!(self_leaf["parent"], "app/src/outer/mod.rs");
    }

    #[test]
    fn without_volatility_the_map_is_grey_with_an_explanatory_line() {
        let svg = rendered_grey();
        let data = static_data_of(&svg);

        assert!(data["volatility"].as_str().is_some_and(|s| !s.is_empty()));
        assert!(
            svg.contains(CSS.hotspots.grey),
            "every circle carries the grey class"
        );
        // The embedded script's own source legitimately contains
        // "color-mix" as JS text (the sidebar's bars view builds its fill
        // the same way); only the markup before it - the circles
        // themselves - must have none.
        let markup = svg.split("<script").next().unwrap_or(&svg);
        assert!(
            !markup.contains("color-mix"),
            "a grey map has no per-node fill computation"
        );
    }

    /// The sidebar note names which of the three causes made the map grey,
    /// instead of one message covering all of them.
    #[test]
    fn the_volatility_note_names_the_flag_as_the_cause_when_given() {
        let tree = single_crate_tree();
        let packed = pack(&tree);
        let config = RenderConfig {
            grey_cause: Some(GreyCause::Flag),
            ..RenderConfig::default()
        };
        let svg = render(&tree, &packed, &HashMap::new(), &config);
        let data = static_data_of(&svg);

        assert_eq!(
            data["volatility"],
            "Volatility disabled: run without --no-volatility to see commit activity."
        );
    }

    #[test]
    fn the_volatility_note_names_git_as_the_cause_when_unavailable() {
        let tree = single_crate_tree();
        let packed = pack(&tree);
        let config = RenderConfig {
            grey_cause: Some(GreyCause::GitUnavailable),
            ..RenderConfig::default()
        };
        let svg = render(&tree, &packed, &HashMap::new(), &config);
        let data = static_data_of(&svg);

        assert_eq!(
            data["volatility"],
            "Volatility unavailable: this workspace is not a git repository, or git could not be run."
        );
    }

    #[test]
    fn the_volatility_note_names_the_empty_window_as_the_cause() {
        let tree = single_crate_tree();
        let packed = pack(&tree);
        let config = RenderConfig {
            grey_cause: Some(GreyCause::NoCommitsInWindow { months: 6 }),
            ..RenderConfig::default()
        };
        let svg = render(&tree, &packed, &HashMap::new(), &config);
        let data = static_data_of(&svg);

        assert_eq!(
            data["volatility"],
            "Volatility unavailable: no commits touched this workspace in the last 6 months."
        );
    }

    #[test]
    fn rendering_twice_is_deterministic() {
        let tree = single_crate_tree();
        let packed = pack(&tree);
        let config = RenderConfig::default();
        let targets = HashMap::new();
        assert_eq!(
            render(&tree, &packed, &targets, &config),
            render(&tree, &packed, &targets, &config)
        );
    }

    #[test]
    fn the_page_loads_the_hotspot_script_bundle() {
        let (_, _, svg) = rendered_default();
        assert!(svg.contains("// @module HotspotScript"));
        assert!(svg.contains("// @module Theme"));
    }

    /// `hotspot_jump_icon.js` draws its own copy of the `#jump-icon` symbol,
    /// so the emitted page must not ship `JumpIcons`'s popover and chip code
    /// just for that one glyph (the arc page's own bundle still does).
    #[test]
    fn the_page_does_not_ship_the_jump_popover_chip_code() {
        let (_, _, svg) = rendered_default();
        assert!(!svg.contains("// @module JumpIcons"), "{svg}");
        assert!(!svg.contains("createJumpIcons"), "{svg}");
        assert!(!svg.contains("CHIP_LABELS"), "{svg}");
    }

    #[test]
    fn sidebar_lists_ranked_hotspots_with_size_and_commits() {
        let leaf = HotspotNode {
            name: "hot".to_string(),
            kind: HotspotKind::File,
            file: PathBuf::from("app/src/hot.rs"),
            lines: 100,
            commits: 5,
            children: Vec::new(),
        };
        let tree = HotspotTree {
            root: HotspotNode {
                name: "app".to_string(),
                kind: HotspotKind::Crate,
                file: PathBuf::from("app/Cargo.toml"),
                lines: 100,
                commits: 5,
                children: vec![leaf.clone()],
            },
            max_file_commits: 5,
            total_commits: 5,
            hotspots: vec![leaf],
        };
        let packed = pack(&tree);
        let svg = render(&tree, &packed, &HashMap::new(), &RenderConfig::default());

        assert!(svg.contains("id=\"hotspot-list\""), "{svg}");
        assert!(svg.contains("1. hot — 100×5"), "{svg}");
        assert!(svg.contains("id=\"hotspot-details\""), "{svg}");
        assert!(svg.contains("id=\"hotspot-list-toggle\""), "{svg}");
        assert!(!svg.contains("id=\"hotspot-volatility-note\""), "{svg}");
    }

    #[test]
    fn a_leafs_fill_percent_is_against_the_file_reference_a_containers_against_the_workspace() {
        let leaf = HotspotNode {
            name: "hot".to_string(),
            kind: HotspotKind::File,
            file: PathBuf::from("app/src/hot.rs"),
            lines: 100,
            commits: 5,
            children: Vec::new(),
        };
        let tree = HotspotTree {
            root: HotspotNode {
                name: "app".to_string(),
                kind: HotspotKind::Crate,
                file: PathBuf::from("app/Cargo.toml"),
                lines: 100,
                commits: 10,
                children: vec![leaf.clone()],
            },
            max_file_commits: 8,
            total_commits: 10,
            hotspots: vec![leaf],
        };
        let packed = pack(&tree);
        let svg = render(&tree, &packed, &HashMap::new(), &RenderConfig::default());
        let data = static_data_of(&svg);

        // round(100 * sqrt(5) / sqrt(8)) = 79: the leaf against the busiest file.
        assert_eq!(data["nodes"]["app/src/hot.rs"]["fillPercent"], 79);
        // sqrt(10) / sqrt(10) = 1: the crate saturates against the workspace total.
        assert_eq!(data["nodes"]["app/Cargo.toml"]["fillPercent"], 100);
    }

    #[test]
    fn node_keys_disambiguates_three_nodes_sharing_one_file() {
        let make = |name: &str| HotspotNode {
            name: name.to_string(),
            kind: HotspotKind::File,
            file: PathBuf::from("app/src/shared.rs"),
            lines: 1,
            commits: 0,
            children: Vec::new(),
        };
        let nodes = [make("a"), make("b"), make("c")];
        let ordered: Vec<(&HotspotNode, Option<usize>)> =
            nodes.iter().map(|node| (node, None)).collect();

        let keys = node_keys(&ordered);

        assert_eq!(
            keys,
            vec![
                "app/src/shared.rs".to_string(),
                "app/src/shared.rs#leaf".to_string(),
                "app/src/shared.rs#leaf2".to_string(),
            ],
            "every node sharing the file must get its own key, not two nodes colliding on \"#leaf\""
        );
    }

    #[test]
    fn sidebar_shows_the_no_volatility_line_when_grey() {
        let svg = rendered_grey();

        assert!(svg.contains("id=\"hotspot-volatility-note\""), "{svg}");
    }

    /// Only a cause makes the map grey: a tree whose files have no commits
    /// in the window is still drawn in colour when no cause is given.
    #[test]
    fn without_a_grey_cause_the_map_is_not_grey() {
        let (_, _, svg) = rendered_default();
        let data = static_data_of(&svg);

        assert!(data["volatility"].is_null(), "{data}");
        let circle = circle_tag_for(&svg, "app/src/hot.rs");
        assert!(!circle.contains(CSS.hotspots.grey), "{circle}");
    }

    /// The list/bars toggle switches between two views of the ranking; with
    /// no churn data that ranking is empty, so the toggle would sit over
    /// nothing to switch.
    #[test]
    fn sidebar_hides_the_list_toggle_when_grey() {
        let svg = rendered_grey();

        assert!(!svg.contains("id=\"hotspot-list-toggle\""), "{svg}");
    }

    /// `targets` is `cli::register_hotspot_targets`'s own return value: a
    /// leaf with an id in it gets one jump target under `with_jump_ids`,
    /// the same idiom the arc page's own `NodeData.targets` uses.
    #[test]
    fn a_leaf_with_an_assigned_id_carries_one_jump_target_under_with_jump_ids() {
        let tree = single_crate_tree();
        let packed = pack(&tree);
        let mut targets = HashMap::new();
        targets.insert(PathBuf::from("app/src/hot.rs"), LocationId::from(7));

        let config = RenderConfig {
            with_jump_ids: true,
            ..RenderConfig::default()
        };
        let svg = render(&tree, &packed, &targets, &config);
        let data = static_data_of(&svg);
        let leaf_targets = data["nodes"]["app/src/hot.rs"]["targets"]
            .as_array()
            .expect("a leaf with an id has one target");
        assert_eq!(leaf_targets.len(), 1);
        assert_eq!(leaf_targets[0]["kind"], "module");
        assert_eq!(leaf_targets[0]["name"], "hot.rs");
        assert_eq!(leaf_targets[0]["jump"], 7);

        // The crate container has no id of its own in `targets` (only files
        // get one) and carries no target field at all.
        assert!(data["nodes"]["app/Cargo.toml"].get("targets").is_none());
    }

    /// The ids exist on `targets` either way; `with_jump_ids` only gates
    /// whether `render` serializes them, the same contract `RenderConfig`
    /// documents for the arc page.
    #[test]
    fn targets_are_not_serialized_without_with_jump_ids() {
        let tree = single_crate_tree();
        let packed = pack(&tree);
        let mut targets = HashMap::new();
        targets.insert(PathBuf::from("app/src/hot.rs"), LocationId::from(7));

        let svg = render(&tree, &packed, &targets, &RenderConfig::default());
        let data = static_data_of(&svg);
        assert!(data["nodes"]["app/src/hot.rs"].get("targets").is_none());
    }

    /// The stylesheet styles the hover tooltip and the sidebar's own
    /// classes, not just the circles: a real background, border and text
    /// colour for each, not the SVG's black-on-transparent default.
    #[test]
    fn the_stylesheet_styles_the_tooltip_and_the_sidebar() {
        let (_, _, svg) = rendered_default();

        for selector in [
            format!(".{} rect", CSS.hotspots.tooltip),
            format!(".{} text", CSS.hotspots.tooltip),
            format!(".{}", CSS.hotspots.sidebar),
            format!(".{}", CSS.hotspots.list_item),
            format!(".{}", CSS.hotspots.list_toggle),
            format!(".{}", CSS.hotspots.bar_track),
        ] {
            assert!(
                svg.contains(&selector),
                "missing rule for {selector}: {svg}"
            );
        }
    }

    /// The `toolbar-fo` foreignObject's own markup, for asserting on the
    /// bar's structure independent of the circles and sidebar around it.
    fn toolbar_markup(svg: &str) -> &str {
        let start = svg.find("id=\"toolbar-fo\"").expect("toolbar present");
        let end = start + svg[start..].find("</foreignObject>").unwrap();
        &svg[start..end]
    }

    /// The map's appearance, light-theme and dark-theme controls sit inside
    /// the same View dropdown the arc page uses, not as bare selects on the
    /// bar (`super::toolbar::render` is the shared renderer for both).
    #[test]
    fn hotspot_toolbar_puts_appearance_controls_inside_the_view_dropdown() {
        let (_, _, svg) = rendered_default();
        let toolbar = toolbar_markup(&svg);
        let panel_start = toolbar
            .find("toolbar-dropdown-panel")
            .expect("View dropdown panel present");

        for id in ["theme-mode", "theme-light", "theme-dark"] {
            let pos = toolbar
                .find(&format!("id=\"{id}\""))
                .unwrap_or_else(|| panic!("{id} missing, got: {toolbar}"));
            assert!(
                pos > panel_start,
                "{id} must sit inside the View dropdown panel, got: {toolbar}"
            );
        }
    }

    /// Outside the (collapsed) View dropdown, the map's bar shows buttons
    /// only - no raw `<select>` eating the bar's width.
    #[test]
    fn hotspot_toolbar_shows_no_bare_selects_outside_the_view_dropdown() {
        let (_, _, svg) = rendered_default();
        let toolbar = toolbar_markup(&svg);
        let panel_start = toolbar
            .find("toolbar-dropdown-panel")
            .expect("View dropdown panel present");

        let selects: Vec<usize> = toolbar.match_indices("<select").map(|(i, _)| i).collect();
        assert!(!selects.is_empty(), "expected the three theme selects");
        assert!(
            selects.iter().all(|&pos| pos > panel_start),
            "a <select> sits outside the View dropdown, got: {toolbar}"
        );
    }

    /// `STATIC_DATA`'s own `classes` map names every hotspot class JS builds
    /// markup with, sourced from `CSS.hotspots` instead of a literal string
    /// repeated in Rust and JS.
    #[test]
    fn static_data_carries_the_hotspot_class_names() {
        let (_, _, svg) = rendered_default();
        let data = static_data_of(&svg);

        assert_eq!(data["classes"]["circle"], CSS.hotspots.circle);
        assert_eq!(data["classes"]["listItem"], CSS.hotspots.list_item);
        assert_eq!(data["classes"]["hover"], CSS.hotspots.hover);
        assert_eq!(data["classes"]["selected"], CSS.hotspots.selected);
        assert_eq!(data["classes"]["barLabel"], CSS.hotspots.bar_label);
        assert_eq!(data["classes"]["barTrack"], CSS.hotspots.bar_track);
        assert_eq!(data["classes"]["barFill"], CSS.hotspots.bar_fill);
        assert_eq!(data["classes"]["detailsTitle"], CSS.hotspots.details_title);
        assert_eq!(data["classes"]["tooltip"], CSS.hotspots.tooltip);
        assert_eq!(data["classes"]["jumpIcon"], CSS.hotspots.jump_icon);
    }
}
