// One file per subcommand.

pub mod serve;
mod setup;

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
    unimplemented!("FEAT-001 Task 5")
}

pub fn status() {
    unimplemented!("FEAT-001 Task 5")
}
