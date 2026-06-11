pub const STRINGS: &[(&str, &str)] = &[
    (
        "login_connecting",
        "Conectando con el servicio de autenticación de Skron en {url}...",
    ),
    (
        "login_verification_title",
        "Código de verificación (introduce esto en tu navegador):",
    ),
    ("login_verification_code", "  {code}"),
    (
        "login_verification_instruction",
        "Se te pedirá que introduzcas este código antes de iniciar sesión.",
    ),
    (
        "login_press_enter",
        "Pulsa ENTER para abrir la página de verificación.",
    ),
    (
        "login_opening_browser",
        "Abriendo la página de verificación: {url}",
    ),
    (
        "login_browser_launch_failed",
        "No se pudo abrir el navegador automáticamente ({err}).",
    ),
    (
        "login_open_link_manually",
        "Abre este enlace manualmente si es necesario: {url}",
    ),
    (
        "login_success",
        "Has iniciado sesión como {email} ({server}).",
    ),
    (
        "unlock_success",
        "Espacio de trabajo desbloqueado para {email}.",
    ),
    ("lock_success", "Espacio de trabajo bloqueado."),
    ("logout_success", "Sesión cerrada."),
    (
        "clone_session_saved",
        "Se encontró la cuenta guardada {email} (org {org_id}) para {server}.",
    ),
    (
        "clone_multiple_accounts",
        "Se encontraron varias cuentas guardadas para {server}. Elige una o inicia sesión con otra cuenta:",
    ),
    ("clone_use_saved_account", "¿Usar esta cuenta guardada?"),
    (
        "clone_add_account_prompt",
        "¿Iniciar sesión con otra cuenta para {server}? Esto reemplaza la sesión guardada.",
    ),
    (
        "clone_no_account",
        "No hay cuentas guardadas para {server}.",
    ),
    ("clone_login_now", "¿Iniciar sesión ahora para continuar?"),
    (
        "clone_login_abort",
        "Clonado cancelado porque no se seleccionó ninguna cuenta para {server}.",
    ),
    (
        "clone_session_mismatch",
        "La cuenta {email} pertenece a la organización {session_org}, pero el repositorio {repo_id} requiere la organización {remote_org}.",
    ),
    (
        "clone_replace_account_prompt",
        "¿Iniciar sesión con otra cuenta para {server}?",
    ),
    (
        "clone_mismatch_abort",
        "Clonado cancelado porque la cuenta actual no tiene acceso a {server}.",
    ),
    (
        "auth_no_identity",
        "Creando una identidad protegida para {email}...",
    ),
    (
        "auth_seed_phrase_banner",
        "\n=== Frase de recuperación — guárdala en un lugar seguro ===\n{phrase}\n===\n",
    ),
    (
        "auth_seed_phrase_tip",
        "Anota esta frase y consérvala sin conexión. La necesitarás para recuperar tu cuenta.",
    ),
    (
        "auth_seed_fingerprint",
        "Huella de la frase (verifícala durante la recuperación): {fingerprint}",
    ),
    (
        "auth_identity_ready",
        "Identidad creada y espacio de trabajo desbloqueado.",
    ),
    (
        "auth_unlock_failed",
        "No pudimos desbloquear el espacio con esa contraseña ({err}). Iniciando la recuperación con tu frase de respaldo...",
    ),
    (
        "auth_password_attempt_failed",
        "Contraseña maestra incorrecta. Quedan {remaining} intento(s).",
    ),
    (
        "auth_password_attempts_exhausted",
        "Se alcanzó el máximo de {attempts} intentos.",
    ),
    (
        "auth_prompt_recover_now",
        "¿Recuperar ahora a {email} en {server} usando la frase de respaldo?",
    ),
    (
        "auth_recover_intro",
        "Iniciando la recuperación de {email} en {server}.",
    ),
    (
        "auth_recover_identity_missing",
        "No se encontró una identidad local para {email} en {server}; ejecuta `skron login`.",
    ),
    (
        "publish_no_changes",
        "No hay nada que publicar; el espacio de trabajo está limpio.",
    ),
    ("publish_prompt_message", "Descripción del cambio:"),
    ("publish_commit_done", "Se registró el cambio {id}."),
    ("publish_pushed", "Cambios publicados en {remote}."),
    (
        "publish_push_skipped",
        "Se omite la publicación remota (las actualizaciones quedan en este dispositivo).",
    ),
    ("auth_prompt_seed_phrase", "Frase de recuperación"),
    (
        "auth_seed_recovery_start",
        "Introduce tu frase de recuperación para recuperar el acceso.",
    ),
    (
        "auth_identity_recovered",
        "Identidad recuperada. Contraseña maestra actualizada.",
    ),
    (
        "accounts_header",
        "Se encontraron varias sesiones guardadas para {base}. Selecciona una:",
    ),
    (
        "accounts_entry",
        "  [{index}] {email} (organización {org_id})",
    ),
    ("prompt_select_account", "Selecciona sesión"),
    (
        "warn_selection_out_of_range",
        "La selección está fuera de rango. Inténtalo de nuevo.",
    ),
    ("prompt_master_password", "Contraseña maestra:"),
    (
        "prompt_new_master_password",
        "Nueva contraseña maestra (mínimo 12 caracteres):",
    ),
    (
        "prompt_confirm_master_password",
        "Confirma la contraseña maestra:",
    ),
    (
        "warn_password_too_short",
        "La contraseña debe tener al menos 12 caracteres.",
    ),
    (
        "warn_passwords_mismatch",
        "Las contraseñas no coinciden. Inténtalo de nuevo.",
    ),
    (
        "password_guidance",
        "Consejo: usa al menos 12 caracteres combinando letras, números y símbolos. Solo se aceptan caracteres UTF-8.",
    ),
    (
        "password_utf8_error",
        "La contraseña contiene caracteres no admitidos. Usa texto UTF-8.",
    ),
    (
        "auth_biometric_enabled",
        "Desbloqueo biométrico activado. Gestiona el acceso desde los ajustes del llavero del sistema.",
    ),
    (
        "auth_biometric_disabled",
        "Desbloqueo biométrico desactivado para esta cuenta.",
    ),
    (
        "auth_biometric_unavailable",
        "El desbloqueo biométrico no está disponible. Se utilizará la contraseña maestra.",
    ),
    (
        "prompt_enable_biometric",
        "¿Activar el desbloqueo biométrico en este dispositivo?",
    ),
    (
        "prompt_keep_biometric_enabled",
        "¿Mantener el desbloqueo biométrico para futuros accesos?",
    ),
    (
        "repo_stage_on_lock_prompt",
        "Al bloquear, ¿cifrar los cambios preparados y eliminar los archivos en texto claro del workspace en este dispositivo?",
    ),
    ("warn_invalid_choice", "Responde con 'y' (yes) o 'n' (no)."),
    (
        "auth_seed_confirm_prompt",
        "Vuelve a escribir la frase de recuperación para confirmar que la guardaste:",
    ),
    (
        "auth_seed_confirm_mismatch",
        "La frase no coincide. Inténtalo de nuevo.",
    ),
    ("prompt_seed_phrase", "Frase de recuperación"),
    (
        "error_read_input",
        "No se pudo leer la entrada. Inténtalo de nuevo.",
    ),
    ("error_input_required", "Se requiere una entrada."),
    ("publish_intro", "Vamos a preparar tu actualización."),
    (
        "publish_confirm_stage",
        "¿Añadimos ahora todos los cambios listados?",
    ),
    (
        "publish_stage_skip_hint",
        "No se añadió nada. Usa `skron stage <archivo>` y vuelve a ejecutar `skron publish`.",
    ),
    (
        "publish_everything_staged",
        "Todos los cambios ya están preparados para publicar.",
    ),
    ("publish_branch_current", "Trabajando en la rama {branch}."),
    (
        "publish_branch_use_current",
        "¿Quieres seguir en esta rama?",
    ),
    ("publish_branch_new_prompt", "Nombre para la nueva rama:"),
    ("publish_branch_switched", "Cambiado a la rama {branch}."),
    (
        "publish_push_now",
        "¿Compartir esta actualización con el equipo ahora?",
    ),
    (
        "publish_review_prompt",
        "¿Quieres solicitar una revisión en cuanto se suba?",
    ),
    (
        "publish_review_manual_hint",
        "Comparte la actualización con los revisores en el chat mientras terminamos la automatización.",
    ),
    ("stage_path_staged", "Se agregó {path}"),
    (
        "stage_reject_git_dir",
        "No se pueden añadir archivos dentro de .git ({path})",
    ),
    (
        "unlock_repo_banner",
        "Repositorio {repo_id} desbloqueado para {email} (org {org_id}). Remoto {remote}.",
    ),
    (
        "merge_fast_forward",
        "Avance rápido de la rama actual a {branch} ({commit}).",
    ),
    (
        "merge_up_to_date",
        "Ya está todo al día con la rama objetivo.",
    ),
    (
        "merge_commit_created",
        "Se registró el commit de fusión {commit}.",
    ),
    (
        "tag_created",
        "La etiqueta firmada {tag} ({id}) ahora apunta al commit {commit}.",
    ),
    ("changes_summary_heading", "Resumen de cambios:"),
    (
        "changes_ready_heading",
        "Ya preparados (listos para publicar)",
    ),
    ("changes_pending_heading", "Cambios pendientes de añadir"),
    ("changes_modified_label", "Archivos modificados"),
    ("changes_new_label", "Archivos nuevos"),
    ("changes_deleted_label", "Archivos eliminados"),
    (
        "preview_intro",
        "Solo vista previa: no se añadirá ni confirmará nada.",
    ),
    (
        "preview_followup_hint",
        "Cuando esté listo, ejecuta `skron publish`.",
    ),
    (
        "sync_checking",
        "Comprobando el repositorio remoto en busca de cambios...",
    ),
    ("sync_up_to_date", "Todo está al día."),
    ("sync_new_commits_heading", "Novedades en la rama {branch}:"),
    ("sync_new_commit_entry", "{id} — {message}"),
    ("sync_branch_updates_heading", "Resumen de ramas:"),
    (
        "sync_branch_new_entry",
        "• Se creó la rama {branch} en {id}: {message}",
    ),
    (
        "sync_branch_update_entry",
        "• Se actualizó la rama {branch} a {id}: {message}",
    ),
    ("sync_commit_message_fallback", "(sin descripción)"),
    (
        "sync_branch_removed_heading",
        "Ramas eliminadas en el remoto:",
    ),
    ("sync_branch_removed_entry", "• {branch}"),
    (
        "reflog_heading",
        "Actualizaciones recientes de referencias:",
    ),
    ("reflog_empty", "No hay movimientos de HEAD registrados."),
    ("reflog_none", "(ninguno)"),
    (
        "reflog_entry",
        "{timestamp} {reference}: {old} -> {new} - {message}",
    ),
];
