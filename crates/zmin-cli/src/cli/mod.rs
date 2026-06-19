pub(crate) mod commands;
pub(crate) mod schema;

const CLI_STACK_SIZE: usize = 16 * 1024 * 1024;

#[cfg(windows)]
static REDIRECTED_STD_HANDLES: std::sync::OnceLock<Vec<std::fs::File>> = std::sync::OnceLock::new();

#[cfg(windows)]
pub fn run_cli() {
    let _trace = crate::runtime::phase_trace("cli.process");
    let handle = std::thread::Builder::new()
        .name("zmin-cli-main".to_owned())
        .stack_size(CLI_STACK_SIZE)
        .spawn(run_cli_inner)
        .expect("spawn Windows CLI thread");
    if let Err(payload) = handle.join() {
        std::panic::resume_unwind(payload);
    }
}

#[cfg(not(windows))]
pub fn run_cli() {
    let _trace = crate::runtime::phase_trace("cli.process");
    let handle = std::thread::Builder::new()
        .name("zmin-cli-main".to_owned())
        .stack_size(CLI_STACK_SIZE)
        .spawn(run_cli_inner)
        .expect("spawn CLI thread");
    if let Err(payload) = handle.join() {
        std::panic::resume_unwind(payload);
    }
}

fn run_cli_inner() {
    commands::register_runtime_services();
    #[cfg(windows)]
    if let Err(error) = apply_windows_git_std_redirects() {
        eprintln!("error: {error}");
        std::process::exit(1);
    }
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

#[cfg(windows)]
fn apply_windows_git_std_redirects() -> std::io::Result<()> {
    use std::os::windows::io::AsRawHandle;

    const STD_OUTPUT_HANDLE: u32 = -11_i32 as u32;
    const STD_ERROR_HANDLE: u32 = -12_i32 as u32;

    unsafe extern "system" {
        fn SetStdHandle(n_std_handle: u32, handle: *mut std::ffi::c_void) -> i32;
        fn GetStdHandle(n_std_handle: u32) -> *mut std::ffi::c_void;
    }

    fn redirected_file(value: &str) -> std::io::Result<Option<std::fs::File>> {
        if value.is_empty() {
            return Ok(None);
        }
        let path = if value == "off" { "NUL" } else { value };
        std::fs::OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(path)
            .map(Some)
    }

    let mut files = Vec::new();
    if let Some(value) =
        std::env::var_os("GIT_REDIRECT_STDOUT").and_then(|value| value.into_string().ok())
        && let Some(file) = redirected_file(&value)?
    {
        let handle = file.as_raw_handle();
        // SAFETY: the handle comes from a live File stored for the process lifetime below.
        unsafe {
            SetStdHandle(STD_OUTPUT_HANDLE, handle.cast());
        }
        files.push(file);
    }
    if let Some(value) =
        std::env::var_os("GIT_REDIRECT_STDERR").and_then(|value| value.into_string().ok())
    {
        if value == "2>&1" {
            // SAFETY: GetStdHandle/SetStdHandle are called with documented standard-handle ids.
            unsafe {
                let stdout = GetStdHandle(STD_OUTPUT_HANDLE);
                SetStdHandle(STD_ERROR_HANDLE, stdout);
            }
        } else if let Some(file) = redirected_file(&value)? {
            let handle = file.as_raw_handle();
            // SAFETY: the handle comes from a live File stored for the process lifetime below.
            unsafe {
                SetStdHandle(STD_ERROR_HANDLE, handle.cast());
            }
            files.push(file);
        }
    }
    let _ = REDIRECTED_STD_HANDLES.set(files);
    Ok(())
}

pub(crate) fn command_definition() -> clap::Command {
    crate::runtime::command_definition()
}

fn run_main() -> std::result::Result<(), crate::runtime::CliError> {
    let _trace = crate::runtime::phase_trace("cli.total");
    let mut argv = std::env::args().collect::<Vec<_>>();
    let program = argv.first().cloned().unwrap_or_else(|| "zmin".to_owned());
    if program_basename(&program) == "git-http-backend" {
        return commands::transport_commands::http_backend();
    }
    let raw_args = argv.drain(1..).collect::<Vec<_>>();
    let (args, command_args) = {
        let _trace = crate::runtime::phase_trace("cli.parse");
        crate::runtime::parse_cli_invocation(program, &raw_args)?
    };
    let result = {
        let _trace = crate::runtime::phase_trace("cli.dispatch");
        commands::dispatch(args.command, &command_args)
    };
    let cleanup = {
        let _trace = crate::runtime::phase_trace("cli.cleanup");
        crate::runtime::shutdown_worktree_filter_processes()
    };
    match (result, cleanup) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(error), _) => Err(error),
        (Ok(()), Err(error)) => Err(error),
    }
}

fn program_basename(program: &str) -> String {
    let name = std::path::Path::new(program)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(program);
    name.strip_suffix(".exe").unwrap_or(name).to_owned()
}
