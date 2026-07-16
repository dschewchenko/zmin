mod cli;
mod compat;
mod runtime;

#[cfg(test)]
#[path = "../tests/support/stock_git.rs"]
pub mod stock_git_support;

pub use cli::run_cli;
