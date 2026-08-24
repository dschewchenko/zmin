//! Runtime adapters for the typed LFS auth and transfer boundaries.
//!
//! Process invocation is deliberately direct (Command, never a shell), and
//! every process input/output is bounded. Credentials are requested using
//! only a canonical origin by default.  When `credential.useHttpPath` is
//! enabled, the validated endpoint path is included exactly as Git LFS sends
//! it to `git credential`; signed query strings never enter the protocol.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fmt;
use std::io::Read;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStderr, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, sync_channel};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use super::{
    LfsAuthCredentials, LfsAuthError, LfsAuthHeaders, LfsAuthOrigin, LfsBatchError,
    LfsCredentialProvider, LfsHttpUrl, LfsOperation, LfsSshCommandOutput, LfsSshCommandRunner,
    LfsSshDestination, LfsTransferAuthHeaders,
};

const MAX_CREDENTIAL_INPUT_BYTES: usize = 8 * 1024;
const MAX_CREDENTIAL_OUTPUT_BYTES: usize = 64 * 1024;
const MAX_CREDENTIAL_FIELD_BYTES: usize = 16 * 1024;
const MAX_CREDENTIAL_RECEIPTS: usize = 8;
const MAX_PROCESS_ARGUMENT_BYTES: usize = 8 * 1024;
const PROCESS_POLL_INTERVAL: Duration = Duration::from_millis(5);
const PROCESS_CAPTURE_DRAIN_GRACE: Duration = Duration::from_millis(250);
const PROCESS_CLEANUP_GRACE: Duration = Duration::from_millis(100);

/// The sole conversion policy from auth headers to transfer headers.
///
/// Both sides are already validated, so this adapter performs no map merge;
/// the transfer client remains the only layer that combines auth and
/// action-specific headers for an HTTP request.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct LfsAuthTransferPolicy;

impl LfsAuthTransferPolicy {
    pub(crate) fn to_transfer(
        credentials: &LfsAuthCredentials,
    ) -> Result<LfsTransferAuthHeaders, LfsAuthError> {
        Self::to_transfer_headers(credentials.headers())
    }

    pub(crate) fn to_transfer_headers(
        headers: &LfsAuthHeaders,
    ) -> Result<LfsTransferAuthHeaders, LfsAuthError> {
        Self::from_headers(headers)
    }

    fn from_headers(headers: &LfsAuthHeaders) -> Result<LfsTransferAuthHeaders, LfsAuthError> {
        let pairs = headers
            .iter()
            .map(|(name, value)| (name.to_owned(), value.to_vec()))
            .collect();
        LfsTransferAuthHeaders::from_pairs(pairs).map_err(map_batch_error)
    }
}

fn map_batch_error(_error: LfsBatchError) -> LfsAuthError {
    LfsAuthError::InvalidHeader
}

/// Direct adapter for the Git credential protocol.
#[derive(Clone)]
pub(crate) struct GitCredentialHelper {
    program: PathBuf,
    timeout: Duration,
    receipts: Arc<Mutex<BTreeMap<CredentialReceiptKey, CredentialPair>>>,
}

impl fmt::Debug for GitCredentialHelper {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("GitCredentialHelper")
            .field("program", &self.program)
            .field("timeout", &self.timeout)
            .field("receipts", &"<redacted>")
            .finish()
    }
}

impl GitCredentialHelper {
    pub(crate) fn new(
        program: impl Into<PathBuf>,
        timeout: Duration,
    ) -> Result<Self, LfsAuthError> {
        if timeout.is_zero() {
            return Err(LfsAuthError::Timeout);
        }
        let program = program.into();
        if program.as_os_str().is_empty() {
            return Err(LfsAuthError::ProcessFailed);
        }
        Ok(Self {
            program,
            timeout,
            receipts: Arc::new(Mutex::new(BTreeMap::new())),
        })
    }

    pub(crate) fn system(timeout: Duration) -> Result<Self, LfsAuthError> {
        Self::new(PathBuf::from("git"), timeout)
    }

    pub(crate) fn fill(
        &self,
        endpoint: &LfsHttpUrl,
        origin: &LfsAuthOrigin,
    ) -> Result<Option<LfsAuthCredentials>, LfsAuthError> {
        self.fill_with_receipt(endpoint, origin)
            .map(|result| result.map(|(credentials, _receipt)| credentials))
    }

    fn fill_with_receipt(
        &self,
        endpoint: &LfsHttpUrl,
        origin: &LfsAuthOrigin,
    ) -> Result<Option<(LfsAuthCredentials, CredentialPair)>, LfsAuthError> {
        validate_endpoint_origin(endpoint, origin)?;
        let input = credential_input(origin, None, None)?;
        let output = run_bounded_credential_process(
            &self.program,
            &[OsString::from("credential"), OsString::from("fill")],
            &input,
            self.timeout,
            MAX_CREDENTIAL_OUTPUT_BYTES,
        )?;
        if output.status != 0 {
            return Err(LfsAuthError::ProcessFailed);
        }
        let Some(pair) = parse_credential_fill(&output.stdout)? else {
            return Ok(None);
        };
        let encoded = basic_authorization_value(&pair.username, &pair.password)?;
        let headers = LfsAuthHeaders::from_pairs(vec![("Authorization".to_owned(), encoded)])?;
        Ok(Some((
            LfsAuthCredentials::new(headers, super::LfsAuthExpiry::never()),
            pair,
        )))
    }

    /// Store credentials through git credential approve.
    pub(crate) fn approve(
        &self,
        endpoint: &LfsHttpUrl,
        origin: &LfsAuthOrigin,
        username: &[u8],
        password: &[u8],
    ) -> Result<(), LfsAuthError> {
        validate_endpoint_origin(endpoint, origin)?;
        let input = credential_input(origin, Some(username), Some(password))?;
        let result = self.run_action("approve", &input);
        let mut input = input;
        wipe(&mut input);
        result
    }

    /// Remove credentials through git credential reject.
    pub(crate) fn reject(
        &self,
        endpoint: &LfsHttpUrl,
        origin: &LfsAuthOrigin,
        username: Option<&[u8]>,
    ) -> Result<(), LfsAuthError> {
        validate_endpoint_origin(endpoint, origin)?;
        let input = credential_input(origin, username, None)?;
        let result = self.run_action("reject", &input);
        let mut input = input;
        wipe(&mut input);
        result
    }

    pub(crate) fn approve_cached(
        &self,
        endpoint: &LfsHttpUrl,
        origin: &LfsAuthOrigin,
        operation: LfsOperation,
    ) -> Result<(), LfsAuthError> {
        let Some(receipt) = self.take_receipt(origin, operation)? else {
            return Ok(());
        };
        self.approve(endpoint, origin, &receipt.username, &receipt.password)
    }

    pub(crate) fn reject_cached(
        &self,
        endpoint: &LfsHttpUrl,
        origin: &LfsAuthOrigin,
        operation: LfsOperation,
    ) -> Result<(), LfsAuthError> {
        let receipt = self.take_receipt(origin, operation)?;
        self.reject(
            endpoint,
            origin,
            receipt.as_ref().map(|receipt| receipt.username.as_slice()),
        )
    }

    fn take_receipt(
        &self,
        origin: &LfsAuthOrigin,
        operation: LfsOperation,
    ) -> Result<Option<CredentialPair>, LfsAuthError> {
        self.receipts
            .lock()
            .map_err(|_| LfsAuthError::ProcessFailed)
            .map(|mut receipts| receipts.remove(&CredentialReceiptKey::new(origin, operation)))
    }

    fn run_action(&self, action: &str, input: &[u8]) -> Result<(), LfsAuthError> {
        let output = run_bounded_credential_process(
            &self.program,
            &[OsString::from("credential"), OsString::from(action)],
            input,
            self.timeout,
            MAX_CREDENTIAL_OUTPUT_BYTES,
        )?;
        if output.status == 0 {
            Ok(())
        } else {
            Err(LfsAuthError::ProcessFailed)
        }
    }
}

