pub fn p() {
    crate::tr2::inner::real();
}
#[cfg(test)]
mod tests {
    #[test]
    fn t() {
        crate::tr2::inner::mock();
        crate::tr2::inner::helper();
    }
}
