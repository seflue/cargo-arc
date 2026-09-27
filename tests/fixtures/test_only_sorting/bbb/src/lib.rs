pub mod aaa_support;
pub mod api;
pub mod engine;

#[cfg(test)]
mod tests {
    use aaa_kit::kit;

    #[test]
    fn test_with_kit() {
        let _ = kit();
    }
}