impl LfsCredentialProvider for GitCredentialHelper {
    fn credentials(
        &self,
        endpoint: &LfsHttpUrl,
        origin: &LfsAuthOrigin,
        operation: LfsOperation,
    ) -> Result<Option<LfsAuthCredentials>, LfsAuthError> {
        let Some((credentials, receipt)) = self.fill_with_receipt(endpoint, origin)? else {
            return Ok(None);
        };
        let mut receipts = self
            .receipts
            .lock()
            .map_err(|_| LfsAuthError::ProcessFailed)?;
        if receipts.len() >= MAX_CREDENTIAL_RECEIPTS
            && !receipts.contains_key(&CredentialReceiptKey::new(origin, operation))
            && let Some(oldest) = receipts.keys().next().cloned()
        {
            receipts.remove(&oldest);
        }
        receipts.insert(CredentialReceiptKey::new(origin, operation), receipt);
        Ok(Some(credentials))
    }
}

#[derive(Clone, Eq, Ord, PartialEq, PartialOrd)]
struct CredentialReceiptKey {
    origin: LfsAuthOrigin,
    operation: u8,
}

impl CredentialReceiptKey {
    fn new(origin: &LfsAuthOrigin, operation: LfsOperation) -> Self {
        Self {
            origin: origin.clone(),
            operation: match operation {
                LfsOperation::Fetch => 0,
                LfsOperation::Push => 1,
            },
        }
    }
}

fn validate_endpoint_origin(
    endpoint: &LfsHttpUrl,
    origin: &LfsAuthOrigin,
) -> Result<(), LfsAuthError> {
    let expected =
        LfsAuthOrigin::from_endpoint_with_http_path(endpoint, origin.credential_path().is_some())?;
    if &expected == origin {
        Ok(())
    } else {
        Err(LfsAuthError::InvalidOrigin)
    }
}

fn credential_input(
    origin: &LfsAuthOrigin,
    username: Option<&[u8]>,
    password: Option<&[u8]>,
) -> Result<Vec<u8>, LfsAuthError> {
    let mut input = Vec::new();
    let result: Result<(), LfsAuthError> = (|| {
        append_credential_line(&mut input, b"protocol", origin.scheme().as_bytes())?;
        append_credential_line(&mut input, b"host", credential_host(origin).as_bytes())?;
        if let Some(path) = origin.credential_path() {
            append_credential_line(&mut input, b"path", path)?;
        }
        if let Some(username) = username {
            append_credential_line(&mut input, b"username", username)?;
        }
        if let Some(password) = password {
            append_credential_line(&mut input, b"password", password)?;
        }
        Ok(())
    })();
    if let Err(error) = result {
        wipe(&mut input);
        return Err(error);
    }
    input.push(b'\n');
    if input.len() > MAX_CREDENTIAL_INPUT_BYTES {
        wipe(&mut input);
        return Err(LfsAuthError::HeaderTooLarge);
    }
    Ok(input)
}

fn credential_host(origin: &LfsAuthOrigin) -> String {
    let default_port = match origin.scheme() {
        "http" => ":80",
        "https" => ":443",
        _ => return origin.authority().to_owned(),
    };
    origin
        .authority()
        .strip_suffix(default_port)
        .unwrap_or(origin.authority())
        .to_owned()
}

fn append_credential_line(
    output: &mut Vec<u8>,
    key: &[u8],
    value: &[u8],
) -> Result<(), LfsAuthError> {
    if matches!(key, b"username" | b"password") {
        validate_basic_credential_value(value)?;
    }
    if (value.is_empty() && !matches!(key, b"password" | b"path"))
        || value.len() > MAX_CREDENTIAL_FIELD_BYTES
        || value.iter().any(|byte| matches!(*byte, 0 | b'\r' | b'\n'))
    {
        return Err(LfsAuthError::InvalidField);
    }
    output.extend_from_slice(key);
    output.push(b'=');
    output.extend_from_slice(value);
    output.push(b'\n');
    Ok(())
}

struct CredentialPair {
    username: Vec<u8>,
    password: Vec<u8>,
}

impl Drop for CredentialPair {
    fn drop(&mut self) {
        wipe(&mut self.username);
        wipe(&mut self.password);
    }
}

struct CredentialParts {
    username: Option<Vec<u8>>,
    password: Option<Vec<u8>>,
}

impl Drop for CredentialParts {
    fn drop(&mut self) {
        if let Some(username) = self.username.as_mut() {
            wipe(username);
        }
        if let Some(password) = self.password.as_mut() {
            wipe(password);
        }
    }
}

fn parse_credential_fill(output: &[u8]) -> Result<Option<CredentialPair>, LfsAuthError> {
    if output.len() > MAX_CREDENTIAL_OUTPUT_BYTES {
        return Err(LfsAuthError::OutputTooLarge);
    }
    let mut parts = CredentialParts {
        username: None,
        password: None,
    };
    let mut quit = false;
    for raw_line in output.split(|byte| *byte == b'\n') {
        let line = raw_line.strip_suffix(b"\r").unwrap_or(raw_line);
        if line.is_empty() {
            continue;
        }
        let separator = line
            .iter()
            .position(|byte| *byte == b'=')
            .ok_or(LfsAuthError::InvalidField)?;
        let (key, value) = (&line[..separator], &line[separator + 1..]);
        if key.is_empty() || !key.iter().all(u8::is_ascii_alphanumeric) {
            return Err(LfsAuthError::InvalidField);
        }
        if value.len() > MAX_CREDENTIAL_FIELD_BYTES
            || value
                .iter()
                .any(|byte| *byte == 0 || byte.is_ascii_control())
        {
            return Err(LfsAuthError::InvalidField);
        }
        match key {
            b"username" => {
                if parts.username.is_some() {
                    return Err(LfsAuthError::DuplicateField);
                }
                validate_basic_credential_value(value)?;
                parts.username = Some(value.to_vec());
            }
            b"password" => {
                if parts.password.is_some() {
                    return Err(LfsAuthError::DuplicateField);
                }
                validate_basic_credential_value(value)?;
                parts.password = Some(value.to_vec());
            }
            b"quit" => {
                if value == b"1" || value.eq_ignore_ascii_case(b"true") {
                    quit = true;
                }
            }
            // Helpers commonly echo the request context. It is validated and
            // ignored; no other returned field is used as an auth header.
            b"protocol" | b"host" | b"path" | b"url" => {}
            _ => {}
        }
    }
    if quit {
        return Ok(None);
    }
    match (parts.username.take(), parts.password.take()) {
        (Some(username), Some(password)) => Ok(Some(CredentialPair { username, password })),
        _ => Ok(None),
    }
}

fn validate_basic_credential_value(value: &[u8]) -> Result<(), LfsAuthError> {
    let value = std::str::from_utf8(value).map_err(|_| LfsAuthError::InvalidField)?;
    if value.len() > MAX_CREDENTIAL_FIELD_BYTES || value.chars().any(char::is_control) {
        return Err(LfsAuthError::InvalidField);
    }
    Ok(())
}

fn basic_authorization_value(username: &[u8], password: &[u8]) -> Result<Vec<u8>, LfsAuthError> {
    validate_basic_credential_value(username)?;
    validate_basic_credential_value(password)?;
    if username.contains(&b':') {
        return Err(LfsAuthError::InvalidField);
    }
    let credential_len = username
        .len()
        .checked_add(1)
        .and_then(|length| length.checked_add(password.len()))
        .ok_or(LfsAuthError::HeaderTooLarge)?;
    let encoded_len = credential_len
        .checked_add(2)
        .map(|length| length / 3)
        .and_then(|length| length.checked_mul(4))
        .and_then(|length| length.checked_add(b"Basic ".len()))
        .ok_or(LfsAuthError::HeaderTooLarge)?;
    if encoded_len > super::LFS_AUTH_MAX_HEADER_VALUE_BYTES {
        return Err(LfsAuthError::HeaderTooLarge);
    }
    let mut credential = Vec::with_capacity(credential_len);
    credential.extend_from_slice(username);
    credential.push(b':');
    credential.extend_from_slice(password);
    let mut encoded = Vec::with_capacity(encoded_len);
    encoded.extend_from_slice(b"Basic ");
    append_base64(&mut encoded, &credential);
    wipe(&mut credential);
    Ok(encoded)
}

