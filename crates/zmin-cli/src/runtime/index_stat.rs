use super::{CliError, ConfigEntry, ConfigScope, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct IndexStatOptions {
    trust_ctime: bool,
    check_stat: bool,
}

impl IndexStatOptions {
    pub(crate) fn from_config(entries: &[ConfigEntry]) -> Result<Self> {
        Ok(Self {
            trust_ctime: config_bool(entries, "trustctime")?.unwrap_or(true),
            check_stat: config_check_stat(entries)?.unwrap_or(true),
        })
    }

    pub(crate) const fn trust_ctime(self) -> bool {
        self.trust_ctime
    }

    pub(crate) const fn check_stat(self) -> bool {
        self.check_stat
    }
}

impl Default for IndexStatOptions {
    fn default() -> Self {
        Self {
            trust_ctime: true,
            check_stat: true,
        }
    }
}

fn config_bool(entries: &[ConfigEntry], key: &str) -> Result<Option<bool>> {
    let Some(entry) = core_entry(entries, key) else {
        return Ok(None);
    };
    entry.bool_value().map(Some).ok_or_else(|| CliError::Fatal {
        code: 128,
        message: format!(
            "bad boolean config value '{}' for 'core.{key}'",
            entry.value
        ),
    })
}

fn config_check_stat(entries: &[ConfigEntry]) -> Result<Option<bool>> {
    let Some(entry) = core_entry(entries, "checkstat") else {
        return Ok(None);
    };
    if entry.value.eq_ignore_ascii_case("default") {
        return Ok(Some(true));
    }
    if entry.value.eq_ignore_ascii_case("minimal") {
        return Ok(Some(false));
    }
    Err(invalid_check_stat_error(entry))
}

fn invalid_check_stat_error(entry: &ConfigEntry) -> CliError {
    let error = if entry.implicit_bool {
        "error: missing value for 'core.checkstat'\n".to_owned()
    } else {
        format!(
            "error: invalid value for 'core.checkstat': '{}'\n",
            entry.value
        )
    };
    if entry.scope == ConfigScope::Command {
        return CliError::Stderr {
            code: 128,
            text: format!(
                "{error}fatal: unable to parse 'core.checkstat' from command-line config\n"
            ),
        };
    }
    if let Some(line) = entry.line {
        let origin = entry.origin.strip_prefix("file:").unwrap_or(&entry.origin);
        return CliError::Stderr {
            code: 128,
            text: format!(
                "{error}fatal: bad config variable 'core.checkstat' in file '{origin}' at line {line}\n"
            ),
        };
    }
    CliError::Stderr {
        code: 128,
        text: error,
    }
}

fn core_entry<'a>(entries: &'a [ConfigEntry], key: &str) -> Option<&'a ConfigEntry> {
    entries
        .iter()
        .rev()
        .find(|entry| entry.section == "core" && entry.subsection.is_empty() && entry.key == key)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::{ConfigScope, parse_global_config_entry};

    fn entry(value: &str) -> ConfigEntry {
        let mut entry = parse_global_config_entry(value).expect("config entry");
        entry.scope = ConfigScope::Command;
        entry
    }

    #[test]
    fn index_stat_options_parse_git_values() {
        let options = IndexStatOptions::from_config(&[
            entry("core.trustctime=false"),
            entry("core.checkstat=minimal"),
        ])
        .expect("options");
        assert!(!options.trust_ctime());
        assert!(!options.check_stat());

        let options =
            IndexStatOptions::from_config(&[entry("core.checkstat=default")]).expect("options");
        assert!(options.trust_ctime());
        assert!(options.check_stat());
    }

    #[test]
    fn index_stat_options_reject_invalid_checkstat() {
        let error = IndexStatOptions::from_config(&[entry("core.checkstat=fast")])
            .expect_err("invalid checkstat");
        assert!(format!("{error:?}").contains(
            "error: invalid value for 'core.checkstat': 'fast'\\nfatal: unable to parse 'core.checkstat' from command-line config"
        ));
    }
}
