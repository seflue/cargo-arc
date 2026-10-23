pub fn p() {
    crate::tr9::inner::real();
}
#[cfg(test)]
mod tests {
    #[test]
    #[cfg(unix)]
    fn t() {
        crate::tr9::inner::helper();
    }
}
