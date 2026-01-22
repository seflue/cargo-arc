use crate_b::gamma;

pub fn helper() -> String {
    format!("beta calls {}", gamma::compute())
}
