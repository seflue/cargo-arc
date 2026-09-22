//! Workspace/crate/module/file hierarchy with sizes, commits and ranks.

use crate::graph::{ArcGraph, EdgeWeight, Node};
use crate::volatility::Commit;
use petgraph::graph::NodeIndex;
use petgraph::visit::EdgeRef;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// A workspace, crate, container module or file in the hotspot tree,
/// identified by its workspace-relative file (a crate by its `Cargo.toml`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HotspotNode {
    pub name: String,
    pub kind: HotspotKind,
    /// Workspace-relative.
    pub file: PathBuf,
    pub lines: usize,
    /// Distinct commits: the file's own for a leaf, the subtree's for a
    /// container.
    pub commits: usize,
    pub children: Vec<HotspotNode>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HotspotKind {
    Workspace,
    Crate,
    Module,
    File,
}

/// The built hierarchy plus the workspace-wide figures the map's colour scale
/// and hotspot list need.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HotspotTree {
    pub root: HotspotNode,
    /// The highest single-file commit count in the workspace.
    pub max_file_commits: usize,
    /// Distinct commits across the whole workspace (the root's own).
    pub total_commits: usize,
    /// The top-N leaves by `lines * commits`, rank order, score above zero.
    pub hotspots: Vec<HotspotNode>,
}

impl HotspotTree {
    /// Return `node`'s 1-based position in `hotspots`; `None` for a
    /// container, even one sharing its file with a ranked leaf.
    #[must_use]
    pub fn rank_of(&self, node: &HotspotNode) -> Option<usize> {
        if node.kind != HotspotKind::File {
            return None;
        }
        self.hotspots
            .iter()
            .position(|hotspot| hotspot.file == node.file)
            .map(|index| index + 1)
    }
}

/// Build the hotspot tree from `graph`'s crates, modules and `Contains` edges,
/// asking `lines` and `commits` per absolute file path; `n` caps `hotspots`.
pub fn build<L, C>(
    graph: &ArcGraph,
    workspace_root: &Path,
    lines: L,
    commits: C,
    n: usize,
) -> HotspotTree
where
    L: Fn(&Path) -> usize,
    C: Fn(&Path) -> BTreeSet<Commit>,
{
    let crate_indices: Vec<_> = graph
        .node_indices()
        .filter(|&idx| graph[idx].is_crate())
        .collect();

    let root = if let [only] = crate_indices[..] {
        build_crate(graph, only, workspace_root, &lines, &commits).0
    } else {
        let crates: Vec<(HotspotNode, BTreeSet<Commit>)> = crate_indices
            .iter()
            .map(|&idx| build_crate(graph, idx, workspace_root, &lines, &commits))
            .collect();
        build_workspace_root(crates, workspace_root)
    };

    let max_file_commits = max_file_commits(&root);
    let hotspots = rank_leaves(&root, n);
    let total_commits = root.commits;

    HotspotTree {
        root,
        max_file_commits,
        total_commits,
        hotspots,
    }
}

fn hotspot_score(node: &HotspotNode) -> usize {
    node.lines.saturating_mul(node.commits)
}

fn max_file_commits(node: &HotspotNode) -> usize {
    if node.kind == HotspotKind::File {
        return node.commits;
    }
    node.children
        .iter()
        .map(max_file_commits)
        .max()
        .unwrap_or(0)
}

/// Return the first `n` file leaves under `root` with positive score, by
/// descending score, ties by name.
fn rank_leaves(root: &HotspotNode, n: usize) -> Vec<HotspotNode> {
    let mut leaves: Vec<&HotspotNode> = Vec::new();
    collect_leaves(root, &mut leaves);

    leaves.retain(|leaf| hotspot_score(leaf) > 0);
    leaves.sort_by(|a, b| {
        hotspot_score(b)
            .cmp(&hotspot_score(a))
            .then_with(|| a.name.cmp(&b.name))
    });
    leaves.truncate(n);
    leaves.into_iter().cloned().collect()
}

/// Wrap `crates` in a workspace root, sorted by name, whose commits are the
/// union of theirs.
fn build_workspace_root(
    mut crates: Vec<(HotspotNode, BTreeSet<Commit>)>,
    workspace_root: &Path,
) -> HotspotNode {
    crates.sort_by(|(a, _), (b, _)| a.name.cmp(&b.name));

    let lines_sum: usize = crates.iter().map(|(c, _)| c.lines).sum();
    let mut all_commits = BTreeSet::new();
    let children: Vec<HotspotNode> = crates
        .into_iter()
        .map(|(node, commits)| {
            all_commits.extend(commits);
            node
        })
        .collect();
    HotspotNode {
        name: workspace_root
            .file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().into_owned()),
        kind: HotspotKind::Workspace,
        file: PathBuf::new(),
        lines: lines_sum,
        commits: all_commits.len(),
        children,
    }
}

