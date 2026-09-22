//! The transitive `@deps` closure of one entry module.

use std::collections::{HashMap, HashSet};

use super::table::{JsModule, MODULES};

/// The entry module (named by its file stem, e.g. `"svg_script"`) and its
/// transitive `@deps` closure, dependencies before dependents.
///
/// `MODULES` is already topologically sorted (build.rs's `topo_sort`), so
/// filtering it down to the reachable set preserves that order for the
/// subset.
///
/// # Panics
///
/// Panics if `entry` names no module in `MODULES`.
pub(crate) fn bundle(entry: &str) -> impl Iterator<Item = &'static JsModule> {
    let by_name: HashMap<&'static str, &'static JsModule> =
        MODULES.iter().map(|m| (m.name, m)).collect();

    let entry_module = MODULES
        .iter()
        .find(|m| m.file_stem == entry)
        .unwrap_or_else(|| panic!("unknown JS module '{entry}'"));

    let mut needed: HashSet<&'static str> = HashSet::new();
    let mut stack = vec![entry_module.name];
    while let Some(name) = stack.pop() {
        if !needed.insert(name) {
            continue;
        }
        // build.rs's topo_sort already validated every @deps name against
        // MODULES, so a name reached here is always present.
        let module = by_name[name];
        for &dep in module.deps {
            stack.push(dep);
        }
    }

    MODULES.iter().filter(move |m| needed.contains(m.name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundle_of_svg_script_covers_known_modules() {
        let bundled: HashSet<&str> = bundle("svg_script").map(|m| m.name).collect();
        let expected = [
            "AppState",
            "ArcLogic",
            "DerivedState",
            "DomAdapter",
            "Follow",
            "HighlightLogic",
            "HighlightRenderer",
            "Jump",
            "JumpIcons",
            "LayerManager",
            "Selectors",
            "SearchLogic",
            "SidebarLogic",
            "StaticData",
            "SvgScript",
            "SwitchToggles",
            "TextMeasure",
            "Theme",
            "TreeLogic",
            "VirtualEdgeLogic",
            "ViewSnapshot",
        ];
        for name in expected {
            assert!(
                bundled.contains(name),
                "expected '{name}' in svg_script's bundle"
            );
        }
    }

    #[test]
    #[should_panic(expected = "unknown JS module 'no_such_entry'")]
    fn bundle_of_unknown_entry_panics() {
        let _ = bundle("no_such_entry").count();
    }

    /// `hotspot_jump_icon.js` draws its own copy of the `#jump-icon` symbol
    /// rather than calling into `JumpIcons`, so the hotspot page's bundle
    /// must not carry that module's popover and chip code at all.
    #[test]
    fn bundle_of_hotspot_script_excludes_jump_icons() {
        let bundled: HashSet<&str> = bundle("hotspot_script").map(|m| m.name).collect();
        assert!(
            !bundled.contains("JumpIcons"),
            "hotspot_script's bundle should not pull in JumpIcons, got: {bundled:?}"
        );
    }
}
