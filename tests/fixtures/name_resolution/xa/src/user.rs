#[cfg(test)]
mod tests {
    #[test]
    fn t() {
        let _: u8 = xb::thing();
        let _: u16 = crate::imp::inner::thing();
    }
}