/// Collect every `File`-kind leaf reachable from `node`, in tree order.
fn collect_leaves<'a>(node: &'a HotspotNode, out: &mut Vec<&'a HotspotNode>) {
    if node.kind == HotspotKind::File {
        out.push(node);
        return;
    }
    for child in &node.children {
        collect_leaves(child, out);
    }
}

fn relative(workspace_root: &Path, absolute: &Path) -> PathBuf {
    absolute
        .strip_prefix(workspace_root)
        .unwrap_or(absolute)
        .to_path_buf()
}

fn file_name(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.to_string_lossy().into_owned(),
        |n| n.to_string_lossy().into_owned(),
    )
}

/// Return `idx`'s direct `Contains` children in edge insertion order.
fn contains_children(graph: &ArcGraph, idx: NodeIndex) -> Vec<NodeIndex> {
    graph
        .edges(idx)
        .filter(|e| matches!(e.weight(), EdgeWeight::Contains))
        .map(|e| e.target())
        .collect()
}

fn leaf(
    name: String,
    file: &Path,
    workspace_root: &Path,
    lines: usize,
    commits: usize,
) -> HotspotNode {
    HotspotNode {
        name,
        kind: HotspotKind::File,
        file: relative(workspace_root, file),
        lines,
        commits,
        children: Vec::new(),
    }
}

fn build_crate<L, C>(
    graph: &ArcGraph,
    idx: NodeIndex,
    workspace_root: &Path,
    lines: &L,
    commits_of: &C,
) -> (HotspotNode, BTreeSet<Commit>)
where
    L: Fn(&Path) -> usize,
    C: Fn(&Path) -> BTreeSet<Commit>,
{
    let Node::Crate {
        name,
        target_roots,
        manifest,
        ..
    } = &graph[idx]
    else {
        unreachable!("filtered to crate nodes")
    };

    let mut children = Vec::new();
    let mut subtree_commits = BTreeSet::new();

    for root_file in target_roots.files() {
        let commits = commits_of(root_file);
        subtree_commits.extend(commits.iter().cloned());
        children.push(leaf(
            file_name(root_file),
            root_file,
            workspace_root,
            lines(root_file),
            commits.len(),
        ));
    }

    for module_idx in contains_children(graph, idx) {
        if let Some((node, commits)) =
            build_module(graph, module_idx, workspace_root, lines, commits_of)
        {
            subtree_commits.extend(commits);
            children.push(node);
        }
    }

    let lines_sum: usize = children.iter().map(|c| c.lines).sum();

    let node = HotspotNode {
        name: name.clone(),
        kind: HotspotKind::Crate,
        file: relative(workspace_root, manifest),
        lines: lines_sum,
        commits: subtree_commits.len(),
        children,
    };
    (node, subtree_commits)
}