fn append_base64(output: &mut Vec<u8>, input: &[u8]) {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    for chunk in input.chunks(3) {
        let first = chunk[0];
        output.push(TABLE[(first >> 2) as usize]);
        let second = chunk.get(1).copied();
        output.push(TABLE[((first & 0x03) << 4 | second.map_or(0, |byte| byte >> 4)) as usize]);
        if let Some(second) = second {
            let third = chunk.get(2).copied();
            output.push(TABLE[((second & 0x0f) << 2 | third.map_or(0, |byte| byte >> 6)) as usize]);
            output.push(third.map_or(b'=', |byte| TABLE[(byte & 0x3f) as usize]));
        } else {
            output.extend_from_slice(b"==");
        }
    }
}

/// Direct OpenSSH adapter for the Git LFS git-lfs-authenticate command.
#[derive(Clone, Debug)]
pub(crate) struct GitSshCommandRunner {
    program: PathBuf,
}

impl GitSshCommandRunner {
    pub(crate) fn new(program: impl Into<PathBuf>) -> Result<Self, LfsAuthError> {
        let program = program.into();
        if program.as_os_str().is_empty() {
            return Err(LfsAuthError::ProcessFailed);
        }
        Ok(Self { program })
    }

    pub(crate) fn system() -> Result<Self, LfsAuthError> {
        Self::new(PathBuf::from("ssh"))
    }
}

impl LfsSshCommandRunner for GitSshCommandRunner {
    fn run(
        &self,
        destination: &LfsSshDestination,
        program: &str,
        args: &[String],
        timeout: Duration,
        stdout_limit: usize,
        stderr_limit: usize,
    ) -> Result<LfsSshCommandOutput, LfsAuthError> {
        if program != "git-lfs-authenticate"
            || args.len() != 2
            || (args[1] != "download" && args[1] != "upload")
        {
            return Err(LfsAuthError::InvalidCommandPath);
        }
        for arg in args {
            validate_remote_argument(arg)?;
        }
        if destination.host().starts_with('-')
            || destination.user().is_some_and(|user| user.starts_with('-'))
        {
            return Err(LfsAuthError::InvalidCommandPath);
        }
        let mut command_args = vec![OsString::from("-o"), OsString::from("BatchMode=yes")];
        if let Some(port) = destination.port() {
            command_args.push(OsString::from("-p"));
            command_args.push(OsString::from(port.to_string()));
        }
        let host = if destination.host().contains(':') && !destination.host().starts_with('[') {
            format!("[{}]", destination.host())
        } else {
            destination.host().to_owned()
        };
        let target = destination
            .user()
            .map(|user| format!("{user}@{host}"))
            .unwrap_or(host);
        command_args.push(OsString::from(target));
        command_args.push(OsString::from(program));
        command_args.extend(args.iter().map(OsString::from));
        run_bounded_process(
            &self.program,
            &command_args,
            &[],
            timeout,
            stdout_limit,
            stderr_limit,
        )
        .map(BoundedProcessOutput::into_ssh_output)
    }
}

fn validate_remote_argument(value: &str) -> Result<(), LfsAuthError> {
    if value.is_empty()
        || value.len() > MAX_PROCESS_ARGUMENT_BYTES
        || value.bytes().any(|byte| {
            byte == 0
                || byte.is_ascii_control()
                || byte.is_ascii_whitespace()
                || matches!(
                    byte,
                    b'\''
                        | b'"'
                        | 0x60
                        | b'$'
                        | b';'
                        | b'&'
                        | b'|'
                        | b'<'
                        | b'>'
                        | b'*'
                        | b'?'
                        | b'['
                        | b']'
                        | b'('
                        | b')'
                        | b'!'
                        | b'~'
                        | b'#'
                        | b'\\'
                )
        })
    {
        return Err(LfsAuthError::InvalidCommandPath);
    }
    Ok(())
}

