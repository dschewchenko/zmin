pub const STRINGS: &[(&str, &str)] = &[
    (
        "login_connecting",
        "Підключення до служби автентифікації Skron за адресою {url}...",
    ),
    (
        "login_verification_title",
        "Код підтвердження (введіть його в браузері):",
    ),
    ("login_verification_code", "  {code}"),
    (
        "login_verification_instruction",
        "Перед входом доведеться ввести цей код.",
    ),
    (
        "login_press_enter",
        "Натисніть ENTER, щоб відкрити сторінку підтвердження.",
    ),
    (
        "login_opening_browser",
        "Відкриваємо сторінку підтвердження: {url}",
    ),
    (
        "login_browser_launch_failed",
        "Не вдалося автоматично відкрити браузер ({err}).",
    ),
    (
        "login_open_link_manually",
        "Якщо потрібно, відкрийте це посилання вручну: {url}",
    ),
    ("login_success", "Увійшли як {email} ({server})."),
    (
        "unlock_success",
        "Робочий простір розблоковано для {email}.",
    ),
    ("lock_success", "Робочий простір заблоковано."),
    ("logout_success", "Вийшли з облікового запису."),
    (
        "clone_session_saved",
        "Знайдено збережений обліковий запис {email} (організація {org_id}) для {server}.",
    ),
    (
        "clone_multiple_accounts",
        "Знайдено кілька збережених облікових записів для {server}. Оберіть один або увійдіть під іншим обліковим записом:",
    ),
    (
        "clone_use_saved_account",
        "Використати цей обліковий запис?",
    ),
    (
        "clone_add_account_prompt",
        "Увійти під іншим обліковим записом для {server}? Поточна сесія буде замінена.",
    ),
    (
        "clone_no_account",
        "Для {server} немає збереженого облікового запису.",
    ),
    ("clone_login_now", "Увійти зараз, щоб продовжити?"),
    (
        "clone_login_abort",
        "Клонування скасовано, оскільки обліковий запис для {server} не вибрано.",
    ),
    (
        "clone_session_mismatch",
        "Обліковий запис {email} належить організації {session_org}, але репозиторій {repo_id} вимагає організацію {remote_org}.",
    ),
    (
        "clone_replace_account_prompt",
        "Увійти під іншим обліковим записом для {server}?",
    ),
    (
        "clone_mismatch_abort",
        "Клонування скасовано, бо поточний обліковий запис не має доступу до {server}.",
    ),
    (
        "auth_no_identity",
        "Створюємо захищену ідентичність для {email}...",
    ),
    (
        "auth_seed_phrase_banner",
        "\n=== Фраза відновлення — зберігайте надійно ===\n{phrase}\n===\n",
    ),
    (
        "auth_seed_phrase_tip",
        "Запишіть цю фразу та зберігайте офлайн. Вона знадобиться для відновлення облікового запису.",
    ),
    (
        "auth_seed_fingerprint",
        "Відбиток фрази (перевіряйте під час відновлення): {fingerprint}",
    ),
    (
        "auth_identity_ready",
        "Ідентичність створено, робочий простір розблоковано.",
    ),
    (
        "auth_unlock_failed",
        "Не вдалося розблокувати простір цим паролем ({err}). Переходимо до відновлення за сид-фразою...",
    ),
    (
        "auth_password_attempt_failed",
        "Невірний мастер-пароль. Залишилося спроб: {remaining}.",
    ),
    (
        "auth_password_attempts_exhausted",
        "Вичерпано {attempts} спроб.",
    ),
    (
        "auth_prompt_recover_now",
        "Розпочати відновлення {email} на {server} за допомогою сид-фрази?",
    ),
    (
        "auth_recover_intro",
        "Починаємо відновлення для {email} на {server}.",
    ),
    (
        "auth_recover_identity_missing",
        "Локальну ідентичність для {email} на {server} не знайдено; запустіть `skron login`.",
    ),
    ("publish_no_changes", "Все актуально; публікувати нічого."),
    ("publish_prompt_message", "Заголовок для цього оновлення:"),
    ("publish_commit_done", "Оновлення {id} збережено."),
    ("publish_pushed", "Оновлення опубліковано на {remote}."),
    (
        "publish_push_skipped",
        "Відправлення на сервер пропущено (залишаємо локально).",
    ),
    ("auth_prompt_seed_phrase", "Фраза відновлення"),
    (
        "auth_seed_recovery_start",
        "Введіть фразу відновлення, щоб повернути доступ.",
    ),
    (
        "auth_identity_recovered",
        "Ідентичність відновлено. Мастер-пароль оновлено.",
    ),
    (
        "accounts_header",
        "Для {base} знайдено кілька сесій. Оберіть одну:",
    ),
    (
        "accounts_entry",
        "  [{index}] {email} (організація {org_id})",
    ),
    ("prompt_select_account", "Оберіть сесію"),
    (
        "warn_selection_out_of_range",
        "Номер поза діапазоном. Спробуйте ще раз.",
    ),
    ("prompt_master_password", "Мастер-пароль:"),
    (
        "prompt_new_master_password",
        "Новий мастер-пароль (мінімум 12 символів):",
    ),
    (
        "prompt_confirm_master_password",
        "Підтвердіть мастер-пароль:",
    ),
    (
        "warn_password_too_short",
        "Пароль має містити щонайменше 12 символів.",
    ),
    (
        "warn_passwords_mismatch",
        "Паролі не співпадають. Спробуйте ще раз.",
    ),
    (
        "password_guidance",
        "Порада: використовуйте щонайменше 12 символів, комбінуйте літери, цифри та знаки. Допускаються лише символи UTF-8.",
    ),
    (
        "password_utf8_error",
        "Пароль містить непідтримувані символи. Використовуйте символи UTF-8.",
    ),
    (
        "auth_biometric_enabled",
        "Біометричне розблокування увімкнене. Керуйте доступом у налаштуваннях сховища ключів ОС.",
    ),
    (
        "auth_biometric_disabled",
        "Біометричне розблокування вимкнено для цього облікового запису.",
    ),
    (
        "auth_biometric_unavailable",
        "Біометричне розблокування недоступне. Повертаємося до мастер-пароля.",
    ),
    (
        "prompt_enable_biometric",
        "Увімкнути біометричне розблокування на цьому пристрої?",
    ),
    (
        "prompt_keep_biometric_enabled",
        "Залишити біометричне розблокування увімкненим надалі?",
    ),
    (
        "repo_stage_on_lock_prompt",
        "Під час блокування зашифрувати зафіксовані зміни та видалити розшифровані робочі файли на цьому пристрої?",
    ),
    ("warn_invalid_choice", "Відповідайте 'y' або 'n'."),
    (
        "auth_seed_confirm_prompt",
        "Повторно введіть фразу відновлення, щоб підтвердити збереження:",
    ),
    (
        "auth_seed_confirm_mismatch",
        "Фраза не співпадає. Спробуйте ще раз.",
    ),
    ("prompt_seed_phrase", "Сид-фраза"),
    (
        "error_read_input",
        "Не вдалося зчитати введення. Спробуйте ще раз.",
    ),
    ("error_input_required", "Потрібно ввести значення."),
    ("publish_intro", "Підготуємо ваше оновлення."),
    (
        "publish_confirm_stage",
        "Додати до оновлення всі перелічені зміни зараз?",
    ),
    (
        "publish_stage_skip_hint",
        "Зміни не додано. Використайте `skron stage <файл>` і повторіть `skron publish`.",
    ),
    (
        "publish_everything_staged",
        "Усі зміни вже додані й готові до публікації.",
    ),
    ("publish_branch_current", "Працюємо в гілці {branch}."),
    ("publish_branch_use_current", "Залишити цю гілку?"),
    ("publish_branch_new_prompt", "Назва нової гілки:"),
    ("publish_branch_switched", "Перейшли до гілки {branch}."),
    ("publish_push_now", "Відправити оновлення команді зараз?"),
    (
        "publish_review_prompt",
        "Створити запит на перегляд після завантаження?",
    ),
    (
        "publish_review_manual_hint",
        "Поки автоматизація ще готується, поділіться оновленням у чаті з рецензентами.",
    ),
    ("stage_path_staged", "Зафіксовано {path}"),
    (
        "stage_reject_git_dir",
        "Неможливо зафіксувати файли всередині .git ({path})",
    ),
    (
        "unlock_repo_banner",
        "Репозиторій {repo_id} розблоковано для {email} (організація {org_id}). Віддалений сервер {remote}.",
    ),
    (
        "merge_fast_forward",
        "Гілку оновлено швидким перенесенням до {branch} ({commit}).",
    ),
    ("merge_up_to_date", "Поточна гілка вже містить усі зміни."),
    ("merge_commit_created", "Створено merge-коміт {commit}."),
    (
        "tag_created",
        "Підписаний тег {tag} ({id}) тепер вказує на коміт {commit}.",
    ),
    ("changes_summary_heading", "Підсумок змін:"),
    ("changes_ready_heading", "Вже додано (готово до публікації)"),
    ("changes_pending_heading", "Зміни, які ще потрібно додати"),
    ("changes_modified_label", "Оновлені файли"),
    ("changes_new_label", "Нові файли"),
    ("changes_deleted_label", "Видалені файли"),
    (
        "preview_intro",
        "Лише попередній перегляд — без додавання чи коміту.",
    ),
    (
        "preview_followup_hint",
        "Коли все готово, запустіть `skron publish`.",
    ),
    (
        "sync_checking",
        "Перевіряємо віддалений репозиторій на нові зміни...",
    ),
    ("sync_up_to_date", "Усе актуально."),
    ("sync_new_commits_heading", "Нові зміни у гілці {branch}:"),
    ("sync_new_commit_entry", "{id} — {message}"),
    ("sync_branch_updates_heading", "Оновлення гілок:"),
    (
        "sync_branch_new_entry",
        "• Створено гілку {branch} на {id}: {message}",
    ),
    (
        "sync_branch_update_entry",
        "• Оновлено гілку {branch} до {id}: {message}",
    ),
    ("sync_commit_message_fallback", "(опис відсутній)"),
    ("sync_branch_removed_heading", "Видалені віддалені гілки:"),
    ("sync_branch_removed_entry", "• {branch}"),
    ("reflog_heading", "Останні зміни посилань:"),
    ("reflog_empty", "Записів про рух HEAD ще немає."),
    ("reflog_none", "(немає)"),
    (
        "reflog_entry",
        "{timestamp} {reference}: {old} -> {new} - {message}",
    ),
];
