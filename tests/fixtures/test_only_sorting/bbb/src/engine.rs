pub fn engine_logic() -> u32 {
    2
}

#[cfg(test)]
mod tests {
    use crate::aaa_support::sample;

    #[test]
    fn test_with_sample() {
        assert_eq!(sample(), 7);
    }
}
