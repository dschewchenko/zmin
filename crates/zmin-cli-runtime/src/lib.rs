use std::io;
use std::path::PathBuf;
use std::sync::OnceLock;

mod fs_ops;
mod phase_trace;
mod temp_files;

pub use fs_ops::{remove_file_if_exists, remove_path_if_exists};
pub use phase_trace::{PhaseTrace, phase_trace, phase_trace_emit, phase_trace_enabled};
pub use temp_files::{unique_temp_sibling, write_content_addressed_file};

pub type Result<T> = std::result::Result<T, CliError>;

#[derive(Debug)]
pub enum CliError {
    Exit(i32),
    Fatal { code: i32, message: String },
    Stderr { code: i32, text: String },
    Message(String),
    Io(io::Error),
}

impl From<io::Error> for CliError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

#[derive(Debug, Clone)]
pub struct GitRepo {
    pub root: PathBuf,
    pub git_dir: PathBuf,
    pub objects_dir: PathBuf,
    pub index_path: PathBuf,
}

pub struct CloneOptions {
    pub quiet: bool,
    pub configs: Vec<String>,
    pub template: Option<PathBuf>,
    pub reject_shallow: bool,
    pub recurse_submodules: Vec<String>,
    pub remote_submodules: bool,
    pub shallow_submodules: bool,
    pub bare: bool,
    pub mirror: bool,
    pub no_checkout: bool,
    pub worktree_first: bool,
    pub background_fetch: bool,
    pub demand_hydrate: bool,
    pub remote_name: String,
    pub no_tags: bool,
    pub single_branch: bool,
    pub no_single_branch: bool,
    pub separate_git_dir: Option<PathBuf>,
    pub references: Vec<PathBuf>,
    pub reference_if_able: Vec<PathBuf>,
    pub shared: bool,
    pub dissociate: bool,
    pub no_hardlinks: bool,
    pub no_local: bool,
    pub depth: Option<String>,
    pub branch: Option<String>,
    pub keep_partial_on_missing_branch: bool,
    pub repository: String,
    pub directory: Option<PathBuf>,
}

type CloneService = fn(CloneOptions) -> Result<()>;
type UploadPackRequestService = fn(&GitRepo, &mut dyn io::Read, bool) -> Result<Vec<u8>>;
type ReceivePackRequestService = fn(&GitRepo, &mut dyn io::Read) -> Result<Vec<u8>>;

static CLONE_SERVICE: OnceLock<CloneService> = OnceLock::new();
static UPLOAD_PACK_REQUEST_SERVICE: OnceLock<UploadPackRequestService> = OnceLock::new();
static RECEIVE_PACK_REQUEST_SERVICE: OnceLock<ReceivePackRequestService> = OnceLock::new();

pub fn register_clone_service(service: CloneService) {
    let _ = CLONE_SERVICE.set(service);
}

pub fn register_upload_pack_request_service(service: UploadPackRequestService) {
    let _ = UPLOAD_PACK_REQUEST_SERVICE.set(service);
}

pub fn register_receive_pack_request_service(service: ReceivePackRequestService) {
    let _ = RECEIVE_PACK_REQUEST_SERVICE.set(service);
}

pub fn run_clone_service(options: CloneOptions) -> Result<()> {
    let Some(service) = CLONE_SERVICE.get() else {
        return Err(CliError::Fatal {
            code: 128,
            message: "internal clone service is not registered".to_owned(),
        });
    };
    service(options)
}

pub fn run_upload_pack_request_service(
    repo: &GitRepo,
    input: &mut dyn io::Read,
    stateless_rpc: bool,
) -> Result<Vec<u8>> {
    let Some(service) = UPLOAD_PACK_REQUEST_SERVICE.get() else {
        return Err(CliError::Fatal {
            code: 128,
            message: "internal upload-pack service is not registered".to_owned(),
        });
    };
    service(repo, input, stateless_rpc)
}

pub fn run_receive_pack_request_service(
    repo: &GitRepo,
    input: &mut dyn io::Read,
) -> Result<Vec<u8>> {
    let Some(service) = RECEIVE_PACK_REQUEST_SERVICE.get() else {
        return Err(CliError::Fatal {
            code: 128,
            message: "internal receive-pack service is not registered".to_owned(),
        });
    };
    service(repo, input)
}
