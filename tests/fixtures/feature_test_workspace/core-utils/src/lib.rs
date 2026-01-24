/// Core utilities that always depend on core.
pub fn core_util() -> String {
    format!("core-utils using {}", core::core_function())
}
