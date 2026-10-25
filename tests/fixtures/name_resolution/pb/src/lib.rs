mod cli;
mod ext;
mod lock_first;
mod lock_glob;
mod lock_last;
mod locker;
mod modes;
mod other;
mod user;

pub(crate) use cli::*;
pub(crate) use link::*;

pub enum Mode {
    A,
    B,
}

pub fn run() -> bool {
    user::f(LinkerFlavor::Gnu);
    cli::build::b(cli::po::Cwpo);
    cli::takes(cli::po::Cwpo);
    other::g();
    modes::run();
    ext::run();
    locker::f(&lock_last::Mutex, &lock_first::Mutex, &lock_glob::Mutex);
    matches!(Mode::A, Mode::A)
}
