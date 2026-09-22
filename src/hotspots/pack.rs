//! Circle packing for the hotspot map: a container's children packed inside
//! it, with a label band reserved at the top.
//!
//! `pack_siblings` and `enclose` are a Rust port of d3-hierarchy's
//! `src/pack/siblings.js` and `src/pack/enclose.js`
//! (<https://github.com/d3/d3-hierarchy>, ISC licence): the front-chain
//! packer (Wang et al., "Visualization of large hierarchical data by circle
//! packing", 2006) and Welzl's smallest-enclosing-circle construction. Their
//! vocabulary (front chain, basis, enclose) is kept.

use super::tree::{HotspotNode, HotspotTree};
use std::path::PathBuf;

/// Gap between a container's children, and from its own boundary to its
/// enclosing children.
const PAD: f64 = 2.0;
/// Radius of a file with no lines, so it stays visible.
const MIN_R: f64 = 2.0;
/// A container's label band, sized for the zoom where its own parent fills
/// the canvas: `LABEL_BAND_OF_PARENT * sqrt(parent.lines)`.
const LABEL_BAND_OF_PARENT: f64 = 0.035;
/// The band capped at this share of the container's own packed radius, so a
/// small container is not mostly band.
const LABEL_BAND_RATIO: f64 = 0.08;

/// One packed circle, in layout units (a leaf's radius is `sqrt(lines)`).
#[derive(Debug, Clone, PartialEq)]
pub struct PackedCircle {
    /// Workspace-relative; identifies the [`HotspotNode`] this circle is for.
    pub file: PathBuf,
    pub cx: f64,
    pub cy: f64,
    pub r: f64,
    pub depth: usize,
}

/// Pack `tree` into one circle per node, in preorder with children by lines
/// descending, ties by name. A container shares its `file` with its own leaf.
#[must_use]
pub fn pack(tree: &HotspotTree) -> Vec<PackedCircle> {
    let root = layout_node(&tree.root, None);
    let mut out = Vec::new();
    flatten(&root, 0.0, 0.0, 0, &mut out);
    out
}

/// A node mid-layout: its packed radius, and its position relative to its
/// parent's centre (the tree root's own is the origin).
struct Layout<'a> {
    node: &'a HotspotNode,
    radius: f64,
    x: f64,
    y: f64,
    children: Vec<Layout<'a>>,
}

#[allow(clippy::cast_precision_loss)] // code line counts stay well below 2^52
fn layout_node(node: &HotspotNode, parent_lines: Option<usize>) -> Layout<'_> {
    if node.children.is_empty() {
        let radius = (node.lines as f64).sqrt().max(MIN_R);
        return Layout {
            node,
            radius,
            x: 0.0,
            y: 0.0,
            children: Vec::new(),
        };
    }

    let mut order: Vec<&HotspotNode> = node.children.iter().collect();
    order.sort_by(|a, b| b.lines.cmp(&a.lines).then_with(|| a.name.cmp(&b.name)));
    let mut children: Vec<Layout> = order
        .into_iter()
        .map(|child| layout_node(child, Some(node.lines)))
        .collect();

    // packSiblings widens each child by PAD so the packed circles keep a gap
    // from one another; the child keeps its true radius once placed.
    let mut circles: Vec<Circle> = children
        .iter()
        .map(|child| Circle {
            r: child.radius + PAD,
            x: 0.0,
            y: 0.0,
        })
        .collect();
    let enclosing_radius = pack_siblings(&mut circles);
    for (child, circle) in children.iter_mut().zip(&circles) {
        child.x = circle.x;
        child.y = circle.y;
    }

    let parent_size = (parent_lines.unwrap_or(node.lines) as f64).sqrt();
    let band = (LABEL_BAND_OF_PARENT * parent_size).min(LABEL_BAND_RATIO * enclosing_radius);
    for child in &mut children {
        child.y += band;
    }

    Layout {
        node,
        radius: enclosing_radius + PAD + band,
        x: 0.0,
        y: 0.0,
        children,
    }
}

#[allow(clippy::similar_names)] // parent_cx/parent_cy are one coordinate pair
fn flatten(
    layout: &Layout,
    parent_cx: f64,
    parent_cy: f64,
    depth: usize,
    out: &mut Vec<PackedCircle>,
) {
    let cx = parent_cx + layout.x;
    let cy = parent_cy + layout.y;
    out.push(PackedCircle {
        file: layout.node.file.clone(),
        cx,
        cy,
        r: layout.radius,
        depth,
    });
    for child in &layout.children {
        flatten(child, cx, cy, depth + 1, out);
    }
}

