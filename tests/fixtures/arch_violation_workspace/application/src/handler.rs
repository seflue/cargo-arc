use domain::service;

pub fn handle() -> String {
    service::get_data().to_string()
}
