//! Minimal localization support for CLI and WASM front-ends.
//!
//! The module exposes a tiny `Locale` enum and a `Messages` helper that returns human-friendly
//! strings for interactive flows. Add new message helpers as more commands are localized.

use std::borrow::Cow;

mod locales;

use locales::de_de::STRINGS as DE_DE;
use locales::en_us::STRINGS as EN_US;
use locales::es_es::STRINGS as ES_ES;
use locales::pl_pl::STRINGS as PL_PL;
use locales::uk_ua::STRINGS as UK_UA;

fn catalog(locale: Locale) -> &'static [(&'static str, &'static str)] {
    match locale {
        Locale::EnUs => EN_US,
        Locale::UkUa => UK_UA,
        Locale::PlPl => PL_PL,
        Locale::EsEs => ES_ES,
        Locale::DeDe => DE_DE,
    }
}

fn lookup(locale: Locale, key: &str) -> Option<&'static str> {
    catalog(locale)
        .iter()
        .find_map(|(k, value)| if *k == key { Some(*value) } else { None })
}

fn replace_params(template: &str, params: &[(&str, &str)]) -> String {
    let mut rendered = template.to_owned();
    for (key, value) in params {
        let needle = format!("{{{}}}", key);
        rendered = rendered.replace(&needle, value);
    }
    rendered
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Locale {
    EnUs,
    UkUa,
    PlPl,
    EsEs,
    DeDe,
}

impl Locale {
    pub fn fallback() -> Self {
        Self::EnUs
    }

    pub fn from_tag(tag: &str) -> Option<Self> {
        let normalized = tag.trim().to_ascii_lowercase().replace('_', "-");
        let primary = normalized.split('-').next().unwrap_or(&normalized);
        match primary {
            "en" => Some(Self::EnUs),
            "uk" => Some(Self::UkUa),
            "pl" => Some(Self::PlPl),
            "es" => Some(Self::EsEs),
            "de" => Some(Self::DeDe),
            _ => None,
        }
    }

    pub fn tag(&self) -> &'static str {
        match self {
            Self::EnUs => "en-US",
            Self::UkUa => "uk-UA",
            Self::PlPl => "pl-PL",
            Self::EsEs => "es-ES",
            Self::DeDe => "de-DE",
        }
    }
}

pub fn detect_locale() -> Locale {
    if let Ok(explicit) = std::env::var("ZMIN_LANG")
        && let Some(locale) = Locale::from_tag(&explicit)
    {
        return locale;
    }

    for var in ["LC_ALL", "LANG", "LC_MESSAGES"] {
        if let Ok(value) = std::env::var(var)
            && let Some(locale) = Locale::from_tag(&value)
        {
            return locale;
        }
    }

    Locale::fallback()
}

#[derive(Clone, Debug)]
pub struct Messages {
    locale: Locale,
}

impl Messages {
    pub fn new(locale: Locale) -> Self {
        Self { locale }
    }

    pub fn locale(&self) -> Locale {
        self.locale
    }

    pub fn t(&self, key: &'static str) -> &'static str {
        lookup(self.locale, key)
            .or_else(|| lookup(Locale::fallback(), key))
            .unwrap_or(key)
    }

    pub fn t_with(&self, key: &'static str, params: &[(&str, &str)]) -> String {
        let template = self.t(key);
        if params.is_empty() {
            template.to_owned()
        } else {
            replace_params(template, params)
        }
    }

    pub fn login_connecting(&self, url: &str) -> String {
        self.t_with("login_connecting", &[("url", url)])
    }

    pub fn login_verification_title(&self) -> &'static str {
        self.t("login_verification_title")
    }

    pub fn login_verification_code(&self, code: &str) -> String {
        self.t_with("login_verification_code", &[("code", code)])
    }