// ===== front-chain packing (d3-hierarchy siblings.js) =====

#[derive(Debug, Clone, Copy)]
struct Circle {
    r: f64,
    x: f64,
    y: f64,
}

/// One entry of the front chain, indexed the same as its circle.
#[derive(Debug, Clone, Copy)]
struct Link {
    next: usize,
    prev: usize,
}

/// Set each circle's `x`, `y` around a common centre without overlap, and
/// return the radius of the smallest circle enclosing them all.
#[allow(clippy::many_single_char_names)] // mirrors d3-hierarchy's own names
#[allow(clippy::too_many_lines)] // single cohesive front-chain port
fn pack_siblings(circles: &mut [Circle]) -> f64 {
    let n = circles.len();
    if n == 0 {
        return 0.0;
    }
    circles[0].x = 0.0;
    circles[0].y = 0.0;
    if n == 1 {
        return circles[0].r;
    }

    let r0 = circles[0].r;
    circles[0].x = -circles[1].r;
    circles[1].x = r0;
    circles[1].y = 0.0;
    if n == 2 {
        return circles[0].r + circles[1].r;
    }

    let (c0, c1) = (circles[0], circles[1]);
    let mut c2 = circles[2];
    place(c1, c0, &mut c2);
    circles[2] = c2;

    let mut links = vec![Link { next: 0, prev: 0 }; n];
    links[0] = Link { next: 1, prev: 2 };
    links[1] = Link { next: 2, prev: 0 };
    links[2] = Link { next: 0, prev: 1 };
    let mut a = 0_usize;
    let mut b = 1_usize;

    let mut i = 3;
    while i < n {
        let (ca, cb) = (circles[a], circles[b]);
        let mut ci = circles[i];
        place(ca, cb, &mut ci);
        circles[i] = ci;

        let mut j = links[b].next;
        let mut k = links[a].prev;
        let mut sj = circles[b].r;
        let mut sk = circles[a].r;
        let mut retry = false;
        loop {
            if sj <= sk {
                if intersects(circles[j], circles[i]) {
                    b = j;
                    links[a].next = b;
                    links[b].prev = a;
                    retry = true;
                    break;
                }
                sj += circles[j].r;
                j = links[j].next;
            } else {
                if intersects(circles[k], circles[i]) {
                    a = k;
                    links[a].next = b;
                    links[b].prev = a;
                    retry = true;
                    break;
                }
                sk += circles[k].r;
                k = links[k].prev;
            }
            if j == links[k].next {
                break;
            }
        }
        if retry {
            continue;
        }

        let old_b = b;
        links[i] = Link {
            next: old_b,
            prev: a,
        };
        links[a].next = i;
        links[old_b].prev = i;

        let mut best = a;
        let mut best_score = score(circles, &links, a);
        let mut c = i;
        loop {
            c = links[c].next;
            if c == i {
                break;
            }
            let s = score(circles, &links, c);
            if s < best_score {
                best = c;
                best_score = s;
            }
        }
        a = best;
        b = links[a].next;
        i += 1;
    }

    let mut chain = vec![circles[b]];
    let mut c = links[b].next;
    while c != b {
        chain.push(circles[c]);
        c = links[c].next;
    }
    let enclosing = enclose(&chain);
    for circle in circles.iter_mut() {
        circle.x -= enclosing.x;
        circle.y -= enclosing.y;
    }
    enclosing.r
}

/// Move `target` tangent to the anchor circles `first` and `second`; d3
/// calls this `place(b, a, c)` with the same argument order.
fn place(first: Circle, second: Circle, target: &mut Circle) {
    let dx = first.x - second.x;
    let dy = first.y - second.y;
    let d2 = dx * dx + dy * dy;
    if d2 == 0.0 {
        target.x = second.x + target.r;
        target.y = second.y;
        return;
    }
    let mut a2 = second.r + target.r;
    let mut b2 = first.r + target.r;
    a2 *= a2;
    b2 *= b2;
    if a2 > b2 {
        let x = (d2 + b2 - a2) / (2.0 * d2);
        let y = (b2 / d2 - x * x).max(0.0).sqrt();
        target.x = first.x - x * dx - y * dy;
        target.y = first.y - x * dy + y * dx;
    } else {
        let x = (d2 + a2 - b2) / (2.0 * d2);
        let y = (a2 / d2 - x * x).max(0.0).sqrt();
        target.x = second.x + x * dx - y * dy;
        target.y = second.y + x * dy + y * dx;
    }
}

