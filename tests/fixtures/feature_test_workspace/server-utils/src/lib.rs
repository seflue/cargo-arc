/// Server utilities with optional core dependency.

#[cfg(feature = "server")]
pub fn server_function() -> String {
    format!("server using {}", core::core_function())
}

#[cfg(not(feature = "server"))]
pub fn server_function() -> &'static str {
    "server (no core)"
}
