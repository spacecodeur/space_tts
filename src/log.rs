use std::sync::atomic::{AtomicBool, Ordering};

static DEBUG: AtomicBool = AtomicBool::new(false);

pub fn set_debug(enabled: bool) {
    DEBUG.store(enabled, Ordering::SeqCst);
}

pub fn is_debug() -> bool {
    DEBUG.load(Ordering::SeqCst)
}

macro_rules! info {
    ($($arg:tt)*) => {
        eprintln!($($arg)*)
    };
}

macro_rules! debug {
    ($($arg:tt)*) => {
        if $crate::log::is_debug() {
            eprintln!($($arg)*)
        }
    };
}

// #[macro_export] places the macro at the crate root, avoiding
// the name-collision with the built-in #[warn] attribute that
// prevents `pub(crate) use warn;` from compiling.
#[macro_export]
macro_rules! warn {
    ($($arg:tt)*) => {{
        eprint!("\x1b[33mWARNING:\x1b[0m ");
        eprintln!($($arg)*)
    }};
}

pub(crate) use debug;
pub(crate) use info;
