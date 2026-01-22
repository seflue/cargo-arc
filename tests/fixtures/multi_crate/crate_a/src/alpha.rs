use crate::beta;

pub fn process() -> String {
    format!("alpha calls {}", beta::helper())
}
