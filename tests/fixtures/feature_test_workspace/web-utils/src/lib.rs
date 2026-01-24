/// Web utilities with optional core dependency.

#[cfg(feature = "web")]
pub fn web_function() -> String {
    format!("web using {}", core::core_function())
}

#[cfg(not(feature = "web"))]
pub fn web_function() -> &'static str {
    "web (no core)"
}