/// Build a module as a leaf, or with children as a container holding its own
/// file as a leaf; `None` for a module without a declaring file.
fn build_module<L, C>(
    graph: &ArcGraph,
    idx: NodeIndex,
    workspace_root: &Path,
    lines: &L,
    commits_of: &C,
) -> Option<(HotspotNode, BTreeSet<Commit>)>
where
    L: Fn(&Path) -> usize,
    C: Fn(&Path) -> BTreeSet<Commit>,
{
    let Node::Module { name, file, .. } = &graph[idx] else {
        unreachable!("contains_children of a crate/module only yields modules")
    };
    let file = file.as_ref()?;

    let child_indices = contains_children(graph, idx);
    let own_commits = commits_of(file);

    if child_indices.is_empty() {
        let node = leaf(
            file_name(file),
            file,
            workspace_root,
            lines(file),
            own_commits.len(),
        );
        return Some((node, own_commits));
    }

    let self_leaf = leaf(
        file_name(file),
        file,
        workspace_root,
        lines(file),
        own_commits.len(),
    );
    let mut subtree_commits = own_commits;
    let mut children = vec![self_leaf];

    for child_idx in child_indices {
        if let Some((child_node, child_commits)) =
            build_module(graph, child_idx, workspace_root, lines, commits_of)
        {
            subtree_commits.extend(child_commits);
            children.push(child_node);
        }
    }

    let lines_sum: usize = children.iter().map(|c| c.lines).sum();
    let node = HotspotNode {
        name: name.clone(),
        kind: HotspotKind::Module,
        file: relative(workspace_root, file),
        lines: lines_sum,
        commits: subtree_commits.len(),
        children,
    };
    Some((node, subtree_commits))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::ArcGraph;
    use crate::model::TargetRoots;
    use crate::test_support::{crate_node_with_targets, module_node, module_node_with_file};
    use std::path::{Path, PathBuf};

    #[test]
    fn a_crate_root_target_is_a_leaf_in_the_crate() {
        let mut graph = ArcGraph::new();
        graph.add_node(crate_node_with_targets(
            "app",
            TargetRoots {
                lib_root: Some(PathBuf::from("/ws/app/src/lib.rs")),
                bin_roots: vec![],
            },
            "/ws/app/Cargo.toml",
        ));

        let tree = build(&graph, Path::new("/ws"), |_| 10, no_commits, 10);

        assert_eq!(tree.root.kind, HotspotKind::Crate);
        assert_eq!(tree.root.name, "app");
        assert_eq!(tree.root.file, PathBuf::from("app/Cargo.toml"));
        assert_eq!(tree.root.children.len(), 1);

        let leaf = &tree.root.children[0];
        assert_eq!(leaf.kind, HotspotKind::File);
        assert_eq!(leaf.name, "lib.rs");
        assert_eq!(leaf.file, PathBuf::from("app/src/lib.rs"));
        assert_eq!(leaf.lines, 10);
        assert_eq!(leaf.commits, 0);
        assert_eq!(tree.rank_of(leaf), None);

        assert_eq!(tree.root.lines, 10);
        assert_eq!(tree.root.commits, 0);
        assert_eq!(tree.total_commits, 0);
        assert_eq!(tree.max_file_commits, 0);
        assert!(tree.hotspots.is_empty());
    }

    #[test]
    fn a_module_without_children_is_a_leaf() {
        let mut graph = ArcGraph::new();
        let crate_idx = graph.add_node(crate_node_with_targets(
            "app",
            TargetRoots::default(),
            "/ws/app/Cargo.toml",
        ));
        let cli_idx = graph.add_node(module_node_with_file(
            "cli",
            crate_idx,
            "/ws/app/src/cli.rs",
        ));
        graph.add_edge(crate_idx, cli_idx, crate::graph::EdgeWeight::Contains);

        let tree = build(&graph, Path::new("/ws"), |_| 4, no_commits, 10);

        assert_eq!(tree.root.children.len(), 1);
        let leaf = &tree.root.children[0];
        assert_eq!(leaf.kind, HotspotKind::File);
        assert_eq!(leaf.name, "cli.rs");
        assert_eq!(leaf.file, PathBuf::from("app/src/cli.rs"));
        assert_eq!(leaf.lines, 4);
        assert!(leaf.children.is_empty());
        assert_eq!(tree.root.lines, 4);
    }

    #[test]
    fn a_module_with_children_is_a_container_holding_its_own_file_as_a_leaf() {
        let mut graph = ArcGraph::new();
        let crate_idx = graph.add_node(crate_node_with_targets(
            "app",
            TargetRoots::default(),
            "/ws/app/Cargo.toml",
        ));
        let analyze_idx = graph.add_node(module_node_with_file(
            "analyze",
            crate_idx,
            "/ws/app/src/analyze/mod.rs",
        ));
        graph.add_edge(crate_idx, analyze_idx, crate::graph::EdgeWeight::Contains);
        let hir_idx = graph.add_node(module_node_with_file(
            "hir",
            crate_idx,
            "/ws/app/src/analyze/hir.rs",
        ));
        graph.add_edge(analyze_idx, hir_idx, crate::graph::EdgeWeight::Contains);

        let tree = build(
            &graph,
            Path::new("/ws"),
            |f| if f.ends_with("mod.rs") { 3 } else { 7 },
            no_commits,
            10,
        );

        assert_eq!(tree.root.children.len(), 1);
        let container = &tree.root.children[0];
        assert_eq!(container.kind, HotspotKind::Module);
        assert_eq!(container.name, "analyze");
        assert_eq!(container.file, PathBuf::from("app/src/analyze/mod.rs"));
        assert_eq!(container.lines, 10);
        assert_eq!(container.children.len(), 2);

        let self_leaf = &container.children[0];
        assert_eq!(self_leaf.kind, HotspotKind::File);
        assert_eq!(self_leaf.name, "mod.rs");
        assert_eq!(self_leaf.file, PathBuf::from("app/src/analyze/mod.rs"));
        assert_eq!(self_leaf.lines, 3);

        let hir_leaf = &container.children[1];
        assert_eq!(hir_leaf.kind, HotspotKind::File);
        assert_eq!(hir_leaf.name, "hir.rs");
        assert_eq!(hir_leaf.file, PathBuf::from("app/src/analyze/hir.rs"));
        assert_eq!(hir_leaf.lines, 7);
    }

    #[test]
    fn several_crates_get_a_workspace_root_a_single_crate_does_not() {
        let mut graph = ArcGraph::new();
        graph.add_node(crate_node_with_targets(
            "app",
            TargetRoots {
                lib_root: Some(PathBuf::from("/ws/app/src/lib.rs")),
                bin_roots: vec![],
            },
            "/ws/app/Cargo.toml",
        ));
        graph.add_node(crate_node_with_targets(
            "core",
            TargetRoots {
                lib_root: Some(PathBuf::from("/ws/core/src/lib.rs")),
                bin_roots: vec![],
            },
            "/ws/core/Cargo.toml",
        ));

        let tree = build(&graph, Path::new("/ws"), |_| 5, no_commits, 10);

        assert_eq!(tree.root.kind, HotspotKind::Workspace);
        assert_eq!(tree.root.name, "ws");
        assert_eq!(tree.root.file, PathBuf::new());
        assert_eq!(tree.root.lines, 10);
        let names: Vec<&str> = tree.root.children.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, vec!["app", "core"]);
    }

    fn no_commits(_: &Path) -> BTreeSet<Commit> {
        BTreeSet::new()
    }

    fn commits_from(
        map: std::collections::HashMap<PathBuf, BTreeSet<Commit>>,
    ) -> impl Fn(&Path) -> BTreeSet<Commit> {
        move |file| map.get(file).cloned().unwrap_or_default()
    }

    fn commit(hash: &str) -> Commit {
        Commit {
            hash: hash.to_string(),
            timestamp: 0,
        }
    }

    #[test]
    fn leaves_rank_by_lines_times_commits_ties_by_name() {
        let mut graph = ArcGraph::new();
        let crate_idx = graph.add_node(crate_node_with_targets(
            "app",
            TargetRoots::default(),
            "/ws/app/Cargo.toml",
        ));
        let hot_idx = graph.add_node(module_node_with_file(
            "hot",
            crate_idx,
            "/ws/app/src/hot.rs",
        ));
        graph.add_edge(crate_idx, hot_idx, crate::graph::EdgeWeight::Contains);
        let cold_idx = graph.add_node(module_node_with_file(
            "cold",
            crate_idx,
            "/ws/app/src/cold.rs",
        ));
        graph.add_edge(crate_idx, cold_idx, crate::graph::EdgeWeight::Contains);
        let quiet_idx = graph.add_node(module_node_with_file(
            "quiet",
            crate_idx,
            "/ws/app/src/quiet.rs",
        ));
        graph.add_edge(crate_idx, quiet_idx, crate::graph::EdgeWeight::Contains);

        let mut commits = std::collections::HashMap::new();
        commits.insert(
            PathBuf::from("/ws/app/src/hot.rs"),
            BTreeSet::from([commit("a"), commit("b")]),
        );
        commits.insert(
            PathBuf::from("/ws/app/src/cold.rs"),
            BTreeSet::from([commit("c")]),
        );
        let tree = build(
            &graph,
            Path::new("/ws"),
            |f| if f.ends_with("hot.rs") { 100 } else { 10 },
            commits_from(commits),
            1,
        );

        assert_eq!(tree.max_file_commits, 2);
        assert_eq!(tree.total_commits, 3);
        assert_eq!(tree.hotspots.len(), 1);
        assert_eq!(tree.hotspots[0].name, "hot.rs");
        assert_eq!(tree.rank_of(&tree.hotspots[0]), Some(1));

        let quiet = tree
            .root
            .children
            .iter()
            .find(|c| c.name == "quiet.rs")
            .unwrap();
        assert_eq!(quiet.commits, 0);
        assert_eq!(tree.rank_of(quiet), None);
    }

    #[test]
    fn a_container_has_no_rank_even_when_its_own_file_is_ranked() {
        let mut graph = ArcGraph::new();
        let crate_idx = graph.add_node(crate_node_with_targets(
            "app",
            TargetRoots::default(),
            "/ws/app/Cargo.toml",
        ));
        let outer_idx = graph.add_node(module_node_with_file(
            "outer",
            crate_idx,
            "/ws/app/src/outer.rs",
        ));
        graph.add_edge(crate_idx, outer_idx, crate::graph::EdgeWeight::Contains);
        let inner_idx = graph.add_node(module_node_with_file(
            "inner",
            crate_idx,
            "/ws/app/src/outer/inner.rs",
        ));
        graph.add_edge(outer_idx, inner_idx, crate::graph::EdgeWeight::Contains);

        let mut commits = std::collections::HashMap::new();
        commits.insert(
            PathBuf::from("/ws/app/src/outer.rs"),
            BTreeSet::from([commit("a")]),
        );

        let tree = build(&graph, Path::new("/ws"), |_| 1, commits_from(commits), 10);

        let container = &tree.root.children[0];
        assert_eq!(container.kind, HotspotKind::Module);
        let own_leaf = &container.children[0];
        assert_eq!(own_leaf.file, container.file);
        assert_eq!(tree.rank_of(own_leaf), Some(1));
        assert_eq!(tree.rank_of(container), None);
    }

    #[test]
    fn container_commits_are_the_union_not_the_sum_of_shared_commits() {
        let mut graph = ArcGraph::new();
        let crate_idx = graph.add_node(crate_node_with_targets(
            "app",
            TargetRoots::default(),
            "/ws/app/Cargo.toml",
        ));
        let outer_idx = graph.add_node(module_node_with_file(
            "outer",
            crate_idx,
            "/ws/app/src/outer.rs",
        ));
        graph.add_edge(crate_idx, outer_idx, crate::graph::EdgeWeight::Contains);
        let hot_idx = graph.add_node(module_node_with_file(
            "hot",
            crate_idx,
            "/ws/app/src/hot.rs",
        ));
        graph.add_edge(outer_idx, hot_idx, crate::graph::EdgeWeight::Contains);
        let cold_idx = graph.add_node(module_node_with_file(
            "cold",
            crate_idx,
            "/ws/app/src/cold.rs",
        ));
        graph.add_edge(outer_idx, cold_idx, crate::graph::EdgeWeight::Contains);

        let mut commits = std::collections::HashMap::new();
        commits.insert(
            PathBuf::from("/ws/app/src/hot.rs"),
            BTreeSet::from([commit("a"), commit("b")]),
        );
        commits.insert(
            PathBuf::from("/ws/app/src/cold.rs"),
            BTreeSet::from([commit("b"), commit("c")]),
        );
        let tree = build(&graph, Path::new("/ws"), |_| 1, commits_from(commits), 10);

        // hot.rs and cold.rs share commit "b"; a summing implementation would
        // count it twice at each container level.
        let outer = &tree.root.children[0];
        assert_eq!(outer.name, "outer");
        assert_eq!(outer.commits, 3);
        assert_eq!(tree.root.commits, 3);
        assert_eq!(tree.total_commits, 3);
    }

    #[test]
    fn a_workspace_nested_in_the_repository_still_emits_workspace_relative_paths() {
        let mut graph = ArcGraph::new();
        graph.add_node(crate_node_with_targets(
            "app",
            TargetRoots {
                lib_root: Some(PathBuf::from("/repo/sub/workspace/app/src/lib.rs")),
                bin_roots: vec![],
            },
            "/repo/sub/workspace/app/Cargo.toml",
        ));

        let tree = build(
            &graph,
            Path::new("/repo/sub/workspace"),
            |_| 1,
            no_commits,
            10,
        );

        assert_eq!(tree.root.file, PathBuf::from("app/Cargo.toml"));
        assert_eq!(tree.root.children[0].file, PathBuf::from("app/src/lib.rs"));
    }

    #[test]
    fn a_module_with_no_declaring_file_has_no_place_in_the_tree() {
        let mut graph = ArcGraph::new();
        let crate_idx = graph.add_node(crate_node_with_targets(
            "app",
            TargetRoots::default(),
            "/ws/app/Cargo.toml",
        ));
        let fileless_idx = graph.add_node(module_node("ghost", crate_idx));
        graph.add_edge(crate_idx, fileless_idx, crate::graph::EdgeWeight::Contains);

        let tree = build(&graph, Path::new("/ws"), |_| 5, no_commits, 10);

        assert!(tree.root.children.is_empty());
    }
}
