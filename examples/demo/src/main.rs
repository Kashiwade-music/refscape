mod config;
mod pipeline;

use config::load_config;
use pipeline::run;

fn main() {
    let config = load_config();
    let report = run(config);
    println!("{}", report.summary());
}