struct BoundedProcessOutput {
    status: i32,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

impl BoundedProcessOutput {
    fn into_ssh_output(mut self) -> LfsSshCommandOutput {
        LfsSshCommandOutput {
            status: self.status,
            stdout: std::mem::take(&mut self.stdout),
            stderr: std::mem::take(&mut self.stderr),
        }
    }
}

impl Drop for BoundedProcessOutput {
    fn drop(&mut self) {
        wipe(&mut self.stdout);
        wipe(&mut self.stderr);
    }
}

#[cfg(unix)]
struct ProcessContainment;

#[cfg(unix)]
impl ProcessContainment {
    fn prepare(command: &mut Command) -> Result<Self, LfsAuthError> {
        use std::os::unix::process::CommandExt;

        unsafe {
            command.pre_exec(|| {
                if libc::setpgid(0, 0) == -1 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        Ok(Self)
    }

    fn prepare_credential(command: &mut Command) -> Result<Self, LfsAuthError> {
        Self::prepare(command)
    }

    fn start(&self, _child: &Child, _deadline: Instant) -> Result<(), LfsAuthError> {
        Ok(())
    }

    fn terminate_tree(&self, child_id: u32) {
        if let Ok(process_group) = libc::pid_t::try_from(child_id) {
            unsafe {
                let _ = libc::kill(-process_group, libc::SIGKILL);
            }
        }
    }
}

#[cfg(unix)]
struct ProcessTaskCancellation {
    cancelled: Arc<AtomicBool>,
}

#[cfg(unix)]
impl ProcessTaskCancellation {
    fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }
}

#[cfg(unix)]
impl Drop for ProcessTaskCancellation {
    fn drop(&mut self) {
        self.cancel();
    }
}

#[cfg(unix)]
fn set_process_pipe_nonblocking(descriptor: std::os::fd::RawFd) -> Result<(), LfsAuthError> {
    let flags = unsafe { libc::fcntl(descriptor, libc::F_GETFL) };
    if flags == -1
        || unsafe { libc::fcntl(descriptor, libc::F_SETFL, flags | libc::O_NONBLOCK) } == -1
    {
        return Err(LfsAuthError::ProcessFailed);
    }
    Ok(())
}

#[cfg(unix)]
fn spawn_io_task<I, T, F>(io: I, operation: F) -> Result<ProcessTask<T>, LfsAuthError>
where
    I: std::os::fd::AsRawFd + Send + 'static,
    T: Send + 'static,
    F: FnOnce(I, &AtomicBool) -> T + Send + 'static,
{
    set_process_pipe_nonblocking(io.as_raw_fd())?;
    let cancelled = Arc::new(AtomicBool::new(false));
    let worker_cancelled = Arc::clone(&cancelled);
    let (sender, receiver) = sync_channel(1);
    thread::spawn(move || {
        let result = operation(io, &worker_cancelled);
        let _ = sender.send(result);
    });
    Ok(ProcessTask {
        receiver,
        cancellation: ProcessTaskCancellation { cancelled },
    })
}

#[cfg(windows)]
struct WindowsOwnedHandle(windows_sys::Win32::Foundation::HANDLE);

#[cfg(windows)]
unsafe impl Send for WindowsOwnedHandle {}

#[cfg(windows)]
impl WindowsOwnedHandle {
    fn new(handle: windows_sys::Win32::Foundation::HANDLE) -> Result<Self, LfsAuthError> {
        use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;

        if handle.is_null() || handle == INVALID_HANDLE_VALUE {
            Err(LfsAuthError::ProcessFailed)
        } else {
            Ok(Self(handle))
        }
    }

    fn raw(&self) -> windows_sys::Win32::Foundation::HANDLE {
        self.0
    }
}

#[cfg(windows)]
impl Drop for WindowsOwnedHandle {
    fn drop(&mut self) {
        unsafe {
            let _ = windows_sys::Win32::Foundation::CloseHandle(self.0);
        }
    }
}

#[cfg(windows)]
fn duplicate_windows_handle(
    source: windows_sys::Win32::Foundation::HANDLE,
) -> Result<WindowsOwnedHandle, LfsAuthError> {
    use windows_sys::Win32::Foundation::{DUPLICATE_SAME_ACCESS, DuplicateHandle};
    use windows_sys::Win32::System::Threading::GetCurrentProcess;

    let process = unsafe { GetCurrentProcess() };
    let mut duplicate = std::ptr::null_mut();
    let duplicated = unsafe {
        DuplicateHandle(
            process,
            source,
            process,
            &mut duplicate,
            0,
            0,
            DUPLICATE_SAME_ACCESS,
        )
    };
    if duplicated == 0 {
        Err(LfsAuthError::ProcessFailed)
    } else {
        WindowsOwnedHandle::new(duplicate)
    }
}

#[cfg(windows)]
fn duplicate_current_windows_thread() -> Result<WindowsOwnedHandle, LfsAuthError> {
    use windows_sys::Win32::System::Threading::GetCurrentThread;

    duplicate_windows_handle(unsafe { GetCurrentThread() })
}

#[cfg(windows)]
struct ProcessTaskCancellation {
    cancelled: Arc<AtomicBool>,
    io: WindowsOwnedHandle,
    thread: WindowsOwnedHandle,
}

#[cfg(windows)]
impl ProcessTaskCancellation {
    fn cancel(&self) {
        use windows_sys::Win32::System::IO::{CancelIoEx, CancelSynchronousIo};

        if self.cancelled.swap(true, Ordering::AcqRel) {
            return;
        }
        unsafe {
            let _ = CancelIoEx(self.io.raw(), std::ptr::null());
            let _ = CancelSynchronousIo(self.thread.raw());
        }
    }
}

#[cfg(windows)]
impl Drop for ProcessTaskCancellation {
    fn drop(&mut self) {
        self.cancel();
    }
}

#[cfg(windows)]
fn spawn_io_task<I, T, F>(
    io: I,
    operation: F,
    setup_deadline: Instant,
) -> Result<ProcessTask<T>, LfsAuthError>
where
    I: std::os::windows::io::AsRawHandle + Send + 'static,
    T: Send + 'static,
    F: FnOnce(I, &AtomicBool) -> T + Send + 'static,
{
    use std::os::windows::io::AsRawHandle;

    let io_handle = duplicate_windows_handle(io.as_raw_handle())?;
    let cancelled = Arc::new(AtomicBool::new(false));
    let worker_cancelled = Arc::clone(&cancelled);
    let (sender, receiver) = sync_channel(1);
    let (setup_sender, setup_receiver) = sync_channel(1);
    thread::spawn(move || {
        let thread_handle = duplicate_current_windows_thread();
        if setup_sender.send(thread_handle).is_err() {
            return;
        }
        let result = operation(io, &worker_cancelled);
        let _ = sender.send(result);
    });
    let thread = match receive_until_deadline(&setup_receiver, setup_deadline) {
        Ok(result) => result?,
        Err(RecvTimeoutError::Timeout) => return Err(LfsAuthError::Timeout),
        Err(RecvTimeoutError::Disconnected) => return Err(LfsAuthError::ProcessFailed),
    };
    Ok(ProcessTask {
        receiver,
        cancellation: ProcessTaskCancellation {
            cancelled,
            io: io_handle,
            thread,
        },
    })
}

#[cfg(windows)]
struct ProcessContainment {
    job: WindowsOwnedHandle,
}

#[cfg(windows)]
impl ProcessContainment {
    fn prepare(command: &mut Command) -> Result<Self, LfsAuthError> {
        Self::prepare_with_kill_on_close(command, true)
    }

    fn prepare_credential(command: &mut Command) -> Result<Self, LfsAuthError> {
        Self::prepare_with_kill_on_close(command, false)
    }

    fn prepare_with_kill_on_close(
        command: &mut Command,
        kill_on_close: bool,
    ) -> Result<Self, LfsAuthError> {
        use std::mem::size_of;
        use std::os::windows::process::CommandExt;
        use windows_sys::Win32::System::JobObjects::{
            CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
            JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
            SetInformationJobObject,
        };
        use windows_sys::Win32::System::Threading::CREATE_SUSPENDED;

        command.creation_flags(CREATE_SUSPENDED);
        let job = WindowsOwnedHandle::new(unsafe {
            CreateJobObjectW(std::ptr::null(), std::ptr::null())
        })?;
        let mut information = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        if kill_on_close {
            information.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        }
        let configured = unsafe {
            SetInformationJobObject(
                job.raw(),
                JobObjectExtendedLimitInformation,
                (&raw const information).cast(),
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
        };
        if configured == 0 {
            return Err(LfsAuthError::ProcessFailed);
        }
        Ok(Self { job })
    }

    fn start(&self, child: &Child, deadline: Instant) -> Result<(), LfsAuthError> {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::System::JobObjects::AssignProcessToJobObject;
        use windows_sys::Win32::System::Threading::ResumeThread;

        let assigned = unsafe { AssignProcessToJobObject(self.job.raw(), child.as_raw_handle()) };
        if assigned == 0 {
            return Err(LfsAuthError::ProcessFailed);
        }
        let thread = suspended_windows_main_thread(child.id(), deadline)?;
        if unsafe { ResumeThread(thread.raw()) } == u32::MAX {
            return Err(LfsAuthError::ProcessFailed);
        }
        Ok(())
    }

    fn terminate_tree(&self, _child_id: u32) {
        unsafe {
            let _ = windows_sys::Win32::System::JobObjects::TerminateJobObject(self.job.raw(), 1);
        }
    }
}

#[cfg(windows)]
fn suspended_windows_main_thread(
    process_id: u32,
    deadline: Instant,
) -> Result<WindowsOwnedHandle, LfsAuthError> {
    use std::mem::size_of;
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, TH32CS_SNAPTHREAD, THREADENTRY32, Thread32First, Thread32Next,
    };
    use windows_sys::Win32::System::Threading::{OpenThread, THREAD_SUSPEND_RESUME};

    loop {
        let snapshot =
            WindowsOwnedHandle::new(unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) })?;
        let mut entry = THREADENTRY32 {
            dwSize: size_of::<THREADENTRY32>() as u32,
            ..THREADENTRY32::default()
        };
        let mut found = unsafe { Thread32First(snapshot.raw(), &mut entry) } != 0;
        while found {
            if entry.th32OwnerProcessID == process_id {
                let thread = unsafe { OpenThread(THREAD_SUSPEND_RESUME, 0, entry.th32ThreadID) };
                if let Ok(thread) = WindowsOwnedHandle::new(thread) {
                    return Ok(thread);
                }
            }
            found = unsafe { Thread32Next(snapshot.raw(), &mut entry) } != 0;
        }
        if Instant::now() >= deadline {
            return Err(LfsAuthError::Timeout);
        }
        thread::sleep(PROCESS_POLL_INTERVAL);
    }
}

#[cfg(not(any(unix, windows)))]
struct ProcessContainment;

#[cfg(not(any(unix, windows)))]
impl ProcessContainment {
    fn prepare(_command: &mut Command) -> Result<Self, LfsAuthError> {
        Err(LfsAuthError::ProcessFailed)
    }

    fn prepare_credential(_command: &mut Command) -> Result<Self, LfsAuthError> {
        Err(LfsAuthError::ProcessFailed)
    }

    fn start(&self, _child: &Child, _deadline: Instant) -> Result<(), LfsAuthError> {
        Err(LfsAuthError::ProcessFailed)
    }

