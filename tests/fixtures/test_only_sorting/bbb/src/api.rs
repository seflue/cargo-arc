pub fn api_logic() -> u32 {
    1
}

#[cfg(test)]
mod tests {
    use crate::aaa_support::sample;

    #[test]
    fn test_with_sample() {
        assert_eq!(sample(), 7);
    }
}