fn intersects(a: Circle, b: Circle) -> bool {
    let dr = a.r + b.r - 1e-6;
    let dx = b.x - a.x;
    let dy = b.y - a.y;
    dr > 0.0 && dr * dr > dx * dx + dy * dy
}

fn score(circles: &[Circle], links: &[Link], idx: usize) -> f64 {
    let a = circles[idx];
    let b = circles[links[idx].next];
    let ab = a.r + b.r;
    let dx = (a.x * b.r + b.x * a.r) / ab;
    let dy = (a.y * b.r + b.y * a.r) / ab;
    dx * dx + dy * dy
}

// ===== smallest enclosing circle (d3-hierarchy enclose.js, Welzl) =====

/// Return the smallest circle enclosing every circle in `circles`. Unlike
/// d3's `packEnclose`, it visits them in slice order without shuffling.
#[allow(clippy::many_single_char_names)] // mirrors d3-hierarchy's own names
fn enclose(circles: &[Circle]) -> Circle {
    let mut basis: Vec<Circle> = Vec::new();
    let mut e: Option<Circle> = None;
    let mut i = 0;
    while i < circles.len() {
        let p = circles[i];
        if e.is_some_and(|e| encloses_weak(e, p)) {
            i += 1;
        } else {
            basis = extend_basis(&basis, p);
            e = Some(enclose_basis(&basis));
            i = 0;
        }
    }
    e.unwrap_or(Circle {
        x: 0.0,
        y: 0.0,
        r: 0.0,
    })
}

fn extend_basis(basis: &[Circle], p: Circle) -> Vec<Circle> {
    if encloses_weak_all(p, basis) {
        return vec![p];
    }
    for &b in basis {
        if encloses_not(p, b) && encloses_weak_all(enclose_basis2(b, p), basis) {
            return vec![b, p];
        }
    }
    for i in 0..basis.len().saturating_sub(1) {
        for j in (i + 1)..basis.len() {
            let (bi, bj) = (basis[i], basis[j]);
            if encloses_not(enclose_basis2(bi, bj), p)
                && encloses_not(enclose_basis2(bi, p), bj)
                && encloses_not(enclose_basis2(bj, p), bi)
                && encloses_weak_all(enclose_basis3(bi, bj, p), basis)
            {
                return vec![bi, bj, p];
            }
        }
    }
    unreachable!("enclose: no basis")
}

fn encloses_not(a: Circle, b: Circle) -> bool {
    let dr = a.r - b.r;
    let dx = b.x - a.x;
    let dy = b.y - a.y;
    dr < 0.0 || dr * dr < dx * dx + dy * dy
}

fn encloses_weak(a: Circle, b: Circle) -> bool {
    let dr = a.r - b.r + a.r.max(b.r).max(1.0) * 1e-9;
    let dx = b.x - a.x;
    let dy = b.y - a.y;
    dr > 0.0 && dr * dr > dx * dx + dy * dy
}

fn encloses_weak_all(a: Circle, basis: &[Circle]) -> bool {
    basis.iter().all(|&b| encloses_weak(a, b))
}

fn enclose_basis(basis: &[Circle]) -> Circle {
    match basis {
        [a] => *a,
        [a, b] => enclose_basis2(*a, *b),
        [a, b, c] => enclose_basis3(*a, *b, *c),
        _ => unreachable!("a front chain's basis holds one to three circles"),
    }
}

fn enclose_basis2(a: Circle, b: Circle) -> Circle {
    let x21 = b.x - a.x;
    let y21 = b.y - a.y;
    let r21 = b.r - a.r;
    let l = (x21 * x21 + y21 * y21).sqrt();
    Circle {
        x: (a.x + b.x + x21 / l * r21) / 2.0,
        y: (a.y + b.y + y21 / l * r21) / 2.0,
        r: (l + a.r + b.r) / 2.0,
    }
}

