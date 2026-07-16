pub(crate) mod commands;
pub(crate) mod schema;

#[cfg(windows)]
static REDIRECTED_STD_HANDLES: std::sync::OnceLock<Vec<std::fs::File>> = std::sync::OnceLock::new();

pub fn run_cli() {
    let _trace = crate::runtime::phase_trace("cli.process");
    run_cli_inner();
}

fn run_cli_inner() {
    #[cfg(unix)]
    crate::runtime::restore_default_sigpipe();
    #[cfg(windows)]
    if let Err(error) = apply_windows_git_std_redirects() {
        eprintln!("error: {error}");
        std::process::exit(1);
    }
    #[cfg(not(unix))]
    crate::runtime::install_broken_pipe_panic_hook();
    #[cfg(unix)]
    let result = run_main();
    #[cfg(not(unix))]
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
    let raw_args = argv.drain(1..).collect::<Vec<_>>();
    match program_basename(&program).as_str() {
        "git-http-backend" => return commands::transport_commands::http_backend(),
        "git-sh-i18n" => return commands::admin_commands::sh_i18n_command(raw_args),
        "git-sh-setup" => return commands::admin_commands::sh_setup_command(raw_args),
        _ => {}
    }
    if let Some(result) = try_run_builtin_test_tool(&raw_args) {
        return result;
    }
    if let Some(result) = try_run_builtin_http_fetch(&raw_args) {
        return result;
    }
    if let Some(result) = try_run_builtin_query(&raw_args) {
        return result;
    }
    if raw_args.first().map(String::as_str) == Some("refs")
        && raw_args.get(1).map(String::as_str) == Some("list")
        && raw_args[2..]
            .iter()
            .any(|argument| matches!(argument.as_str(), "-h" | "--help"))
    {
        return Err(crate::runtime::CliError::Stderr {
            code: 129,
            text: "usage: git refs list [<options>] [<patterns>]\n".to_owned(),
        });
    }
    let (args, command_args) = {
        let _trace = crate::runtime::phase_trace("cli.parse");
        crate::runtime::parse_cli_invocation(program, &raw_args)?
    };
    let trace2 = crate::runtime::GitTrace2Session::start(&command_args);
    let result = {
        let _trace = crate::runtime::phase_trace("cli.dispatch");
        commands::dispatch(args.command, &command_args)
    };
    let cleanup = {
        let _trace = crate::runtime::phase_trace("cli.cleanup");
        crate::runtime::shutdown_worktree_filter_processes()
    };
    let final_result = match (result, cleanup) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(error), _) => Err(error),
        (Ok(()), Err(error)) => Err(error),
    };
    if let Some(trace2) = &trace2 {
        trace2.finish(cli_result_exit_code(&final_result));
    }
    final_result
}

fn cli_result_exit_code(result: &std::result::Result<(), crate::runtime::CliError>) -> i32 {
    match result {
        Ok(()) => 0,
        Err(crate::runtime::CliError::Exit(code)) => *code,
        Err(crate::runtime::CliError::Fatal { code, .. }) => *code,
        Err(crate::runtime::CliError::Stderr { code, .. }) => *code,
        Err(crate::runtime::CliError::Message(_)) => 1,
        Err(crate::runtime::CliError::Io(error)) => {
            if error.kind() == std::io::ErrorKind::InvalidData {
                128
            } else {
                1
            }
        }
    }
}

fn program_basename(program: &str) -> String {
    let name = std::path::Path::new(program)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(program);
    name.strip_suffix(".exe").unwrap_or(name).to_owned()
}

fn try_run_builtin_test_tool(
    raw_args: &[String],
) -> Option<std::result::Result<(), crate::runtime::CliError>> {
    if raw_args.first().map(String::as_str) != Some("test-tool") {
        return None;
    }

    let result = {
        let _trace = crate::runtime::phase_trace("cli.dispatch.test_tool");
        commands::admin_commands::test_tool_command(raw_args[1..].to_vec())
    };
    let cleanup = {
        let _trace = crate::runtime::phase_trace("cli.cleanup");
        crate::runtime::shutdown_worktree_filter_processes()
    };
    Some(match (result, cleanup) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(error), _) => Err(error),
        (Ok(()), Err(error)) => Err(error),
    })
}

