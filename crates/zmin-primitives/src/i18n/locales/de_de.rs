pub const STRINGS: &[(&str, &str)] = &[
    (
        "login_connecting",
        "Verbindung zum Zmin-Authentifizierungsdienst unter {url} wird hergestellt...",
    ),
    (
        "login_verification_title",
        "Verifizierungscode (im Browser eingeben):",
    ),
    ("login_verification_code", "  {code}"),
    (
        "login_verification_instruction",
        "Dieser Code muss vor der Anmeldung eingegeben werden.",
    ),
    (
        "login_press_enter",
        "Drücke ENTER, um die Verifizierungsseite zu öffnen.",
    ),
    ("login_opening_browser", "Öffne Verifizierungsseite: {url}"),
    (
        "login_browser_launch_failed",
        "Der Browser konnte nicht automatisch gestartet werden ({err}).",
    ),
    (
        "login_open_link_manually",
        "Öffne diesen Link bei Bedarf manuell: {url}",
    ),
    ("login_success", "Angemeldet als {email} ({server})."),
    ("unlock_success", "Arbeitsbereich für {email} entsperrt."),
    ("lock_success", "Arbeitsbereich gesperrt."),
    ("logout_success", "Abgemeldet."),
    (
        "clone_session_saved",
        "Gespeichertes Konto {email} (Organisation {org_id}) für {server} gefunden.",
    ),
    (
        "clone_multiple_accounts",
        "Mehrere gespeicherte Konten für {server} gefunden. Wähle eines aus oder melde dich mit einem anderen Konto an:",
    ),
    ("clone_use_saved_account", "Gespeichertes Konto verwenden?"),
    (
        "clone_add_account_prompt",
        "Mit einem anderen Konto für {server} anmelden? Dadurch wird die gespeicherte Sitzung ersetzt.",
    ),
    (
        "clone_no_account",
        "Kein gespeichertes Konto für {server} vorhanden.",
    ),
    ("clone_login_now", "Jetzt anmelden, um fortzufahren?"),
    (
        "clone_login_abort",
        "Klonen abgebrochen, da kein Konto für {server} ausgewählt wurde.",
    ),
    (
        "clone_session_mismatch",
        "Konto {email} gehört zur Organisation {session_org}, das Repository {repo_id} erfordert jedoch die Organisation {remote_org}.",
    ),
    (
        "clone_replace_account_prompt",
        "Mit einem anderen Konto für {server} anmelden?",
    ),
    (
        "clone_mismatch_abort",
        "Klonen abgebrochen, da das aktuelle Konto keinen Zugriff auf {server} hat.",
    ),
    (
        "auth_no_identity",
        "Erstelle eine geschützte Identität für {email}...",
    ),
    (
        "auth_seed_phrase_banner",
        "\n=== Wiederherstellungsphrase – sicher aufbewahren ===\n{phrase}\n===\n",
    ),
    (
        "auth_seed_phrase_tip",
        "Notiere diese Phrase und bewahre sie offline auf. Sie wird zur Kontowiederherstellung benötigt.",
    ),
    (
        "auth_seed_fingerprint",
        "Fingerabdruck der Phrase (bei der Wiederherstellung prüfen): {fingerprint}",
    ),
    (
        "auth_identity_ready",
        "Identität erstellt und Arbeitsbereich entsperrt.",
    ),
    (
        "auth_unlock_failed",
        "Der Arbeitsbereich konnte mit diesem Passwort ({err}) nicht entsperrt werden. Starte die Wiederherstellung mit deiner Sicherungsphrase...",
    ),
    (
        "auth_password_attempt_failed",
        "Falsches Master-Passwort. Verbleibende Versuche: {remaining}.",
    ),
    (
        "auth_password_attempts_exhausted",
        "Maximal {attempts} Versuche erreicht.",
    ),
    (
        "auth_prompt_recover_now",
        "Soll {email} auf {server} jetzt mit der Sicherungsphrase wiederhergestellt werden?",
    ),
    (
        "auth_recover_intro",
        "Starte die Wiederherstellung für {email} auf {server}.",
    ),
    (
        "auth_recover_identity_missing",
        "Keine lokale Identität für {email} auf {server} gefunden; führe `zmin login` aus.",
    ),
    (
        "publish_no_changes",
        "Alles erledigt; es gibt nichts zu veröffentlichen.",
    ),
    ("publish_prompt_message", "Titel für diese Aktualisierung:"),
    ("publish_commit_done", "Änderung {id} wurde gespeichert."),
    ("publish_pushed", "Änderungen auf {remote} veröffentlicht."),
    (
        "publish_push_skipped",
        "Veröffentlichung ausgelassen (bleibt lokal auf diesem Gerät).",
    ),
    ("auth_prompt_seed_phrase", "Wiederherstellungsphrase"),
    (
        "auth_seed_recovery_start",
        "Gib deine Wiederherstellungsphrase ein, um den Zugang zurückzuerhalten.",
    ),
    (
        "auth_identity_recovered",
        "Identität wiederhergestellt. Master-Passwort aktualisiert.",
    ),
    (
        "accounts_header",
        "Mehrere gespeicherte Sitzungen für {base}. Bitte auswählen:",
    ),
    (
        "accounts_entry",
        "  [{index}] {email} (Organisation {org_id})",
    ),
    ("prompt_select_account", "Sitzung auswählen"),
    (
        "warn_selection_out_of_range",
        "Auswahl außerhalb des gültigen Bereichs. Bitte erneut versuchen.",
    ),
    ("prompt_master_password", "Master-Passwort:"),
    (
        "prompt_new_master_password",
        "Neues Master-Passwort (mindestens 12 Zeichen):",
    ),
    (
        "prompt_confirm_master_password",
        "Master-Passwort bestätigen:",
    ),
    (
        "warn_password_too_short",
        "Das Passwort muss mindestens 12 Zeichen haben.",
    ),
    (
        "warn_passwords_mismatch",
        "Passwörter stimmen nicht überein. Bitte erneut versuchen.",
    ),
    (
        "password_guidance",
        "Hinweis: Verwende mindestens 12 Zeichen mit Buchstaben, Zahlen und Symbolen. Es werden nur UTF-8-Zeichen akzeptiert.",
    ),
    (
        "password_utf8_error",
        "Das Passwort enthält nicht unterstützte Zeichen. Bitte UTF-8-Text verwenden.",
    ),
    (
        "auth_biometric_enabled",
        "Biometrisches Entsperren aktiviert. Zugriff über die Systemeinstellungen für den Schlüsselbund verwalten.",
    ),
    (
        "auth_biometric_disabled",
        "Biometrisches Entsperren für dieses Konto deaktiviert.",
    ),
    (
        "auth_biometric_unavailable",
        "Biometrisches Entsperren ist nicht verfügbar. Es wird das Master-Passwort verwendet.",
    ),
    (
        "prompt_enable_biometric",
        "Biometrisches Entsperren auf diesem Gerät aktivieren?",
    ),
    (
        "prompt_keep_biometric_enabled",
        "Biometrisches Entsperren für zukünftige Anmeldungen aktiviert lassen?",
    ),
    (
        "repo_stage_on_lock_prompt",
        "Beim Sperren Arbeitsstände verschlüsseln und entschlüsselte Workspace-Dateien auf diesem Gerät entfernen?",
    ),
    ("warn_invalid_choice", "Bitte mit 'y' oder 'n' antworten."),
    (
        "auth_seed_confirm_prompt",
        "Gib die Wiederherstellungsphrase erneut ein, um zu bestätigen, dass sie gespeichert wurde:",
    ),
    (
        "auth_seed_confirm_mismatch",
        "Die Phrase stimmt nicht überein. Bitte erneut versuchen.",
    ),
    ("prompt_seed_phrase", "Wiederherstellungsphrase"),
    (
        "error_read_input",
        "Eingabe konnte nicht gelesen werden. Bitte erneut versuchen.",
    ),
    ("error_input_required", "Eine Eingabe ist erforderlich."),
    ("publish_intro", "Lass uns dein Update vorbereiten."),
    (
        "publish_confirm_stage",
        "Sollen alle oben aufgeführten Änderungen jetzt übernommen werden?",
    ),
    (
        "publish_stage_skip_hint",
        "Es wurde nichts übernommen. Nutze `zmin stage <Datei>` und starte `zmin publish` erneut.",
    ),
    (
        "publish_everything_staged",
        "Alle Änderungen sind bereits vorbereitet und können veröffentlicht werden.",
    ),
    (
        "publish_branch_current",
        "Aktuell arbeitest du auf dem Branch {branch}.",
    ),
    ("publish_branch_use_current", "Diesen Branch beibehalten?"),
    ("publish_branch_new_prompt", "Name für den neuen Branch:"),
    ("publish_branch_switched", "Zum Branch {branch} gewechselt."),
    (
        "publish_push_now",
        "Soll dieses Update jetzt mit dem Team geteilt werden?",
    ),
    (
        "publish_review_prompt",
        "Nach dem Hochladen direkt eine Review anfragen?",
    ),
    (
        "publish_review_manual_hint",
        "Teile das Update bis dahin manuell im Chat mit den Reviewer*innen.",
    ),
    ("stage_path_staged", "{path} hinzugefügt"),
    (
        "stage_reject_git_dir",
        "Dateien innerhalb von .git ({path}) können nicht hinzugefügt werden",
    ),
    (
        "unlock_repo_banner",
        "Repository {repo_id} für {email} (Organisation {org_id}) freigegeben. Remote {remote}.",
    ),
    (
        "merge_fast_forward",
        "Branch wurde per Fast-Forward auf {branch} ({commit}) aktualisiert.",
    ),
    (
        "merge_up_to_date",
        "Der aktuelle Branch ist bereits auf dem neuesten Stand.",
    ),
    (
        "merge_commit_created",
        "Merge-Commit {commit} wurde erstellt.",
    ),
    (
        "tag_created",
        "Signierter Tag {tag} ({id}) verweist jetzt auf Commit {commit}.",
    ),
    ("changes_summary_heading", "Änderungsübersicht:"),
    (
        "changes_ready_heading",
        "Bereits vorbereitet (bereit zur Veröffentlichung)",
    ),
    ("changes_pending_heading", "Noch aufzunehmende Änderungen"),
    ("changes_modified_label", "Geänderte Dateien"),
    ("changes_new_label", "Neue Dateien"),
    ("changes_deleted_label", "Entfernte Dateien"),
    (
        "preview_intro",
        "Nur Vorschau – es wird nichts hinzugefügt oder committed.",
    ),
    (
        "preview_followup_hint",
        "Starte `zmin publish`, sobald du mit der Liste zufrieden bist.",
    ),
    (
        "sync_checking",
        "Prüfe das Remote-Repository auf neue Änderungen...",
    ),
    ("sync_up_to_date", "Alles ist auf dem neuesten Stand."),
    (
        "sync_new_commits_heading",
        "Neue Updates auf dem Branch {branch}:",
    ),
    ("sync_new_commit_entry", "{id} — {message}"),
    ("sync_branch_updates_heading", "Branch-Zusammenfassung:"),
    (
        "sync_branch_new_entry",
        "• Branch {branch} wurde bei {id} erstellt: {message}",
    ),
    (
        "sync_branch_update_entry",
        "• Branch {branch} wurde auf {id} aktualisiert: {message}",
    ),
    ("sync_commit_message_fallback", "(keine Beschreibung)"),
    (
        "sync_branch_removed_heading",
        "Im Remote entfernte Branches:",
    ),
    ("sync_branch_removed_entry", "• {branch}"),
    ("reflog_heading", "Aktuelle Referenzänderungen:"),
    ("reflog_empty", "Keine protokollierten HEAD-Bewegungen."),
    ("reflog_none", "(keine)"),
    (
        "reflog_entry",
        "{timestamp} {reference}: {old} -> {new} - {message}",
    ),
];
