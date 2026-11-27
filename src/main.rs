mod cli;
mod derive;
mod hooks;
mod ingest;
mod report;
mod store;

use std::process::exit;

fn main() {
    let arg = std::env::args().nth(1);
    match arg.as_deref() {
        Some("serve") => cli::serve(),
        Some("setup") => cli::setup(),
        Some("unsetup") => cli::unsetup(),
        Some("report") => cli::report(),
        Some("status") => cli::status(),
        _ => {
            eprintln!("usage: byline <serve|setup|unsetup|report|status>");
            exit(2);
        }
    }
}
