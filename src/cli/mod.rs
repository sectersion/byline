// One file per subcommand.

pub mod serve;

pub fn serve() {
    if let Err(e) = serve::run() {
        eprintln!("byline serve: {e}");
        std::process::exit(1);
    }
}

pub fn setup() {
    unimplemented!("FEAT-001 Task 4")
}

pub fn unsetup() {
    unimplemented!("FEAT-001 Task 4")
}

pub fn report() {
    unimplemented!("FEAT-001 Task 5")
}

pub fn status() {
    unimplemented!("FEAT-001 Task 5")
}
