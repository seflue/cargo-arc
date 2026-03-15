use infra::db;

pub fn get_data() -> &'static str {
    db::connect()
}