    pub fn login_verification_instruction(&self) -> &'static str {
        self.t("login_verification_instruction")
    }

    pub fn login_press_enter(&self) -> String {
        self.t("login_press_enter").to_owned()
    }

    pub fn login_opening_browser(&self, url: &str) -> String {
        self.t_with("login_opening_browser", &[("url", url)])
    }

    pub fn login_browser_launch_failed(&self, err: &str) -> String {
        self.t_with("login_browser_launch_failed", &[("err", err)])
    }

    pub fn login_open_link_manually(&self, url: &str) -> String {
        self.t_with("login_open_link_manually", &[("url", url)])
    }

    pub fn login_success(&self, email: &str, server: &str) -> String {
        self.t_with("login_success", &[("email", email), ("server", server)])
    }

    pub fn unlock_success(&self, email: &str) -> String {
        self.t_with("unlock_success", &[("email", email)])
    }

    pub fn lock_success(&self) -> &'static str {
        self.t("lock_success")
    }

    pub fn logout_success(&self) -> &'static str {
        self.t("logout_success")
    }

    pub fn clone_session_saved(&self, email: &str, org_id: &str, server: &str) -> String {
        self.t_with(
            "clone_session_saved",
            &[("email", email), ("org_id", org_id), ("server", server)],
        )
    }

    pub fn clone_multiple_accounts(&self, server: &str) -> String {
        self.t_with("clone_multiple_accounts", &[("server", server)])
    }

    pub fn clone_use_saved_account(&self) -> &'static str {
        self.t("clone_use_saved_account")
    }

    pub fn clone_add_account_prompt(&self, server: &str) -> String {
        self.t_with("clone_add_account_prompt", &[("server", server)])
    }

    pub fn clone_no_account(&self, server: &str) -> String {
        self.t_with("clone_no_account", &[("server", server)])
    }

    pub fn clone_login_now(&self, server: &str) -> String {
        self.t_with("clone_login_now", &[("server", server)])
    }

    pub fn clone_login_abort(&self, server: &str) -> String {
        self.t_with("clone_login_abort", &[("server", server)])
    }

    pub fn clone_session_mismatch(
        &self,
        email: &str,
        session_org: &str,
        repo_id: &str,
        remote_org: &str,
    ) -> String {
        self.t_with(
            "clone_session_mismatch",
            &[
                ("email", email),
                ("session_org", session_org),
                ("repo_id", repo_id),
                ("remote_org", remote_org),
            ],
        )
    }

    pub fn clone_replace_account_prompt(&self, server: &str) -> String {
        self.t_with("clone_replace_account_prompt", &[("server", server)])
    }

    pub fn clone_mismatch_abort(&self, server: &str) -> String {
        self.t_with("clone_mismatch_abort", &[("server", server)])
    }

    pub fn stage_path_staged(&self, path: &str) -> String {
        self.t_with("stage_path_staged", &[("path", path)])
    }

    pub fn stage_reject_git_dir(&self, path: &str) -> String {
        self.t_with("stage_reject_git_dir", &[("path", path)])
    }

    pub fn unlock_repo_banner(
        &self,
        email: &str,
        org_id: &str,
        repo_id: &str,
        remote: &str,
    ) -> String {
        self.t_with(
            "unlock_repo_banner",
            &[
                ("email", email),
                ("org_id", org_id),
                ("repo_id", repo_id),
                ("remote", remote),
            ],
        )
    }

    pub fn merge_fast_forward(&self, branch: &str, commit: &str) -> String {
        self.t_with(
            "merge_fast_forward",
            &[("branch", branch), ("commit", commit)],
        )
    }

    pub fn merge_up_to_date(&self) -> &'static str {
        self.t("merge_up_to_date")
    }

    pub fn merge_commit_created(&self, commit: &str) -> String {
        self.t_with("merge_commit_created", &[("commit", commit)])
    }

    pub fn tag_created(&self, tag: &str, commit: &str, id: &str) -> String {
        self.t_with(
            "tag_created",
            &[("tag", tag), ("commit", commit), ("id", id)],
        )
    }

    pub fn reflog_heading(&self) -> &'static str {
        self.t("reflog_heading")
    }

    pub fn reflog_empty(&self) -> &'static str {
        self.t("reflog_empty")
    }

    pub fn reflog_none(&self) -> &'static str {
        self.t("reflog_none")
    }

    pub fn reflog_entry(
        &self,
        reference: &str,
        old: &str,
        new: &str,
        message: &str,
        timestamp: &str,
    ) -> String {
        self.t_with(
            "reflog_entry",
            &[
                ("reference", reference),
                ("old", old),
                ("new", new),
                ("message", message),
                ("timestamp", timestamp),
            ],
        )
    }

    pub fn auth_no_identity(&self, email: &str) -> String {
        self.t_with("auth_no_identity", &[("email", email)])
    }

    pub fn auth_seed_phrase_banner<'a>(&self, phrase: &'a str) -> Cow<'a, str> {
        Cow::Owned(self.t_with("auth_seed_phrase_banner", &[("phrase", phrase)]))
    }

    pub fn auth_seed_phrase_tip(&self) -> &'static str {
        self.t("auth_seed_phrase_tip")
    }

    pub fn auth_seed_fingerprint(&self, fingerprint: &str) -> String {
        self.t_with("auth_seed_fingerprint", &[("fingerprint", fingerprint)])
    }

    pub fn auth_identity_ready(&self) -> &'static str {
        self.t("auth_identity_ready")
    }

    pub fn auth_unlock_failed(&self, err: &str) -> String {
        self.t_with("auth_unlock_failed", &[("err", err)])
    }
    pub fn auth_password_attempt_failed(&self, remaining: usize) -> String {
        let remaining_str = remaining.to_string();
        self.t_with(
            "auth_password_attempt_failed",
            &[("remaining", remaining_str.as_str())],
        )
    }

    pub fn auth_password_attempts_exhausted(&self, attempts: usize) -> String {
        let attempts_str = attempts.to_string();
        self.t_with(
            "auth_password_attempts_exhausted",
            &[("attempts", attempts_str.as_str())],
        )
    }

    pub fn auth_prompt_recover_now(&self, email: &str, server: &str) -> String {
        self.t_with(
            "auth_prompt_recover_now",
            &[("email", email), ("server", server)],
        )
    }

    pub fn auth_recover_intro(&self, email: &str, server: &str) -> String {
        self.t_with(
            "auth_recover_intro",
            &[("email", email), ("server", server)],
        )
    }

    pub fn auth_recover_identity_missing(&self, email: &str, server: &str) -> String {
        self.t_with(
            "auth_recover_identity_missing",
            &[("email", email), ("server", server)],
        )
    }

    pub fn repo_stage_on_lock_prompt(&self) -> &'static str {
        self.t("repo_stage_on_lock_prompt")
    }

    pub fn publish_no_changes(&self) -> &'static str {
        self.t("publish_no_changes")
    }

    pub fn publish_intro(&self) -> &'static str {
        self.t("publish_intro")
    }

    pub fn publish_confirm_stage(&self) -> &'static str {
        self.t("publish_confirm_stage")
    }

    pub fn publish_stage_skip_hint(&self) -> &'static str {
        self.t("publish_stage_skip_hint")
    }

    pub fn publish_everything_staged(&self) -> &'static str {
        self.t("publish_everything_staged")
    }

    pub fn publish_branch_current(&self, branch: &str) -> String {
        self.t_with("publish_branch_current", &[("branch", branch)])
    }

    pub fn publish_branch_use_current(&self, branch: &str) -> String {
        self.t_with("publish_branch_use_current", &[("branch", branch)])
    }

    pub fn publish_branch_new_prompt(&self) -> &'static str {
        self.t("publish_branch_new_prompt")
    }

    pub fn publish_branch_switched(&self, branch: &str) -> String {
        self.t_with("publish_branch_switched", &[("branch", branch)])
    }

    pub fn publish_push_now(&self) -> &'static str {
        self.t("publish_push_now")
    }

    pub fn publish_prompt_message(&self) -> &'static str {
        self.t("publish_prompt_message")
    }

    pub fn publish_commit_done(&self, id: &str) -> String {
        self.t_with("publish_commit_done", &[("id", id)])
    }

    pub fn publish_pushed(&self, remote: &str) -> String {
        self.t_with("publish_pushed", &[("remote", remote)])
    }

    pub fn publish_push_skipped(&self) -> &'static str {
        self.t("publish_push_skipped")
    }

    pub fn publish_review_prompt(&self) -> &'static str {
        self.t("publish_review_prompt")
    }

    pub fn publish_review_manual_hint(&self) -> &'static str {
        self.t("publish_review_manual_hint")
    }

    pub fn changes_summary_heading(&self) -> &'static str {
        self.t("changes_summary_heading")
    }

    pub fn changes_ready_heading(&self) -> &'static str {
        self.t("changes_ready_heading")
    }

    pub fn changes_pending_heading(&self) -> &'static str {
        self.t("changes_pending_heading")
    }

    pub fn changes_modified_label(&self) -> &'static str {
        self.t("changes_modified_label")
    }

    pub fn changes_new_label(&self) -> &'static str {
        self.t("changes_new_label")
    }

    pub fn changes_deleted_label(&self) -> &'static str {
        self.t("changes_deleted_label")
    }

    pub fn preview_intro(&self) -> &'static str {
        self.t("preview_intro")
    }

    pub fn preview_followup_hint(&self) -> &'static str {
        self.t("preview_followup_hint")
    }

    pub fn sync_checking(&self) -> &'static str {
        self.t("sync_checking")
    }

    pub fn sync_up_to_date(&self) -> &'static str {
        self.t("sync_up_to_date")
    }

    pub fn sync_new_commits_heading(&self, branch: &str) -> String {
        self.t_with("sync_new_commits_heading", &[("branch", branch)])
    }

    pub fn sync_new_commit_entry(&self, id: &str, message: &str) -> String {
        self.t_with("sync_new_commit_entry", &[("id", id), ("message", message)])
    }

    pub fn sync_branch_updates_heading(&self) -> &'static str {
        self.t("sync_branch_updates_heading")
    }

    pub fn sync_branch_new_entry(&self, branch: &str, id: &str, message: &str) -> String {
        self.t_with(
            "sync_branch_new_entry",
            &[("branch", branch), ("id", id), ("message", message)],
        )
    }

    pub fn sync_branch_update_entry(&self, branch: &str, id: &str, message: &str) -> String {
        self.t_with(
            "sync_branch_update_entry",
            &[("branch", branch), ("id", id), ("message", message)],
        )
    }

    pub fn sync_commit_message_fallback(&self) -> &'static str {
        self.t("sync_commit_message_fallback")
    }

    pub fn sync_branch_removed_heading(&self) -> &'static str {
        self.t("sync_branch_removed_heading")
    }

    pub fn sync_branch_removed_entry(&self, branch: &str) -> String {
        self.t_with("sync_branch_removed_entry", &[("branch", branch)])
    }

    pub fn auth_prompt_seed_phrase(&self) -> &'static str {
        self.t("auth_prompt_seed_phrase")
    }

    pub fn auth_seed_recovery_start(&self) -> &'static str {
        self.t("auth_seed_recovery_start")
    }

    pub fn auth_identity_recovered(&self) -> &'static str {
        self.t("auth_identity_recovered")
    }

    pub fn accounts_header(&self, base: &str) -> String {
        self.t_with("accounts_header", &[("base", base)])
    }

    pub fn accounts_entry(&self, index: usize, email: &str, org_id: &str) -> String {
        let index_str = index.to_string();
        self.t_with(
            "accounts_entry",
            &[
                ("index", index_str.as_str()),
                ("email", email),
                ("org_id", org_id),
            ],
        )
    }

    pub fn prompt_select_account(&self) -> &'static str {
        self.t("prompt_select_account")
    }

    pub fn warn_selection_out_of_range(&self) -> &'static str {
        self.t("warn_selection_out_of_range")
    }

    pub fn prompt_master_password(&self) -> &'static str {
        self.t("prompt_master_password")
    }

    pub fn prompt_new_master_password(&self) -> &'static str {
        self.t("prompt_new_master_password")
    }

    pub fn prompt_confirm_master_password(&self) -> &'static str {
        self.t("prompt_confirm_master_password")
    }

    pub fn warn_password_too_short(&self) -> &'static str {
        self.t("warn_password_too_short")
    }

    pub fn warn_passwords_mismatch(&self) -> &'static str {
        self.t("warn_passwords_mismatch")
    }

    pub fn password_guidance(&self) -> &'static str {
        self.t("password_guidance")
    }

    pub fn password_utf8_error(&self) -> &'static str {
        self.t("password_utf8_error")
    }

    pub fn auth_biometric_enabled(&self) -> &'static str {
        self.t("auth_biometric_enabled")
    }

    pub fn auth_biometric_disabled(&self) -> &'static str {
        self.t("auth_biometric_disabled")
    }

    pub fn auth_biometric_unavailable(&self) -> &'static str {
        self.t("auth_biometric_unavailable")
    }

    pub fn prompt_enable_biometric(&self) -> &'static str {
        self.t("prompt_enable_biometric")
    }

    pub fn prompt_keep_biometric_enabled(&self) -> &'static str {
        self.t("prompt_keep_biometric_enabled")
    }

    pub fn warn_invalid_choice(&self) -> &'static str {
        self.t("warn_invalid_choice")
    }

    pub fn auth_seed_confirm_prompt(&self) -> &'static str {
        self.t("auth_seed_confirm_prompt")
    }

    pub fn auth_seed_confirm_mismatch(&self) -> &'static str {
        self.t("auth_seed_confirm_mismatch")
    }

    pub fn prompt_seed_phrase(&self) -> &'static str {
        self.t("prompt_seed_phrase")
    }

    pub fn error_read_input(&self) -> &'static str {
        self.t("error_read_input")
    }

    pub fn error_input_required(&self) -> &'static str {
        self.t("error_input_required")
    }
}
