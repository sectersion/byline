mod cli;
mod derive;
mod hooks;
mod ingest;
mod report;
mod store;

use std::process::exit;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let arg = args.first().map(String::as_str);
    let no_plugins = args.iter().any(|a| a == "--no-plugins");
    match arg {
        Some("serve") => cli::serve(),
        Some("setup") => cli::setup(no_plugins),
        Some("unsetup") => cli::unsetup(),
        Some("report") => cli::report(),
        Some("status") => cli::status(),
        _ => {
            eprintln!("usage: byline <serve|setup|unsetup|report|status>");
            exit(2);
        }
    }
}
