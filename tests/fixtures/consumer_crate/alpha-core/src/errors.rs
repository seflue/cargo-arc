use crate::models::Entity;

pub enum AppError {
    NotFound(u64),
    InvalidEntity(Entity),
}
