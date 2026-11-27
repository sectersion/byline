use std::process::exit;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let arg = args.first().map(String::as_str);
    let no_plugins = args.iter().any(|a| a == "--no-plugins");
    match arg {
        Some("serve") => byline::cli::serve(),
        Some("setup") => byline::cli::setup(no_plugins),
        Some("unsetup") => byline::cli::unsetup(),
        Some("report") => byline::cli::report(),
        Some("export") => byline::cli::export(),
        Some("status") => byline::cli::status(),
        Some("explain") => {
            let session_id = args.get(1).cloned().unwrap_or_default();
            byline::cli::explain(&session_id);
        }
        _ => {
            eprintln!("usage: byline <serve|setup|unsetup|report|export|status|explain> [session-id]");
            exit(2);
        }
    }
}