#[allow(clippy::many_single_char_names)] // mirrors d3-hierarchy's own names
fn enclose_basis3(a: Circle, b: Circle, c: Circle) -> Circle {
    let (x1, y1, r1) = (a.x, a.y, a.r);
    let a2 = x1 - b.x;
    let a3 = x1 - c.x;
    let b2 = y1 - b.y;
    let b3 = y1 - c.y;
    let c2 = b.r - r1;
    let c3 = c.r - r1;
    let d1 = x1 * x1 + y1 * y1 - r1 * r1;
    let d2 = d1 - b.x * b.x - b.y * b.y + b.r * b.r;
    let d3 = d1 - c.x * c.x - c.y * c.y + c.r * c.r;
    let ab = a3 * b2 - a2 * b3;
    let xa = (b2 * d3 - b3 * d2) / (ab * 2.0) - x1;
    let xb = (b3 * c2 - b2 * c3) / ab;
    let ya = (a3 * d2 - a2 * d3) / (ab * 2.0) - y1;
    let yb = (a2 * c3 - a3 * c2) / ab;
    let big_a = xb * xb + yb * yb - 1.0;
    let big_b = 2.0 * (r1 + xa * xb + ya * yb);
    let big_c = xa * xa + ya * ya - r1 * r1;
    let r = -if big_a.abs() > 1e-6 {
        (big_b + (big_b * big_b - 4.0 * big_a * big_c).sqrt()) / (2.0 * big_a)
    } else {
        big_c / big_b
    };
    Circle {
        x: x1 + xa + xb * r,
        y: y1 + ya + yb * r,
        r,
    }
}

#[cfg(test)]
mod tests {
    use super::super::tree::HotspotKind;
    use super::*;
    use std::path::Path;

    fn leaf(name: &str, file: &str, lines: usize) -> HotspotNode {
        HotspotNode {
            name: name.to_string(),
            kind: HotspotKind::File,
            file: PathBuf::from(file),
            lines,
            commits: 0,
            children: Vec::new(),
        }
    }

    #[test]
    fn a_single_leaf_packs_to_one_circle_at_the_origin() {
        let tree = HotspotTree {
            root: leaf("lib.rs", "app/src/lib.rs", 100),
            max_file_commits: 0,
            total_commits: 0,
            hotspots: Vec::new(),
        };

        let packed = pack(&tree);

        assert_eq!(packed.len(), 1);
        assert_eq!(packed[0].file, PathBuf::from("app/src/lib.rs"));
        assert_eq!(packed[0].depth, 0);
        assert!(packed[0].cx.abs() < 1e-9);
        assert!(packed[0].cy.abs() < 1e-9);
        assert!((packed[0].r - 10.0).abs() < 1e-9);
    }

    fn module(name: &str, file: &str, children: Vec<HotspotNode>) -> HotspotNode {
        let lines = children.iter().map(|c| c.lines).sum();
        HotspotNode {
            name: name.to_string(),
            kind: HotspotKind::Module,
            file: PathBuf::from(file),
            lines,
            commits: 0,
            children,
        }
    }

    #[test]
    fn two_siblings_do_not_overlap_and_lie_inside_the_parent() {
        let tree = HotspotTree {
            root: module(
                "app",
                "app/src/mod.rs",
                vec![
                    leaf("mod.rs", "app/src/mod.rs", 4),
                    leaf("big.rs", "app/src/big.rs", 900),
                ],
            ),
            max_file_commits: 0,
            total_commits: 0,
            hotspots: Vec::new(),
        };

        let packed = pack(&tree);

        assert_eq!(packed.len(), 3);
        let root = &packed[0];
        let a = &packed[1];
        let b = &packed[2];

        let gap = ((a.cx - b.cx).powi(2) + (a.cy - b.cy).powi(2)).sqrt() - (a.r + b.r);
        assert!(gap > -1e-6, "siblings overlap by {}", -gap);

        for child in [a, b] {
            let dist = ((child.cx - root.cx).powi(2) + (child.cy - root.cy).powi(2)).sqrt();
            assert!(
                dist + child.r <= root.r + 1e-6,
                "child at ({}, {}) r={} sticks out of parent r={}",
                child.cx,
                child.cy,
                child.r,
                root.r
            );
        }
    }

    fn krate(name: &str, file: &str, children: Vec<HotspotNode>) -> HotspotNode {
        let lines = children.iter().map(|c| c.lines).sum();
        HotspotNode {
            name: name.to_string(),
            kind: HotspotKind::Crate,
            file: PathBuf::from(file),
            lines,
            commits: 0,
            children,
        }
    }

