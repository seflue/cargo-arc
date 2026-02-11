use alpha_core::models::Entity;

pub fn handle(entity: &Entity) -> String {
    format!("Handling entity {}", entity.name)
}
