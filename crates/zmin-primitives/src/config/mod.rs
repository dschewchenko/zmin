use std::env;
use std::path::{Path, PathBuf};

use crate::error::{Error, Result};
use crate::i18n::{Locale, detect_locale};

#[derive(Clone, Debug)]
pub struct AppConfig {
    pub runtime: RuntimeProfile,
    pub data_dir: PathBuf,
    pub cache_dir: PathBuf,
    pub default_server: String,
    pub locale: Locale,
}

impl AppConfig {
    pub fn new(runtime: RuntimeProfile, data_dir: PathBuf, cache_dir: PathBuf) -> Result<Self> {
        let enforce_absolute = runtime.needs_absolute_paths();
        if enforce_absolute && !data_dir.is_absolute() {
            return Err(Error::Config {
                details: format!("data directory must be absolute: {}", data_dir.display()),
            });
        }
        if enforce_absolute && !cache_dir.is_absolute() {
            return Err(Error::Config {
                details: format!("cache directory must be absolute: {}", cache_dir.display()),
            });
        }
        Ok(Self {
            runtime,
            data_dir,
            cache_dir,
            default_server: resolve_default_server_url(),
            locale: detect_locale(),
        })
    }

    pub fn with_runtime(runtime: RuntimeProfile) -> Result<Self> {
        let data_dir = runtime.default_data_dir()?;
        let cache_dir = runtime.default_cache_dir(&data_dir)?;
        Self::new(runtime, data_dir, cache_dir)
    }

    pub fn server_base(&self) -> &str {
        &self.default_server
    }

    pub fn locale(&self) -> Locale {
        self.locale
    }
}

fn resolve_default_server_url() -> String {
    if let Ok(url) = env::var("ZMIN_SERVER") {
        let trimmed = url.trim();
        if !trimmed.is_empty() {
            return trimmed.to_owned();
        }
    }

    if let Some(url) = option_env!("ZMIN_SERVER_DEFAULT") {
        let trimmed = url.trim();
        if !trimmed.is_empty() {
            return trimmed.to_owned();
        }
    }

    "https://zmin.dev/api".to_owned()
}

#[derive(Clone, Debug)]
pub struct RuntimeProfile {
    pub kind: RuntimeKind,
    pub environment: RuntimeEnv,
}

impl RuntimeProfile {
    pub fn default_data_dir(&self) -> Result<PathBuf> {
        match &self.kind {
            RuntimeKind::Cli(_) => Ok(default_cli_root()?.join("data")),
            RuntimeKind::Wasm(_) => Ok(PathBuf::from("/zmin/data")),
        }
    }

    pub fn default_cache_dir(&self, _data_dir: &Path) -> Result<PathBuf> {
        match &self.kind {
            RuntimeKind::Cli(cli) => Ok(default_cli_root()?
                .join("cache")
                .join(cli.environment_scope.clone())),
            RuntimeKind::Wasm(_) => Ok(PathBuf::from("/zmin/data/cache")),
        }
    }

    fn needs_absolute_paths(&self) -> bool {
        matches!(self.kind, RuntimeKind::Cli(_))
    }
}

#[derive(Clone, Debug)]
pub enum RuntimeKind {
    Cli(CliProfile),
    Wasm(WasmProfile),
}

#[derive(Clone, Debug)]
pub enum RuntimeEnv {
    Development,
    Staging,
    Production,
}

impl RuntimeEnv {
    pub fn from_env() -> Self {
        match std::env::var("ZMIN_ENV").as_deref() {
            Ok("production") => Self::Production,
            Ok("staging") => Self::Staging,
            _ => Self::Development,
        }
    }
}

#[derive(Clone, Debug)]
pub struct CliProfile {
    pub workspace_root: PathBuf,
    pub environment_scope: String,
}

impl CliProfile {
    pub fn detect() -> Result<Self> {
        let workspace_root = std::env::current_dir()
            .map_err(|error| Error::from_display(error, crate::error::ErrorKind::Config))?;
        let environment_scope =
            std::env::var("ZMIN_PROFILE").unwrap_or_else(|_| "default".to_owned());
        Ok(Self {
            workspace_root,
            environment_scope,
        })
    }
}

#[derive(Clone, Debug)]
pub struct WasmProfile {
    pub origin: String,
}

impl WasmProfile {
    pub fn new(origin: impl Into<String>) -> Self {
        Self {
            origin: origin.into(),
        }
    }
}

#[cfg(feature = "cli")]
pub fn load_cli_config() -> Result<AppConfig> {
    let env = RuntimeEnv::from_env();
    let cli_profile = CliProfile::detect()?;
    let runtime = RuntimeProfile {
        kind: RuntimeKind::Cli(cli_profile),
        environment: env,
    };
    AppConfig::with_runtime(runtime)
}

#[cfg(all(feature = "wasm", target_arch = "wasm32"))]
pub fn wasm_config_with_origin(origin: impl Into<String>) -> Result<AppConfig> {
    let env = RuntimeEnv::from_env();
    let runtime = RuntimeProfile {
        kind: RuntimeKind::Wasm(WasmProfile::new(origin)),
        environment: env,
    };
    AppConfig::with_runtime(runtime)
}

fn default_cli_root() -> Result<PathBuf> {
    if let Ok(custom) = env::var("ZMIN_DATA_DIR") {
        let path = PathBuf::from(custom);
        if path.is_absolute() {
            return Ok(path);
        }
    }
    if let Some(dir) = dirs::data_dir() {
        return Ok(dir.join("zmin"));
    }
    if let Some(home) = dirs::home_dir() {
        return Ok(home.join(".zmin"));
    }
    Err(Error::Config {
        details: "unable to determine data directory; set ZMIN_DATA_DIR".into(),
    })
}
