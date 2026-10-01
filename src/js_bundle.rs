//! The page scripts, bundled from `js/` by `build.rs`.

/// The bundled script for the `entry` module in `js/` (file stem, e.g.
/// `"svg_script"`).
///
/// # Panics
///
/// Panics if `entry` is not one of the bundled entry modules.
pub(crate) fn bundle(entry: &str) -> &'static str {
    match entry {
        "svg_script" => include_str!(concat!(env!("OUT_DIR"), "/svg_script.js")),
        "hotspot_script" => include_str!(concat!(env!("OUT_DIR"), "/hotspot_script.js")),
        other => panic!("unknown JS bundle '{other}'"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[should_panic(expected = "unknown JS bundle 'no_such_entry'")]
    fn bundle_of_unknown_entry_panics() {
        let _ = bundle("no_such_entry");
    }
}
