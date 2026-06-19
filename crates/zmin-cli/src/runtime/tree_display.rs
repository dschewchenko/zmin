use zmin_git_core::{GitObjectKind, TreeMode};

pub(crate) fn tree_entry_path(prefix: &[u8], name: &[u8]) -> Vec<u8> {
    if prefix.is_empty() {
        name.to_vec()
    } else {
        let mut path = Vec::with_capacity(prefix.len() + 1 + name.len());
        path.extend_from_slice(prefix);
        path.push(b'/');
        path.extend_from_slice(name);
        path
    }
}

pub(crate) fn tree_entry_kind(mode: TreeMode) -> GitObjectKind {
    match mode {
        TreeMode::File | TreeMode::Executable | TreeMode::Symlink => GitObjectKind::Blob,
        TreeMode::Tree => GitObjectKind::Tree,
        TreeMode::Gitlink => GitObjectKind::Commit,
    }
}

pub(crate) fn tree_mode_display(mode: TreeMode) -> &'static str {
    match mode {
        TreeMode::File => "100644",
        TreeMode::Executable => "100755",
        TreeMode::Symlink => "120000",
        TreeMode::Tree => "040000",
        TreeMode::Gitlink => "160000",
    }
}