    fn terminate_tree(&self, _child_id: u32) {}
}

#[cfg(not(any(unix, windows)))]
struct ProcessTaskCancellation;

#[cfg(not(any(unix, windows)))]
impl ProcessTaskCancellation {
    fn cancel(&self) {}
}

fn run_bounded_process(
    program: &Path,
    args: &[OsString],
    input: &[u8],
    timeout: Duration,
    stdout_limit: usize,
    stderr_limit: usize,
) -> Result<BoundedProcessOutput, LfsAuthError> {
    if timeout.is_zero() {
        return Err(LfsAuthError::Timeout);
    }
    let started = Instant::now();
    let hard_deadline = started.checked_add(timeout).ok_or(LfsAuthError::Timeout)?;
    // Reserve a small portion of the caller's wall-time budget for killing
    // the process tree, reaping the direct child, and cancelling pipe tasks.
    // Cleanup is therefore included in `timeout`, rather than extending it.
    let cleanup_budget = std::cmp::min(PROCESS_CLEANUP_GRACE, timeout / 10);
    let deadline = hard_deadline.checked_sub(cleanup_budget).unwrap_or(started);
    let mut command = Command::new(program);
    command
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let containment = ProcessContainment::prepare(&mut command)?;
    let mut child = command.spawn().map_err(|_| LfsAuthError::ProcessFailed)?;
    let stdout = child.stdout.take().ok_or(LfsAuthError::ProcessFailed)?;
    let stderr = child.stderr.take().ok_or(LfsAuthError::ProcessFailed)?;
    let stdin = child.stdin.take().ok_or(LfsAuthError::ProcessFailed)?;
    let oversized = Arc::new(AtomicBool::new(false));
    let reader_failed = Arc::new(AtomicBool::new(false));
    let tasks = match ProcessTasks::spawn(
        stdin,
        stdout,
        stderr,
        input,
        stdout_limit,
        stderr_limit,
        Arc::clone(&oversized),
        Arc::clone(&reader_failed),
        deadline,
    ) {
        Ok(tasks) => tasks,
        Err(error) => {
            terminate_and_reap(&containment, &mut child, hard_deadline);
            return Err(error);
        }
    };
    if let Err(error) = containment.start(&child, deadline) {
        terminate_and_reap(&containment, &mut child, hard_deadline);
        drop(tasks);
        return Err(error);
    }

    let completion = loop {
        if oversized.load(Ordering::Acquire) {
            break ProcessCompletion::failed(LfsAuthError::OutputTooLarge);
        }
        if reader_failed.load(Ordering::Acquire) {
            break ProcessCompletion::failed(LfsAuthError::ProcessFailed);
        }
        let before_poll = Instant::now();
        if before_poll >= deadline {
            break ProcessCompletion::failed(LfsAuthError::Timeout);
        }
        let observation = match child.try_wait() {
            Ok(Some(status)) => ProcessPollObservation::Exited(status.code().unwrap_or(-1)),
            Ok(None) => ProcessPollObservation::Running,
            Err(_) => ProcessPollObservation::Failed,
        };
        let after_poll = Instant::now();
        if let Some(completion) =
            classify_process_poll(observation, before_poll, after_poll, deadline)
        {
            break completion;
        }
        thread::sleep(PROCESS_POLL_INTERVAL);
    };
    containment.terminate_tree(child.id());
    if !completion.reaped {
        let _ = child.kill();
    }
    let cleanup_deadline = cleanup_deadline(hard_deadline);
    let reap = if completion.reaped {
        Ok(())
    } else {
        reap_direct_child_until(&mut child, cleanup_deadline)
    };
    let drain_deadline = earlier_deadline(
        cleanup_deadline,
        Instant::now()
            .checked_add(PROCESS_CAPTURE_DRAIN_GRACE)
            .unwrap_or(cleanup_deadline),
    );
    let captured = tasks.collect(drain_deadline, cleanup_deadline);

    if oversized.load(Ordering::Acquire) {
        return Err(LfsAuthError::OutputTooLarge);
    }
    let status = completion.status?;
    reap?;
    captured.into_output(status)
}

/// Run `git credential` with Git LFS's stderr policy.
///
/// The credential-cache helper may start a daemon that inherits stderr.  The
/// official LFS adapter therefore inherits stderr instead of piping it, and a
/// successful direct helper exit must not terminate that descendant.  Failed
/// or timed-out helpers still use the same process-group/job containment as
/// the SSH runner so they cannot outlive the bounded operation.
fn run_bounded_credential_process(
    program: &Path,
    args: &[OsString],
    input: &[u8],
    timeout: Duration,
    stdout_limit: usize,
) -> Result<BoundedProcessOutput, LfsAuthError> {
    if timeout.is_zero() {
        return Err(LfsAuthError::Timeout);
    }
    let started = Instant::now();
    let hard_deadline = started.checked_add(timeout).ok_or(LfsAuthError::Timeout)?;
    let cleanup_budget = std::cmp::min(PROCESS_CLEANUP_GRACE, timeout / 10);
    let deadline = hard_deadline.checked_sub(cleanup_budget).unwrap_or(started);
    let mut command = Command::new(program);
    command
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit());
    let containment = ProcessContainment::prepare_credential(&mut command)?;
    let mut child = command.spawn().map_err(|_| LfsAuthError::ProcessFailed)?;
    let stdout = child.stdout.take().ok_or(LfsAuthError::ProcessFailed)?;
    let stdin = child.stdin.take().ok_or(LfsAuthError::ProcessFailed)?;
    let oversized = Arc::new(AtomicBool::new(false));
    let reader_failed = Arc::new(AtomicBool::new(false));
    let tasks = match CredentialProcessTasks::spawn(
        stdin,
        stdout,
        input,
        stdout_limit,
        Arc::clone(&oversized),
        Arc::clone(&reader_failed),
        deadline,
    ) {
        Ok(tasks) => tasks,
        Err(error) => {
            terminate_and_reap(&containment, &mut child, hard_deadline);
            return Err(error);
        }
    };
    if let Err(error) = containment.start(&child, deadline) {
        terminate_and_reap(&containment, &mut child, hard_deadline);
        drop(tasks);
        return Err(error);
    }

    let completion = loop {
        if oversized.load(Ordering::Acquire) {
            break ProcessCompletion::failed(LfsAuthError::OutputTooLarge);
        }
        if reader_failed.load(Ordering::Acquire) {
            break ProcessCompletion::failed(LfsAuthError::ProcessFailed);
        }
        let before_poll = Instant::now();
        if before_poll >= deadline {
            break ProcessCompletion::failed(LfsAuthError::Timeout);
        }
        let observation = match child.try_wait() {
            Ok(Some(status)) => ProcessPollObservation::Exited(status.code().unwrap_or(-1)),
            Ok(None) => ProcessPollObservation::Running,
            Err(_) => ProcessPollObservation::Failed,
        };
        let after_poll = Instant::now();
        if let Some(completion) =
            classify_process_poll(observation, before_poll, after_poll, deadline)
        {
            break completion;
        }
        thread::sleep(PROCESS_POLL_INTERVAL);
    };

    let successful = completion.reaped && completion.status == Ok(0);
    if !successful {
        containment.terminate_tree(child.id());
    }
    if !completion.reaped {
        let _ = child.kill();
    }
    let cleanup_deadline = cleanup_deadline(hard_deadline);
    let reap = if completion.reaped {
        Ok(())
    } else {
        reap_direct_child_until(&mut child, cleanup_deadline)
    };
    let captured = tasks.collect(cleanup_deadline, cleanup_deadline);

    if oversized.load(Ordering::Acquire) {
        return Err(LfsAuthError::OutputTooLarge);
    }
    let status = completion.status?;
    reap?;
    captured.into_output(status)
}

struct ProcessCompletion {
    status: Result<i32, LfsAuthError>,
    reaped: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ProcessPollObservation {
    Running,
    Exited(i32),
    Failed,
}

fn classify_process_poll(
    observation: ProcessPollObservation,
    before_poll: Instant,
    after_poll: Instant,
    deadline: Instant,
) -> Option<ProcessCompletion> {
    if before_poll >= deadline {
        return Some(ProcessCompletion::failed(LfsAuthError::Timeout));
    }
    if after_poll >= deadline {
        return Some(match observation {
            ProcessPollObservation::Exited(_) => {
                ProcessCompletion::failed_after_reap(LfsAuthError::Timeout)
            }
            ProcessPollObservation::Running | ProcessPollObservation::Failed => {
                ProcessCompletion::failed(LfsAuthError::Timeout)
            }
        });
    }
    match observation {
        ProcessPollObservation::Exited(status) => Some(ProcessCompletion::reaped(status)),
        ProcessPollObservation::Running => None,
        ProcessPollObservation::Failed => {
            Some(ProcessCompletion::failed(LfsAuthError::ProcessFailed))
        }
    }
}

impl ProcessCompletion {
    fn reaped(status: i32) -> Self {
        Self {
            status: Ok(status),
            reaped: true,
        }
    }

    fn failed(error: LfsAuthError) -> Self {
        Self {
            status: Err(error),
            reaped: false,
        }
    }

    fn failed_after_reap(error: LfsAuthError) -> Self {
        Self {
            status: Err(error),
            reaped: true,
        }
    }
}

struct SensitiveProcessBuffer(Vec<u8>);

impl SensitiveProcessBuffer {
    fn new() -> Self {
        Self(Vec::new())
    }

    fn len(&self) -> usize {
        self.0.len()
    }

    fn extend_from_slice(&mut self, bytes: &[u8]) {
        self.0.extend_from_slice(bytes);
    }

    fn take(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.0)
    }
}

impl Drop for SensitiveProcessBuffer {
    fn drop(&mut self) {
        wipe(&mut self.0);
    }
}

struct SensitiveStackBuffer<const SIZE: usize>([u8; SIZE]);