    /// Checks that no sibling pair overlaps and every child lies inside its
    /// parent (band included, since the band only grows the parent's
    /// radius).
    ///
    /// Walks `node` and `packed` together, re-deriving the same
    /// lines-descending, name-tiebreak child order `pack` sorts by, since a
    /// flat `PackedCircle` carries no parent link of its own.
    fn assert_no_overlap_and_contained(
        node: &HotspotNode,
        packed: &[PackedCircle],
        cursor: &mut usize,
    ) -> PackedCircle {
        let this = packed[*cursor].clone();
        *cursor += 1;

        let mut order: Vec<&HotspotNode> = node.children.iter().collect();
        order.sort_by(|a, b| b.lines.cmp(&a.lines).then_with(|| a.name.cmp(&b.name)));
        let child_circles: Vec<PackedCircle> = order
            .into_iter()
            .map(|child| assert_no_overlap_and_contained(child, packed, cursor))
            .collect();

        for (i, a) in child_circles.iter().enumerate() {
            let dist = ((a.cx - this.cx).powi(2) + (a.cy - this.cy).powi(2)).sqrt();
            assert!(
                dist + a.r <= this.r + 1e-6,
                "{:?} sticks out of {:?} by {}",
                a.file,
                this.file,
                dist + a.r - this.r
            );
            for b in &child_circles[i + 1..] {
                let gap = ((a.cx - b.cx).powi(2) + (a.cy - b.cy).powi(2)).sqrt() - (a.r + b.r);
                assert!(
                    gap > -1e-6,
                    "{:?} overlaps {:?} by {}",
                    a.file,
                    b.file,
                    -gap
                );
            }
        }

        this
    }

    #[test]
    fn check_mjs_invariants_hold_over_a_small_tree() {
        let tree = small_tree();

        let packed = pack(&tree);

        assert_eq!(packed.len(), 6);
        let mut cursor = 0;
        assert_no_overlap_and_contained(&tree.root, &packed, &mut cursor);
    }

    fn small_tree() -> HotspotTree {
        HotspotTree {
            root: krate(
                "app",
                "app/Cargo.toml",
                vec![
                    leaf("lib.rs", "app/src/lib.rs", 400),
                    module(
                        "mid",
                        "app/src/mid/mod.rs",
                        vec![
                            leaf("mod.rs", "app/src/mid/mod.rs", 10),
                            leaf("b.rs", "app/src/mid/b.rs", 300),
                        ],
                    ),
                    leaf("d.rs", "app/src/d.rs", 5),
                ],
            ),
            max_file_commits: 0,
            total_commits: 0,
            hotspots: Vec::new(),
        }
    }

    #[test]
    fn packing_the_same_tree_twice_gives_identical_output() {
        let tree = small_tree();

        assert_eq!(pack(&tree), pack(&tree));
    }

    #[test]
    fn siblings_are_packed_in_lines_descending_name_tiebreak_order() {
        let tree = small_tree();

        let packed = pack(&tree);

        let depth1: Vec<&PathBuf> = packed
            .iter()
            .filter(|c| c.depth == 1)
            .map(|c| &c.file)
            .collect();
        assert_eq!(
            depth1,
            vec![
                &PathBuf::from("app/src/lib.rs"),
                &PathBuf::from("app/src/mid/mod.rs"),
                &PathBuf::from("app/src/d.rs"),
            ]
        );
    }

    #[test]
    fn siblings_with_equal_lines_are_packed_by_name() {
        let tree = HotspotTree {
            root: module(
                "app",
                "app/src/mod.rs",
                vec![
                    leaf("b.rs", "app/src/b.rs", 50),
                    leaf("a.rs", "app/src/a.rs", 50),
                ],
            ),
            max_file_commits: 0,
            total_commits: 0,
            hotspots: Vec::new(),
        };

        let packed = pack(&tree);

        let children: Vec<&PathBuf> = packed.iter().skip(1).map(|c| &c.file).collect();
        assert_eq!(
            children,
            vec![
                &PathBuf::from("app/src/a.rs"),
                &PathBuf::from("app/src/b.rs"),
            ]
        );
    }

