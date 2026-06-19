pub const STRINGS: &[(&str, &str)] = &[
    (
        "login_connecting",
        "Łączenie z serwisem uwierzytelniania Zmin pod adresem {url}...",
    ),
    (
        "login_verification_title",
        "Kod weryfikacyjny (wpisz go w przeglądarce):",
    ),
    ("login_verification_code", "  {code}"),
    (
        "login_verification_instruction",
        "Przed logowaniem zostaniesz poproszony o podanie tego kodu.",
    ),
    (
        "login_press_enter",
        "Naciśnij ENTER, aby otworzyć stronę weryfikacji.",
    ),
    (
        "login_opening_browser",
        "Otwieranie strony weryfikacji: {url}",
    ),
    (
        "login_browser_launch_failed",
        "Nie udało się automatycznie uruchomić przeglądarki ({err}).",
    ),
    (
        "login_open_link_manually",
        "W razie potrzeby otwórz ten link ręcznie: {url}",
    ),
    ("login_success", "Zalogowano jako {email} ({server})."),
    (
        "unlock_success",
        "Przestrzeń robocza odblokowana dla {email}.",
    ),
    ("lock_success", "Przestrzeń robocza zablokowana."),
    ("logout_success", "Wylogowano."),
    (
        "clone_session_saved",
        "Znaleziono zapisane konto {email} (organizacja {org_id}) dla {server}.",
    ),
    (
        "clone_multiple_accounts",
        "Znaleziono kilka zapisanych kont dla {server}. Wybierz jedno lub zaloguj się innym kontem:",
    ),
    ("clone_use_saved_account", "Użyć zapisanego konta?"),
    (
        "clone_add_account_prompt",
        "Zalogować się innym kontem dla {server}? Zastąpi to obecną sesję.",
    ),
    ("clone_no_account", "Brak zapisanego konta dla {server}."),
    ("clone_login_now", "Zalogować się teraz, aby kontynuować?"),
    (
        "clone_login_abort",
        "Anulowano klonowanie, ponieważ nie wybrano konta dla {server}.",
    ),
    (
        "clone_session_mismatch",
        "Konto {email} należy do organizacji {session_org}, ale repozytorium {repo_id} wymaga organizacji {remote_org}.",
    ),
    (
        "clone_replace_account_prompt",
        "Zalogować się innym kontem dla {server}?",
    ),
    (
        "clone_mismatch_abort",
        "Anulowano klonowanie, ponieważ bieżące konto nie ma dostępu do {server}.",
    ),
    (
        "auth_no_identity",
        "Tworzenie zabezpieczonej tożsamości dla {email}...",
    ),
    (
        "auth_seed_phrase_banner",
        "\n=== Fraza odzyskiwania — przechowuj bezpiecznie ===\n{phrase}\n===\n",
    ),
    (
        "auth_seed_phrase_tip",
        "Zapisz tę frazę i przechowuj ją offline. Będzie potrzebna do odzyskania konta.",
    ),
    (
        "auth_seed_fingerprint",
        "Odcisk frazy (użyj przy odzyskiwaniu): {fingerprint}",
    ),
    (
        "auth_identity_ready",
        "Tożsamość utworzona, przestrzeń robocza odblokowana.",
    ),
    (
        "auth_unlock_failed",
        "Nie udało się odblokować przestrzeni tym hasłem ({err}). Rozpoczynam odzyskiwanie za pomocą frazy...",
    ),
    (
        "auth_password_attempt_failed",
        "Niepoprawne hasło główne. Pozostało prób: {remaining}.",
    ),
    (
        "auth_password_attempts_exhausted",
        "Wykorzystano {attempts} prób.",
    ),
    (
        "auth_prompt_recover_now",
        "Rozpocząć odzyskiwanie {email} na {server} przy użyciu frazy seed?",
    ),
    (
        "auth_recover_intro",
        "Rozpoczynam odzyskiwanie dla {email} na {server}.",
    ),
    (
        "auth_recover_identity_missing",
        "Nie znaleziono lokalnej tożsamości dla {email} na {server}; uruchom `zmin login`.",
    ),
    (
        "publish_no_changes",
        "Wszystko aktualne; brak zmian do opublikowania.",
    ),
    ("publish_prompt_message", "Nagłówek dla tej aktualizacji:"),
    ("publish_commit_done", "Zapisano aktualizację {id}."),
    ("publish_pushed", "Opublikowano zmiany na {remote}."),
    (
        "publish_push_skipped",
        "Publikacja zdalna pominięta (pozostają lokalnie).",
    ),
    ("auth_prompt_seed_phrase", "Fraza odzyskiwania"),
    (
        "auth_seed_recovery_start",
        "Podaj frazę odzyskiwania, aby odzyskać dostęp.",
    ),
    (
        "auth_identity_recovered",
        "Tożsamość odzyskana. Hasło główne zaktualizowane.",
    ),
    (
        "accounts_header",
        "Znaleziono kilka sesji dla {base}. Wybierz jedną:",
    ),
    (
        "accounts_entry",
        "  [{index}] {email} (organizacja {org_id})",
    ),
    ("prompt_select_account", "Wybierz sesję"),
    (
        "warn_selection_out_of_range",
        "Wybór poza zakresem. Spróbuj ponownie.",
    ),
    ("prompt_master_password", "Hasło główne:"),
    (
        "prompt_new_master_password",
        "Nowe hasło główne (minimum 12 znaków):",
    ),
    ("prompt_confirm_master_password", "Potwierdź hasło główne:"),
    (
        "warn_password_too_short",
        "Hasło musi mieć co najmniej 12 znaków.",
    ),
    (
        "warn_passwords_mismatch",
        "Hasła nie są takie same. Spróbuj ponownie.",
    ),
    (
        "password_guidance",
        "Wskazówka: użyj co najmniej 12 znaków, łącz litery, cyfry i symbole. Akceptowane są tylko znaki UTF-8.",
    ),
    (
        "password_utf8_error",
        "Hasło zawiera nieobsługiwane znaki. Użyj znaków UTF-8.",
    ),
    (
        "auth_biometric_enabled",
        "Odblokowanie biometryczne włączone. Zarządzaj dostępem w ustawieniach systemowego magazynu kluczy.",
    ),
    (
        "auth_biometric_disabled",
        "Odblokowanie biometryczne zostało wyłączone dla tego konta.",
    ),
    (
        "auth_biometric_unavailable",
        "Odblokowanie biometryczne jest niedostępne. Wróć do hasła głównego.",
    ),
    (
        "prompt_enable_biometric",
        "Włączyć odblokowanie biometryczne na tym urządzeniu?",
    ),
    (
        "prompt_keep_biometric_enabled",
        "Pozostawić odblokowanie biometryczne włączone na przyszłość?",
    ),
    (
        "repo_stage_on_lock_prompt",
        "Czy podczas blokowania zaszyfrować przygotowane zmiany i usunąć z tego urządzenia odszyfrowane pliki robocze?",
    ),
    ("warn_invalid_choice", "Odpowiedz 'y' lub 'n'."),
    (
        "auth_seed_confirm_prompt",
        "Przepisz frazę odzyskiwania, aby potwierdzić zapisanie:",
    ),
    (
        "auth_seed_confirm_mismatch",
        "Fraza nie pasuje. Spróbuj ponownie.",
    ),
    ("prompt_seed_phrase", "Fraza seed"),
    (
        "error_read_input",
        "Nie udało się odczytać danych. Spróbuj ponownie.",
    ),
    ("error_input_required", "Wymagane jest podanie wartości."),
    ("publish_intro", "Przygotujmy Twoją aktualizację."),
    (
        "publish_confirm_stage",
        "Dodać teraz wszystkie wymienione zmiany?",
    ),
    (
        "publish_stage_skip_hint",
        "Nic nie zostało dodane. Użyj `zmin stage <plik>` i ponownie uruchom `zmin publish`.",
    ),
    (
        "publish_everything_staged",
        "Wszystkie zmiany są już dodane i gotowe do publikacji.",
    ),
    ("publish_branch_current", "Pracujesz na gałęzi {branch}."),
    ("publish_branch_use_current", "Pozostać na tej gałęzi?"),
    ("publish_branch_new_prompt", "Nazwa nowej gałęzi:"),
    ("publish_branch_switched", "Przełączono na gałąź {branch}."),
    ("publish_push_now", "Udostępnić teraz ten pakiet zespołowi?"),
    (
        "publish_review_prompt",
        "Czy po wysłaniu poprosić od razu o przegląd?",
    ),
    (
        "publish_review_manual_hint",
        "Do czasu automatyki podziel się aktualizacją z recenzentami na czacie.",
    ),
    ("stage_path_staged", "Dodano {path}"),
    (
        "stage_reject_git_dir",
        "Nie można dodać plików wewnątrz .git ({path})",
    ),
    (
        "unlock_repo_banner",
        "Repozytorium {repo_id} odblokowane dla {email} (organizacja {org_id}). Zdalny serwer {remote}.",
    ),
    (
        "merge_fast_forward",
        "Gałąź zaktualizowana szybkim przesunięciem do {branch} ({commit}).",
    ),
    (
        "merge_up_to_date",
        "Nie ma czego scalać – wszystko jest aktualne.",
    ),
    (
        "merge_commit_created",
        "Utworzono commit scalający {commit}.",
    ),
    (
        "tag_created",
        "Podpisany tag {tag} ({id}) wskazuje teraz na commit {commit}.",
    ),
    ("changes_summary_heading", "Podsumowanie zmian:"),
    ("changes_ready_heading", "Już dodane (gotowe do publikacji)"),
    ("changes_pending_heading", "Zmiany wymagające dodania"),
    ("changes_modified_label", "Zmienione pliki"),
    ("changes_new_label", "Nowe pliki"),
    ("changes_deleted_label", "Usunięte pliki"),
    (
        "preview_intro",
        "Tylko podgląd — nic nie zostanie dodane ani zatwierdzone.",
    ),
    (
        "preview_followup_hint",
        "Gdy lista będzie gotowa, uruchom `zmin publish`.",
    ),
    (
        "sync_checking",
        "Sprawdzam zdalne repozytorium w poszukiwaniu zmian...",
    ),
    ("sync_up_to_date", "Wszystko jest aktualne."),
    (
        "sync_new_commits_heading",
        "Nowe zmiany na gałęzi {branch}:",
    ),
    ("sync_new_commit_entry", "{id} — {message}"),
    ("sync_branch_updates_heading", "Podsumowanie gałęzi:"),
    (
        "sync_branch_new_entry",
        "• Utworzono gałąź {branch} na {id}: {message}",
    ),
    (
        "sync_branch_update_entry",
        "• Zaktualizowano gałąź {branch} do {id}: {message}",
    ),
    ("sync_commit_message_fallback", "(brak opisu)"),
    (
        "sync_branch_removed_heading",
        "Gałęzie usunięte po stronie zdalnej:",
    ),
    ("sync_branch_removed_entry", "• {branch}"),
    ("reflog_heading", "Ostatnie aktualizacje odniesień:"),
    ("reflog_empty", "Brak zapisanych ruchów HEAD."),
    ("reflog_none", "(brak)"),
    (
        "reflog_entry",
        "{timestamp} {reference}: {old} -> {new} - {message}",
    ),
];
