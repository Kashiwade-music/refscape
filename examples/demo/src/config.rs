/// Settings used by the report pipeline.
pub struct Config {
    pub title: String,
    pub values: Vec<i32>,
}

/// Click this function's name to find all callers.
pub fn load_config() -> Config {
    Config {
        title: "Refscape demo".to_string(),
        values: vec![12, 24, 36],
    }
}