fn try_run_builtin_query(
    raw_args: &[String],
) -> Option<std::result::Result<(), crate::runtime::CliError>> {
    let output = match raw_args {
        [flag] if flag == "--man-path" => Some(query_path_output("man")),
        [flag] if flag == "--html-path" => Some(query_path_output("html")),
        [flag] if flag == "--info-path" => Some(query_path_output("info")),
        _ => None,
    }?;

    let trace_args = vec!["_query_".to_owned()];
    let trace2 = crate::runtime::GitTrace2Session::start(&trace_args);
    let result = {
        let _trace = crate::runtime::phase_trace("cli.dispatch.query");
        println!("{output}");
        Ok(())
    };
    if let Some(trace2) = &trace2 {
        trace2.finish(cli_result_exit_code(&result));
    }
    Some(result)
}

fn query_path_output(kind: &str) -> String {
    let base = std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(std::path::Path::to_path_buf))
        .unwrap_or_else(|| {
            std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."))
        });
    base.join("../share")
        .join(kind)
        .to_string_lossy()
        .into_owned()
}

fn try_run_builtin_http_fetch(
    raw_args: &[String],
) -> Option<std::result::Result<(), crate::runtime::CliError>> {
    if raw_args.first().map(String::as_str) != Some("http-fetch") {
        return None;
    }

    let parsed = parse_builtin_http_fetch_options(&raw_args[1..]);
    let result = match parsed {
        Ok(options) => {
            commands::register_runtime_services();
            crate::runtime::clear_pending_trace2_state();
            crate::runtime::override_pending_trace2_command(
                "_run_dashed_".to_owned(),
                "_run_dashed_".to_owned(),
            );
            crate::runtime::push_pending_trace2_perf_child_start(
                0,
                "main".to_owned(),
                "dashed".to_owned(),
                "git-http-fetch".to_owned(),
            );
            crate::runtime::push_pending_trace2_perf_cmd_name(
                1,
                "main".to_owned(),
                "http-fetch".to_owned(),
                "_run_dashed_/http-fetch".to_owned(),
            );
            crate::runtime::push_pending_trace2_perf_def_params(1, "main".to_owned());

            let trace2 = crate::runtime::GitTrace2Session::start(raw_args);
            let result = {
                let _trace = crate::runtime::phase_trace("cli.dispatch.http_fetch");
                commands::transport_commands::http_fetch(options)
            };
            if let Some(trace2) = &trace2 {
                trace2.finish(cli_result_exit_code(&result));
            }
            result
        }
        Err(error) => Err(error),
    };
    Some(result)
}

fn parse_builtin_http_fetch_options(
    args: &[String],
) -> std::result::Result<commands::transport_commands::HttpFetchOptions, crate::runtime::CliError> {
    let mut options = commands::transport_commands::HttpFetchOptions {
        commit: false,
        tags: false,
        all: false,
        verbose: false,
        recover: false,
        write_ref: Vec::new(),
        stdin: false,
        packfile: None,
        index_pack_args: Vec::new(),
        args: Vec::new(),
    };
    let mut index = 0;
    while index < args.len() {
        let arg = &args[index];
        match arg.as_str() {
            "-c" => options.commit = true,
            "-t" => options.tags = true,
            "-a" => options.all = true,
            "-v" => options.verbose = true,
            "--recover" => options.recover = true,
            "--stdin" => options.stdin = true,
            "-w" => {
                let Some(value) = args.get(index + 1) else {
                    return Err(http_fetch_usage_error());
                };
                options.write_ref.push(value.clone());
                index += 1;
            }
            "--" => {
                options.args.extend(args[index + 1..].iter().cloned());
                break;
            }
            _ if arg.starts_with("--packfile=") => {
                options.packfile = Some(arg["--packfile=".len()..].to_owned());
            }
            _ if arg.starts_with("--index-pack-args=") => {
                options
                    .index_pack_args
                    .push(arg["--index-pack-args=".len()..].to_owned());
            }
            _ if arg.starts_with('-') => return Err(http_fetch_usage_error()),
            _ => options.args.push(arg.clone()),
        }
        index += 1;
    }
    Ok(options)
}

fn http_fetch_usage_error() -> crate::runtime::CliError {
    crate::runtime::CliError::Stderr {
        code: 129,
        text: "usage: git http-fetch [-c] [-t] [-a] [-v] [--recover] [-w ref] [--stdin | --packfile=hash | commit-id] url\n".into(),
    }
}
