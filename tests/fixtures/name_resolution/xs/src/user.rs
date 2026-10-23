pub fn shared() -> u64 {
    xs::pc::shared()
}

#[cfg(test)]
mod tests {
    #[test]
    fn t() {
        let _: u16 = xs::fake::helper();
        let _: u32 = xs::tfake::helper2();
    }
}
