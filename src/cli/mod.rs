// One file per subcommand.

pub mod serve;
mod setup;

pub mod report;
pub mod status;

pub fn serve() {
    if let Err(e) = serve::run() {
        eprintln!("byline serve: {e}");
        std::process::exit(1);
    }
}

pub fn setup() {
    if let Err(e) = setup::run() {
        eprintln!("byline setup: {e}");
        std::process::exit(1);
    }
}

pub fn unsetup() {
    if let Err(e) = setup::unsetup() {
        eprintln!("byline unsetup: {e}");
        std::process::exit(1);
    }
}

pub fn report() {
    if let Err(e) = report::run() {
        eprintln!("byline report: {e}");
        std::process::exit(1);
    }
}

pub fn status() {
    match status::open_store() {
        Ok((store, _)) => println!("{}", status::one_line(&store)),
        Err(e) => {
            eprintln!("byline status: {e}");
            std::process::exit(1);
        }
    }
}
