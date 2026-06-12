pub(crate) mod commands;
pub(crate) mod schema;

#[cfg(windows)]
const WINDOWS_CLI_STACK_SIZE: usize = 16 * 1024 * 1024;

#[cfg(windows)]
pub fn run_cli() {
    let handle = std::thread::Builder::new()
        .name("skron-cli-main".to_owned())
        .stack_size(WINDOWS_CLI_STACK_SIZE)
        .spawn(run_cli_inner)
        .expect("spawn Windows CLI thread");
    if let Err(payload) = handle.join() {
        std::panic::resume_unwind(payload);
    }
}

#[cfg(not(windows))]
pub fn run_cli() {
    run_cli_inner();
}

fn run_cli_inner() {
    crate::runtime::install_broken_pipe_panic_hook();
    let result = match std::panic::catch_unwind(run_main) {
        Ok(result) => result,
        Err(payload)
            if crate::runtime::broken_pipe_panic_triggered()
                || crate::runtime::panic_payload_is_broken_pipe(&payload) =>
        {
            std::process::exit(0)
        }
        Err(payload) => std::panic::resume_unwind(payload),
    };
    match result {
        Ok(()) => {}
        Err(crate::runtime::CliError::Exit(code)) => std::process::exit(code),
        Err(crate::runtime::CliError::Fatal { code, message }) => {
            eprintln!("fatal: {message}");
            std::process::exit(code);
        }
        Err(crate::runtime::CliError::Stderr { code, text }) => {
            eprint!("{text}");
            std::process::exit(code);
        }
        Err(crate::runtime::CliError::Message(message)) => {
            eprintln!("error: {message}");
            std::process::exit(1);
        }
        Err(crate::runtime::CliError::Io(error)) => {
            if error.kind() == std::io::ErrorKind::InvalidData {
                eprintln!("fatal: {error}");
                std::process::exit(128);
            }
            eprintln!("error: {error}");
            std::process::exit(1);
        }
    }
}

pub(crate) fn command_definition() -> clap::Command {
    crate::runtime::command_definition()
}

fn run_main() -> std::result::Result<(), crate::runtime::CliError> {
    let mut argv = std::env::args().collect::<Vec<_>>();
    let program = argv
        .first()
        .cloned()
        .unwrap_or_else(|| "skron-git".to_owned());
    let raw_args = argv.drain(1..).collect::<Vec<_>>();
    let (args, command_args) = crate::runtime::parse_cli_invocation(program, &raw_args)?;
    commands::dispatch(args.command, &command_args)
}