impl<const SIZE: usize> SensitiveStackBuffer<SIZE> {
    fn zeroed() -> Self {
        Self([0; SIZE])
    }
}

impl<const SIZE: usize> Drop for SensitiveStackBuffer<SIZE> {
    fn drop(&mut self) {
        wipe(&mut self.0);
    }
}

struct ProcessTasks {
    stdin: ProcessTask<()>,
    stdout: ProcessTask<Result<SensitiveProcessBuffer, LfsAuthError>>,
    stderr: ProcessTask<Result<SensitiveProcessBuffer, LfsAuthError>>,
}

struct CredentialProcessTasks {
    stdin: ProcessTask<()>,
    stdout: ProcessTask<Result<SensitiveProcessBuffer, LfsAuthError>>,
}

impl CredentialProcessTasks {
    fn spawn(
        stdin: ChildStdin,
        stdout: ChildStdout,
        input: &[u8],
        stdout_limit: usize,
        oversized: Arc<AtomicBool>,
        reader_failed: Arc<AtomicBool>,
        setup_deadline: Instant,
    ) -> Result<Self, LfsAuthError> {
        Ok(Self {
            stdin: spawn_writer(stdin, input, setup_deadline)?,
            stdout: spawn_reader(
                stdout,
                stdout_limit,
                oversized,
                reader_failed,
                setup_deadline,
            )?,
        })
    }

    fn collect(
        self,
        drain_deadline: Instant,
        hard_deadline: Instant,
    ) -> CapturedCredentialProcessTasks {
        CapturedCredentialProcessTasks {
            stdout: self
                .stdout
                .receive_until(drain_deadline, hard_deadline)
                .and_then(std::convert::identity),
            stdin: self
                .stdin
                .receive_until(drain_deadline, hard_deadline)
                .map(|_| ()),
        }
    }
}

struct CapturedCredentialProcessTasks {
    stdin: Result<(), LfsAuthError>,
    stdout: Result<SensitiveProcessBuffer, LfsAuthError>,
}

impl CapturedCredentialProcessTasks {
    fn into_output(mut self, status: i32) -> Result<BoundedProcessOutput, LfsAuthError> {
        self.stdin?;
        if let Err(error) = &self.stdout {
            return Err(*error);
        }
        let stdout = self
            .stdout
            .as_mut()
            .expect("validated stdout capture")
            .take();
        Ok(BoundedProcessOutput {
            status,
            stdout,
            stderr: Vec::new(),
        })
    }
}

impl ProcessTasks {
    #[allow(clippy::too_many_arguments)]
    fn spawn(
        stdin: ChildStdin,
        stdout: ChildStdout,
        stderr: ChildStderr,
        input: &[u8],
        stdout_limit: usize,
        stderr_limit: usize,
        oversized: Arc<AtomicBool>,
        reader_failed: Arc<AtomicBool>,
        setup_deadline: Instant,
    ) -> Result<Self, LfsAuthError> {
        let stdin = spawn_writer(stdin, input, setup_deadline)?;
        let stdout = spawn_reader(
            stdout,
            stdout_limit,
            Arc::clone(&oversized),
            Arc::clone(&reader_failed),
            setup_deadline,
        )?;
        let stderr = spawn_reader(
            stderr,
            stderr_limit,
            oversized,
            reader_failed,
            setup_deadline,
        )?;
        Ok(Self {
            stdin,
            stdout,
            stderr,
        })
    }

    fn collect(self, drain_deadline: Instant, hard_deadline: Instant) -> CapturedProcessTasks {
        CapturedProcessTasks {
            stdout: self
                .stdout
                .receive_until(drain_deadline, hard_deadline)
                .and_then(std::convert::identity),
            stderr: self
                .stderr
                .receive_until(drain_deadline, hard_deadline)
                .and_then(std::convert::identity),
            // Broken stdin is compatible with a child that deliberately exits
            // before consuming all input. Collection itself must still finish.
            stdin: self
                .stdin
                .receive_until(drain_deadline, hard_deadline)
                .map(|_| ()),
        }
    }
}

struct CapturedProcessTasks {
    stdin: Result<(), LfsAuthError>,
    stdout: Result<SensitiveProcessBuffer, LfsAuthError>,
    stderr: Result<SensitiveProcessBuffer, LfsAuthError>,
}

impl CapturedProcessTasks {
    fn into_output(mut self, status: i32) -> Result<BoundedProcessOutput, LfsAuthError> {
        self.stdin?;
        if let Err(error) = &self.stdout {
            return Err(*error);
        }
        if let Err(error) = &self.stderr {
            return Err(*error);
        }
        let stdout = self
            .stdout
            .as_mut()
            .expect("validated stdout capture")
            .take();
        let stderr = self
            .stderr
            .as_mut()
            .expect("validated stderr capture")
            .take();
        Ok(BoundedProcessOutput {
            status,
            stdout,
            stderr,
        })
    }
}

struct ProcessTask<T> {
    receiver: Receiver<T>,
    cancellation: ProcessTaskCancellation,
}

impl<T> ProcessTask<T> {
    fn receive_until(
        self,
        drain_deadline: Instant,
        hard_deadline: Instant,
    ) -> Result<T, LfsAuthError> {
        let Self {
            receiver,
            cancellation,
        } = self;
        match receive_until_deadline(&receiver, drain_deadline) {
            Ok(result) => Ok(result),
            Err(RecvTimeoutError::Disconnected) => Err(LfsAuthError::ProcessFailed),
            Err(RecvTimeoutError::Timeout) => {
                cancellation.cancel();
                receive_until_deadline(&receiver, hard_deadline)
                    .map_err(|_| LfsAuthError::ProcessFailed)
            }
        }
    }
}

fn receive_until_deadline<T>(
    receiver: &Receiver<T>,
    deadline: Instant,
) -> Result<T, RecvTimeoutError> {
    receiver.recv_timeout(deadline.saturating_duration_since(Instant::now()))
}

#[cfg(unix)]
fn spawn_reader<R>(
    reader: R,
    limit: usize,
    oversized: Arc<AtomicBool>,
    reader_failed: Arc<AtomicBool>,
    _setup_deadline: Instant,
) -> Result<ProcessTask<Result<SensitiveProcessBuffer, LfsAuthError>>, LfsAuthError>
where
    R: Read + std::os::fd::AsRawFd + Send + 'static,
{
    spawn_io_task(reader, move |reader, cancelled| {
        read_process_output(reader, limit, cancelled, oversized, reader_failed)
    })
}

#[cfg(windows)]
fn spawn_reader<R>(
    reader: R,
    limit: usize,
    oversized: Arc<AtomicBool>,
    reader_failed: Arc<AtomicBool>,
    setup_deadline: Instant,
) -> Result<ProcessTask<Result<SensitiveProcessBuffer, LfsAuthError>>, LfsAuthError>
where
    R: Read + std::os::windows::io::AsRawHandle + Send + 'static,
{
    spawn_io_task(
        reader,
        move |reader, cancelled| {
            read_process_output(reader, limit, cancelled, oversized, reader_failed)
        },
        setup_deadline,
    )
}

#[cfg(not(any(unix, windows)))]
fn spawn_reader<R>(
    _reader: R,
    _limit: usize,
    _oversized: Arc<AtomicBool>,
    _reader_failed: Arc<AtomicBool>,
    _setup_deadline: Instant,
) -> Result<ProcessTask<Result<SensitiveProcessBuffer, LfsAuthError>>, LfsAuthError>
where
    R: Read + Send + 'static,
{
    Err(LfsAuthError::ProcessFailed)
}

#[cfg(unix)]
fn spawn_writer(
    writer: ChildStdin,
    input: &[u8],
    _setup_deadline: Instant,
) -> Result<ProcessTask<()>, LfsAuthError> {
    let input = SensitiveProcessBuffer(input.to_vec());
    spawn_io_task(writer, move |writer, cancelled| {
        write_process_input(writer, input, cancelled);
    })
}

#[cfg(windows)]
fn spawn_writer(
    writer: ChildStdin,
    input: &[u8],
    setup_deadline: Instant,
) -> Result<ProcessTask<()>, LfsAuthError> {
    let input = SensitiveProcessBuffer(input.to_vec());
    spawn_io_task(
        writer,
        move |writer, cancelled| {
            write_process_input(writer, input, cancelled);
        },
        setup_deadline,
    )
}

