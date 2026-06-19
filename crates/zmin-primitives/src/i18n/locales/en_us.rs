pub const STRINGS: &[(&str, &str)] = &[
    (
        "login_connecting",
        "Connecting to Zmin authentication service at {url}...",
    ),
    (
        "login_verification_title",
        "Verification code (enter this in your browser):",
    ),
    ("login_verification_code", "  {code}"),
    (
        "login_verification_instruction",
        "You will be asked to enter this code before signing in.",
    ),
    (
        "login_press_enter",
        "Press ENTER to open the verification page.",
    ),
    ("login_opening_browser", "Opening verification page: {url}"),
    (
        "login_browser_launch_failed",
        "Could not launch the browser automatically ({err}).",
    ),
    (
        "login_open_link_manually",
        "Open this link manually if needed: {url}",
    ),
    ("login_success", "Signed in as {email} ({server})."),
    ("unlock_success", "Workspace unlocked for {email}."),
    ("lock_success", "Workspace locked."),
    ("logout_success", "Signed out."),
    (
        "clone_session_saved",
        "Saved account {email} (org {org_id}) is available for {server}.",
    ),
    (
        "clone_multiple_accounts",
        "Multiple saved accounts found for {server}. Choose one or sign in with a different account:",
    ),
    ("clone_use_saved_account", "Use the saved account?"),
    (
        "clone_add_account_prompt",
        "Sign in with a different account for {server}? This replaces the saved session.",
    ),
    ("clone_no_account", "No saved account found for {server}."),
    ("clone_login_now", "Sign in now to continue?"),
    (
        "clone_login_abort",
        "Clone cancelled because no account was selected for {server}.",
    ),
    (
        "clone_session_mismatch",
        "Account {email} belongs to org {session_org}, but repository {repo_id} requires org {remote_org}.",
    ),
    (
        "clone_replace_account_prompt",
        "Sign in with another account for {server}?",
    ),
    (
        "clone_mismatch_abort",
        "Clone cancelled because the saved account does not have access to {server}.",
    ),
    (
        "auth_no_identity",
        "Creating a secure identity for {email}...",
    ),
    (
        "auth_seed_phrase_banner",
        "\n=== Recovery phrase — store securely ===\n{phrase}\n===\n",
    ),
    (
        "auth_seed_phrase_tip",
        "Write this phrase down and store it offline. You will need it to recover your account.",
    ),
    (
        "auth_seed_fingerprint",
        "Seed fingerprint (verify during recovery): {fingerprint}",
    ),
    (
        "auth_identity_ready",
        "Identity created and workspace unlocked.",
    ),
    (
        "auth_unlock_failed",
        "We could not unlock the workspace with that password ({err}). Starting recovery with your seed phrase...",
    ),
    (
        "auth_password_attempt_failed",
        "Incorrect master password. {remaining} attempt(s) remaining.",
    ),
    (
        "auth_password_attempts_exhausted",
        "Maximum of {attempts} attempts reached.",
    ),
    (
        "auth_prompt_recover_now",
        "Recover {email} on {server} now using the seed phrase?",
    ),
    (
        "auth_recover_intro",
        "Starting recovery for {email} on {server}.",
    ),
    (
        "auth_recover_identity_missing",
        "No local identity found for {email} on {server}; run `zmin login`.",
    ),
    (
        "publish_no_changes",
        "You are all caught up; nothing to publish.",
    ),
    ("publish_prompt_message", "Headline for this update:"),
    ("publish_commit_done", "Saved update {id}."),
    ("publish_pushed", "Published updates to {remote}."),
    (
        "publish_push_skipped",
        "Keeping updates on this device (remote publish skipped).",
    ),
    ("auth_prompt_seed_phrase", "Recovery phrase"),
    (
        "auth_seed_recovery_start",
        "Enter your recovery phrase to regain access.",
    ),
    (
        "auth_identity_recovered",
        "Identity recovered. Master password updated.",
    ),
    (
        "accounts_header",
        "Multiple saved sessions for {base}. Select one:",
    ),
    ("accounts_entry", "  [{index}] {email} (org {org_id})"),
    ("prompt_select_account", "Select session"),
    (
        "warn_selection_out_of_range",
        "Selection is out of range. Try again.",
    ),
    ("prompt_master_password", "Master password:"),
    (
        "prompt_new_master_password",
        "New master password (min 12 characters):",
    ),
    ("prompt_confirm_master_password", "Confirm master password:"),
    (
        "warn_password_too_short",
        "Password must be at least 12 characters.",
    ),
    (
        "warn_passwords_mismatch",
        "Passwords do not match. Try again.",
    ),
    (
        "password_guidance",
        "Tip: pick at least 12 characters mixing letters, numbers, and symbols. Only UTF-8 characters are accepted.",
    ),
    (
        "password_utf8_error",
        "Password contains unsupported characters. Please use UTF-8 text.",
    ),
    (
        "auth_biometric_enabled",
        "Biometric unlock enabled. Manage access via your OS keychain settings.",
    ),
    (
        "auth_biometric_disabled",
        "Biometric unlock disabled for this account.",
    ),
    (
        "auth_biometric_unavailable",
        "Biometric unlock is unavailable. Falling back to the master password.",
    ),
    (
        "prompt_enable_biometric",
        "Enable biometric unlock on this device?",
    ),
    (
        "prompt_keep_biometric_enabled",
        "Keep biometric unlock enabled for future logins?",
    ),
    (
        "repo_stage_on_lock_prompt",
        "When locking, encrypt staged changes and remove plaintext workspace files on this device?",
    ),
    ("warn_invalid_choice", "Please answer with 'y' or 'n'."),
    (
        "auth_seed_confirm_prompt",
        "Re-type the recovery phrase to confirm you saved it:",
    ),
    (
        "auth_seed_confirm_mismatch",
        "The phrase does not match. Please try again.",
    ),
    ("prompt_seed_phrase", "Seed phrase"),
    ("error_read_input", "Failed to read input. Try again."),
    ("error_input_required", "Input required."),
    ("publish_intro", "Let's get your update ready."),
    (
        "publish_confirm_stage",
        "Stage everything listed above now?",
    ),
    (
        "publish_stage_skip_hint",
        "Okay, nothing was staged. Use `zmin stage <file>` and run `zmin publish` again.",
    ),
    (
        "publish_everything_staged",
        "Everything is already staged and ready to publish.",
    ),
    ("publish_branch_current", "Working on branch {branch}."),
    ("publish_branch_use_current", "Keep using this branch?"),
    ("publish_branch_new_prompt", "Name for the new branch:"),
    ("publish_branch_switched", "Switched to branch {branch}."),
    ("publish_push_now", "Share this update with the team now?"),
    (
        "publish_review_prompt",
        "Would you like to request a review once it uploads?",
    ),
    (
        "publish_review_manual_hint",
        "Share the update with your reviewers in chat while we finish the automation.",
    ),
    ("stage_path_staged", "Staged {path}"),
    (
        "stage_reject_git_dir",
        "Cannot stage files inside .git ({path})",
    ),
    (
        "unlock_repo_banner",
        "Unlocked repository {repo_id} for {email} (org {org_id}). Remote {remote}.",
    ),
    (
        "merge_fast_forward",
        "Fast-forwarded current branch to {branch} at {commit}.",
    ),
    (
        "merge_up_to_date",
        "Already up to date with the target branch.",
    ),
    ("merge_commit_created", "Recorded merge commit {commit}."),
    (
        "tag_created",
        "Signed tag {tag} ({id}) now points to commit {commit}.",
    ),
    ("changes_summary_heading", "Summary of changes:"),
    ("changes_ready_heading", "Already staged (ready to publish)"),
    (
        "changes_pending_heading",
        "Pending additions before publish",
    ),
    ("changes_modified_label", "Updated files"),
    ("changes_new_label", "New files"),
    ("changes_deleted_label", "Removed files"),
    (
        "preview_intro",
        "Preview only — nothing will be staged or committed.",
    ),
    (
        "preview_followup_hint",
        "Run `zmin publish` when you're happy with the pending list.",
    ),
    ("sync_checking", "Checking the remote for new changes..."),
    ("sync_up_to_date", "Everything is up to date."),
    (
        "sync_new_commits_heading",
        "New updates on branch {branch}:",
    ),
    ("sync_new_commit_entry", "{id} — {message}"),
    ("sync_branch_updates_heading", "Branch summaries:"),
    (
        "sync_branch_new_entry",
        "• Created branch {branch} at {id}: {message}",
    ),
    (
        "sync_branch_update_entry",
        "• Updated branch {branch} to {id}: {message}",
    ),
    ("sync_commit_message_fallback", "(no description provided)"),
    ("sync_branch_removed_heading", "Branches removed remotely:"),
    ("sync_branch_removed_entry", "• {branch}"),
    ("reflog_heading", "Recent reference updates:"),
    ("reflog_empty", "No recorded HEAD movements yet."),
    ("reflog_none", "(none)"),
    (
        "reflog_entry",
        "{timestamp} {reference}: {old} -> {new} - {message}",
    ),
];
