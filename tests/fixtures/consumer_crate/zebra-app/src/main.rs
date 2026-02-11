use alpha_core::models::Entity;
use beta_service::handlers::handle;
use gamma_util::format::prettify;
use delta_api::routes::get_entity;

fn main() {
    let entity = Entity { id: 1, name: "test".to_string() };
    let result = handle(&entity);
    let formatted = prettify(&result);
    let api_result = get_entity();
    println!("{} {}", formatted, api_result);
}
