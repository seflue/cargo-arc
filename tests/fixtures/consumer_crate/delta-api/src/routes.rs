use alpha_core::models::Entity;
use gamma_util::format::prettify;

pub fn get_entity() -> String {
    let e = Entity { id: 1, name: "test".to_string() };
    prettify(&e.name)
}
