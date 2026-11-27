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
        _ => {
            eprintln!("usage: byline <serve|setup|unsetup|report|export|status>");
            exit(2);
        }
    }
}
