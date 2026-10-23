use crate::c::f;
pub fn p() -> u16 { 1 }
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn t() { let _: u16 = f(); }
}