#[cfg(not(any(unix, windows)))]
fn spawn_writer(
    _writer: ChildStdin,
    _input: &[u8],
    _setup_deadline: Instant,
) -> Result<ProcessTask<()>, LfsAuthError> {
    Err(LfsAuthError::ProcessFailed)
}

fn read_process_output<R: Read>(
    mut reader: R,
    limit: usize,
    cancelled: &AtomicBool,
    oversized: Arc<AtomicBool>,
    reader_failed: Arc<AtomicBool>,
) -> Result<SensitiveProcessBuffer, LfsAuthError> {
    let mut output = SensitiveProcessBuffer::new();
    let mut buffer = SensitiveStackBuffer::<8192>::zeroed();
    loop {
        if cancelled.load(Ordering::Acquire) {
            return Err(LfsAuthError::ProcessFailed);
        }
        match reader.read(&mut buffer.0) {
            Ok(0) => return Ok(output),
            Ok(read) => {
                if output.len().saturating_add(read) > limit {
                    oversized.store(true, Ordering::Release);
                    return Err(LfsAuthError::OutputTooLarge);
                }
                output.extend_from_slice(&buffer.0[..read]);
            }
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(PROCESS_POLL_INTERVAL);
            }
            Err(_) => {
                reader_failed.store(true, Ordering::Release);
                return Err(LfsAuthError::ProcessFailed);
            }
        }
    }
}

fn write_process_input<W: Write>(
    mut writer: W,
    mut input: SensitiveProcessBuffer,
    cancelled: &AtomicBool,
) {
    let mut written = 0_usize;
    while written < input.len() {
        if cancelled.load(Ordering::Acquire) {
            return;
        }
        match writer.write(&input.0[written..]) {
            Ok(0) => return,
            Ok(count) => written += count,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(PROCESS_POLL_INTERVAL);
            }
            Err(_) => return,
        }
    }
    wipe(&mut input.0);
}

fn cleanup_deadline(hard_deadline: Instant) -> Instant {
    earlier_deadline(
        hard_deadline,
        Instant::now()
            .checked_add(PROCESS_CLEANUP_GRACE)
            .unwrap_or(hard_deadline),
    )
}

fn earlier_deadline(left: Instant, right: Instant) -> Instant {
    if left <= right { left } else { right }
}

fn terminate_and_reap(containment: &ProcessContainment, child: &mut Child, hard_deadline: Instant) {
    containment.terminate_tree(child.id());
    let _ = child.kill();
    let _ = reap_direct_child_until(child, cleanup_deadline(hard_deadline));
}

fn reap_direct_child_until(child: &mut Child, deadline: Instant) -> Result<(), LfsAuthError> {
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return Ok(()),
            Ok(None) if Instant::now() < deadline => thread::sleep(PROCESS_POLL_INTERVAL),
            Ok(None) | Err(_) => return Err(LfsAuthError::ProcessFailed),
        }
    }
}

