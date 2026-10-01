use crate::config::Config;

pub struct Report {
    title: String,
    total: i32,
    count: usize,
}

impl Report {
    pub fn summary(&self) -> String {
        format!("{}: {} items, total {}", self.title, self.count, self.total)
    }
}

/// Expand the calls to follow data through the pipeline.
pub fn run(config: Config) -> Report {
    let total = calculate_total(&config.values);
    Report {
        title: config.title,
        total,
        count: config.values.len(),
    }
}

fn calculate_total(values: &[i32]) -> i32 {
    values.iter().sum()
}