    /// A workspace transcribed from the prototype's sample data
    /// (`proto/hotspot-map` @ `b4a3557d2f463a1f1c26da79fbb5d019ce55f7ea`,
    /// `proto/hotspots/sample.json`), where a container's own file matches
    /// its self-leaf child's path.
    fn sample_workspace_tree() -> HotspotTree {
        let pass = module(
            "pass",
            "crates/engine/src/render/pass/mod.rs",
            vec![
                leaf("mod.rs", "crates/engine/src/render/pass/mod.rs", 150),
                leaf("depth.rs", "crates/engine/src/render/pass/depth.rs", 95),
            ],
        );
        let render = module(
            "render",
            "crates/engine/src/render/render.rs",
            vec![
                leaf("render.rs", "crates/engine/src/render/render.rs", 1200),
                leaf("pipeline.rs", "crates/engine/src/render/pipeline.rs", 640),
                leaf("shader.rs", "crates/engine/src/render/shader.rs", 310),
                pass,
            ],
        );
        let mesh = module(
            "mesh",
            "crates/engine/src/mesh/mesh.rs",
            vec![
                leaf("mesh.rs", "crates/engine/src/mesh/mesh.rs", 500),
                leaf("vertex.rs", "crates/engine/src/mesh/vertex.rs", 120),
                leaf("index.rs", "crates/engine/src/mesh/index.rs", 0),
            ],
        );
        let engine = krate(
            "engine",
            "crates/engine/Cargo.toml",
            vec![
                render,
                mesh,
                leaf("lib.rs", "crates/engine/src/lib.rs", 80),
                leaf("util.rs", "crates/engine/src/util.rs", 45),
            ],
        );
        let commands = module(
            "commands",
            "crates/cli/src/commands/mod.rs",
            vec![
                leaf("build.rs", "crates/cli/src/commands/build.rs", 410),
                leaf("run.rs", "crates/cli/src/commands/run.rs", 330),
                leaf("mod.rs", "crates/cli/src/commands/mod.rs", 60),
                leaf("check.rs", "crates/cli/src/commands/check.rs", 25),
            ],
        );
        let cli = krate(
            "cli",
            "crates/cli/Cargo.toml",
            vec![
                commands,
                leaf("main.rs", "crates/cli/src/main.rs", 220),
                leaf("args.rs", "crates/cli/src/args.rs", 90),
            ],
        );
        HotspotTree {
            root: HotspotNode {
                name: "sample".to_string(),
                kind: HotspotKind::Workspace,
                file: PathBuf::new(),
                lines: engine.lines + cli.lines,
                commits: 0,
                children: vec![engine, cli],
            },
            max_file_commits: 0,
            total_commits: 0,
            hotspots: Vec::new(),
        }
    }

    /// Finds the one packed circle for `file` at `depth`; a container and
    /// its own self-leaf child share a file, so both are needed to tell
    /// them apart.
    fn packed_at<'a>(packed: &'a [PackedCircle], file: &str, depth: usize) -> &'a PackedCircle {
        packed
            .iter()
            .find(|c| c.file == Path::new(file) && c.depth == depth)
            .unwrap_or_else(|| panic!("no packed circle for {file} at depth {depth}"))
    }

    /// Radii and positions captured by running the prototype's own `layout`
    /// (its `page.html`'s PURE block) over its sample workspace, via Node.
    #[test]
    fn matches_the_prototype_layout_on_sample_json_within_tolerance() {
        let tree = sample_workspace_tree();
        let packed = pack(&tree);

        let tolerance = 1e-3;
        let cases: &[(&str, usize, f64, f64, f64)] = &[
            // (file, depth, radius, cx, cy)
            ("", 0, 203.883_192, 0.0, 0.0),
            (
                "crates/engine/Cargo.toml",
                1,
                126.880_451,
                -70.714_319,
                2.288_422,
            ),
            (
                "crates/cli/Cargo.toml",
                1,
                68.714_319,
                128.880_451,
                2.288_422,
            ),
            (
                "crates/engine/src/render/render.rs",
                2,
                77.315_649,
                -113.990_700,
                4.576_844,
            ),
            (
                "crates/engine/src/mesh/mesh.rs",
                2,
                41.276_381,
                8.601_329,
                4.576_844,
            ),
            (
                "crates/cli/src/commands/mod.rs",
                2,
                45.593_500,
                112.048_054,
                4.576_844,
            ),
            (
                "crates/engine/src/render/pass/mod.rs",
                3,
                29.707_099,
                -106.880_448,
                -34.497_768,
            ),
        ];

        for &(file, depth, radius, cx, cy) in cases {
            let circle = packed_at(&packed, file, depth);
            assert!(
                (circle.r - radius).abs() < tolerance,
                "{file}: radius {} vs prototype {radius}",
                circle.r
            );
            assert!(
                (circle.cx - cx).abs() < tolerance,
                "{file}: cx {} vs prototype {cx}",
                circle.cx
            );
            assert!(
                (circle.cy - cy).abs() < tolerance,
                "{file}: cy {} vs prototype {cy}",
                circle.cy
            );
        }
    }
}