fn wipe(bytes: &mut [u8]) {
    for byte in bytes {
        *byte = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn assert_wall_time_bound(started: Instant, timeout: Duration, allowance: Duration) {
        let elapsed = started.elapsed();
        assert!(
            elapsed <= timeout + allowance,
            "bounded process exceeded wall-time budget: elapsed {elapsed:?}, timeout {timeout:?}, allowance {allowance:?}"
        );
    }

    #[test]
    fn auth_transfer_adapter_keeps_headers_typed_and_redacted() {
        let auth = LfsAuthHeaders::from_pairs(vec![(
            "Authorization".to_owned(),
            b"Basic secret".to_vec(),
        )])
        .expect("auth headers");
        let transfer = LfsAuthTransferPolicy::to_transfer_headers(&auth).expect("transfer");
        let debug = format!("{transfer:?}");
        assert!(!debug.contains("secret"));
    }

    #[test]
    fn credential_fill_parser_rejects_duplicate_secrets() {
        assert!(matches!(
            parse_credential_fill(b"username=a\npassword=b\npassword=c\n"),
            Err(LfsAuthError::DuplicateField)
        ));
    }

    #[test]
    fn credential_input_matches_git_lfs_http_path_scope() {
        let endpoint = super::super::parse_http_url(
            "https://example.test/team/repo/info/lfs?signature=secret",
        )
        .expect("endpoint");
        let origin =
            LfsAuthOrigin::from_endpoint_with_http_path(&endpoint, true).expect("scoped origin");
        assert_eq!(
            credential_input(&origin, None, None).expect("scoped input"),
            b"protocol=https\nhost=example.test\npath=team/repo/info/lfs\n\n"
        );

        let unscoped =
            LfsAuthOrigin::from_endpoint_with_http_path(&endpoint, false).expect("unscoped origin");
        assert_eq!(
            credential_input(&unscoped, None, None).expect("unscoped input"),
            b"protocol=https\nhost=example.test\n\n"
        );
    }

    #[test]
    fn credential_input_allows_an_empty_opt_in_path() {
        let endpoint = super::super::parse_http_url("https://example.test").expect("endpoint");
        let origin =
            LfsAuthOrigin::from_endpoint_with_http_path(&endpoint, true).expect("scoped origin");
        assert_eq!(
            credential_input(&origin, None, None).expect("scoped input"),
            b"protocol=https\nhost=example.test\npath=\n\n"
        );
    }

    #[test]
    fn credential_input_uses_decoded_byte_preserving_url_path() {
        let endpoint = super::super::parse_http_url(
            "https://example.test/team/%72epo%20one/%fF/info/lfs?signature=secret",
        )
        .expect("endpoint");
        let origin =
            LfsAuthOrigin::from_endpoint_with_http_path(&endpoint, true).expect("scoped origin");
        assert_eq!(
            credential_input(&origin, None, None).expect("scoped input"),
            b"protocol=https\nhost=example.test\npath=team/repo one/\xff/info/lfs\n\n"
        );
    }

    #[test]
    fn credential_receipt_is_opaque_bounded_and_operation_scoped() {
        let helper = GitCredentialHelper::system(Duration::from_secs(1)).expect("helper");
        let endpoint = super::super::parse_http_url("https://example.test/repo").expect("endpoint");
        let origin = LfsAuthOrigin::from_endpoint(&endpoint).expect("origin");
        let key = CredentialReceiptKey::new(&origin, LfsOperation::Fetch);
        helper.receipts.lock().expect("receipt lock").insert(
            key,
            CredentialPair {
                username: b"private-user".to_vec(),
                password: b"private-password".to_vec(),
            },
        );
        let debug = format!("{helper:?}");
        assert!(!debug.contains("private-user"));
        assert!(!debug.contains("private-password"));
        assert!(
            helper
                .take_receipt(&origin, LfsOperation::Push)
                .expect("push receipt")
                .is_none()
        );
        assert!(
            helper
                .take_receipt(&origin, LfsOperation::Fetch)
                .expect("fetch receipt")
                .is_some()
        );
    }

    #[test]
    fn basic_authorization_encodes_the_single_rfc_user_pass_buffer() {
        assert_eq!(
            basic_authorization_value(b"Aladdin", b"open sesame").expect("basic auth"),
            b"Basic QWxhZGRpbjpvcGVuIHNlc2FtZQ=="
        );
        assert_eq!(
            basic_authorization_value(b"", b"").expect("empty basic auth"),
            b"Basic Og=="
        );
        assert_eq!(
            basic_authorization_value(b"user", b"").expect("empty password"),
            b"Basic dXNlcjo="
        );
        assert_eq!(
            basic_authorization_value(b"", b"password").expect("empty username"),
            b"Basic OnBhc3N3b3Jk"
        );
    }

    #[test]
    fn basic_authorization_rejects_ambiguous_or_non_utf8_credentials() {
        for (username, password) in [
            (&b"user:name"[..], &b"password"[..]),
            (&b"user\n"[..], &b"password"[..]),
            ("user\u{0085}".as_bytes(), &b"password"[..]),
            (&[0xff][..], &b"password"[..]),
            (&b"user"[..], &[0xff][..]),
        ] {
            assert_eq!(
                basic_authorization_value(username, password),
                Err(LfsAuthError::InvalidField)
            );
        }
        assert_eq!(
            basic_authorization_value(
                &vec![b'u'; MAX_CREDENTIAL_FIELD_BYTES],
                &vec![b'p'; MAX_CREDENTIAL_FIELD_BYTES],
            ),
            Err(LfsAuthError::HeaderTooLarge)
        );
    }

    #[test]
    fn process_exit_observed_at_the_deadline_is_a_timeout() {
        let started = Instant::now();
        let deadline = started + Duration::from_millis(10);
        let before_deadline = deadline - Duration::from_nanos(1);

        let boundary = classify_process_poll(
            ProcessPollObservation::Exited(0),
            before_deadline,
            deadline,
            deadline,
        )
        .expect("boundary completion");
        assert_eq!(boundary.status, Err(LfsAuthError::Timeout));
        assert!(boundary.reaped, "try_wait already reaped the direct child");

        let in_time = classify_process_poll(
            ProcessPollObservation::Exited(0),
            before_deadline,
            before_deadline,
            deadline,
        )
        .expect("in-time completion");
        assert_eq!(in_time.status, Ok(0));
        assert!(in_time.reaped);
    }

    #[cfg(unix)]
    #[test]
    fn credential_runner_does_not_wait_for_inherited_stderr_daemon() {
        let started = Instant::now();
        let output = run_bounded_credential_process(
            Path::new("/bin/sh"),
            &[
                OsString::from("-c"),
                OsString::from("(sleep 0.2 >/dev/null 2>&1) & printf daemon-ok"),
            ],
            b"protocol=https\nhost=example.test\n\n",
            Duration::from_secs(1),
            MAX_CREDENTIAL_OUTPUT_BYTES,
        )
        .expect("credential helper");
        assert_eq!(output.status, 0);
        assert_eq!(output.stdout, b"daemon-ok");
        assert!(output.stderr.is_empty());
        assert!(
            started.elapsed() < Duration::from_millis(150),
            "credential helper waited for inherited stderr: {:?}",
            started.elapsed()
        );
    }

    #[cfg(unix)]
    #[test]
    fn bounded_process_streams_binary_input_and_output() {
        let arguments = [OsString::from("-c"), OsString::from("cat")];
        let output = run_bounded_process(
            Path::new("/bin/sh"),
            &arguments,
            b"binary\0secret",
            Duration::from_secs(2),
            1024,
            1024,
        )
        .expect("bounded cat");
        assert_eq!(output.status, 0);
        assert_eq!(output.stdout, b"binary\0secret");
        assert!(output.stderr.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn bounded_process_normal_exit_reaps_descendants_holding_pipes() {
        let arguments = [
            OsString::from("-c"),
            OsString::from("(sleep 30) & printf ready"),
        ];
        let started = Instant::now();
        let output = run_bounded_process(
            Path::new("/bin/sh"),
            &arguments,
            b"",
            Duration::from_secs(2),
            1024,
            1024,
        )
        .expect("bounded background descendant");
        assert_eq!(output.status, 0);
        assert_eq!(output.stdout, b"ready");
        assert!(started.elapsed() < Duration::from_secs(3));
    }

    #[cfg(unix)]
    #[test]
    fn bounded_process_timeout_kills_descendants_holding_pipes() {
        let arguments = [
            OsString::from("-c"),
            OsString::from("(sleep 30) & sleep 30"),
        ];
        let timeout = Duration::from_millis(100);
        let started = Instant::now();
        assert_eq!(
            run_bounded_process(
                Path::new("/bin/sh"),
                &arguments,
                b"secret input",
                timeout,
                1024,
                1024,
            )
            .map(|_| ()),
            Err(LfsAuthError::Timeout)
        );
        assert_wall_time_bound(started, timeout, Duration::from_millis(150));
    }

    #[cfg(unix)]
    #[test]
    fn bounded_process_oversize_kills_descendants_holding_pipes() {
        let arguments = [
            OsString::from("-c"),
            OsString::from("(sleep 30) & while :; do printf 0123456789abcdef; done"),
        ];
        let timeout = Duration::from_secs(2);
        let started = Instant::now();
        assert_eq!(
            run_bounded_process(
                Path::new("/bin/sh"),
                &arguments,
                b"secret input",
                timeout,
                1024,
                1024,
            )
            .map(|_| ()),
            Err(LfsAuthError::OutputTooLarge)
        );
        assert_wall_time_bound(started, timeout, Duration::from_millis(150));
    }

    #[cfg(unix)]
    #[test]
    fn reader_collection_cancels_an_open_pipe_at_its_deadline() {
        use std::os::unix::net::UnixStream;

        let (reader, _escaped_writer) = UnixStream::pair().expect("reader pipe pair");
        let task = spawn_reader(
            reader,
            1024,
            Arc::new(AtomicBool::new(false)),
            Arc::new(AtomicBool::new(false)),
            Instant::now() + Duration::from_secs(1),
        )
        .expect("spawn bounded reader");
        let started = Instant::now();
        let result = task
            .receive_until(
                Instant::now() + Duration::from_millis(20),
                Instant::now() + Duration::from_secs(1),
            )
            .and_then(std::convert::identity);
        assert_eq!(result.map(|_| ()), Err(LfsAuthError::ProcessFailed));
        assert!(started.elapsed() < Duration::from_secs(1));
    }

    #[cfg(unix)]
    #[test]
    fn writer_collection_cancels_a_full_pipe_at_its_deadline() {
        use std::os::unix::net::UnixStream;

        let (mut writer, _escaped_reader) = UnixStream::pair().expect("writer pipe pair");
        writer
            .set_nonblocking(true)
            .expect("set saturation writer nonblocking");
        let fill = [b'x'; 8192];
        loop {
            match writer.write(&fill) {
                Ok(_) => {}
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(error) => panic!("saturate writer pipe: {error}"),
            }
        }
        let input = SensitiveProcessBuffer(b"secret writer input".to_vec());
        let task = spawn_io_task(writer, move |writer, cancelled| {
            write_process_input(writer, input, cancelled);
        })
        .expect("spawn bounded writer");
        let started = Instant::now();
        task.receive_until(
            Instant::now() + Duration::from_millis(20),
            Instant::now() + Duration::from_secs(1),
        )
        .expect("cancel bounded writer");
        assert!(started.elapsed() < Duration::from_secs(1));
    }

    #[cfg(windows)]
    #[test]
    fn windows_job_timeout_kills_descendants_holding_pipes() {
        let arguments = [
            OsString::from("/C"),
            OsString::from(
                "start \"\" /B cmd /C \"ping 127.0.0.1 -n 30 >NUL\" & ping 127.0.0.1 -n 30 >NUL",
            ),
        ];
        let timeout = Duration::from_millis(150);
        let started = Instant::now();
        assert_eq!(
            run_bounded_process(
                Path::new("cmd"),
                &arguments,
                b"secret input",
                timeout,
                1024,
                1024,
            )
            .map(|_| ()),
            Err(LfsAuthError::Timeout)
        );
        // Wine and native Windows process creation add scheduler overhead
        // outside the child execution budget, but cleanup itself is bounded.
        assert_wall_time_bound(started, timeout, Duration::from_millis(500));
    }

    #[cfg(windows)]
    #[test]
    fn windows_job_oversize_kills_descendants_holding_pipes() {
        let arguments = [
            OsString::from("/C"),
            OsString::from(
                "start \"\" /B cmd /C \"ping 127.0.0.1 -n 30 >NUL\" & for /L %i in (1,0,2) do @echo 0123456789abcdef",
            ),
        ];
        let timeout = Duration::from_secs(2);
        let started = Instant::now();
        assert_eq!(
            run_bounded_process(
                Path::new("cmd"),
                &arguments,
                b"secret input",
                timeout,
                1024,
                1024,
            )
            .map(|_| ()),
            Err(LfsAuthError::OutputTooLarge)
        );
        assert_wall_time_bound(started, timeout, Duration::from_millis(500));
    }

    #[test]
    fn ssh_runner_rejects_remote_shell_metacharacters() {
        assert_eq!(
            validate_remote_argument("org/repo;touch"),
            Err(LfsAuthError::InvalidCommandPath)
        );
        assert!(validate_remote_argument("org/repo.git").is_ok());
    }

    #[test]
    fn helper_rejects_zero_timeout() {
        assert!(matches!(
            GitCredentialHelper::system(Duration::ZERO),
            Err(LfsAuthError::Timeout)
        ));
    }
}
