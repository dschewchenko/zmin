use std::fs;
use std::path::{Path, PathBuf};

fn collect_rust_files(root: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(root).expect("read dir") {
        let entry = entry.expect("dir entry");
        let path = entry.path();
        if path.is_dir() {
            collect_rust_files(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
}

fn occurrence_lines(text: &str, needle: &str) -> Vec<usize> {
    text.lines()
        .enumerate()
        .filter_map(|(index, line)| line.contains(needle).then_some(index + 1))
        .collect()
}

fn first_test_module_line(text: &str) -> Option<usize> {
    text.lines().enumerate().find_map(|(index, line)| {
        let trimmed = line.trim();
        (trimmed == "mod tests {" || trimmed == "#[cfg(test)] mod tests {").then_some(index + 1)
    })
}

#[test]
fn stock_git_runtime_dependencies_remain_test_only_in_src() {
    let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root");

    let allowed_test_only_files = [
        workspace_root.join("crates/zmin-cli/src/runtime/diff_render.rs"),
        workspace_root.join("crates/zmin-cli/src/cli/commands/maintenance_impl.rs"),
        workspace_root.join("crates/zmin-git-core/src/checkout.rs"),
        workspace_root.join("crates/zmin-git-core/src/commit.rs"),
        workspace_root.join("crates/zmin-git-core/src/diff.rs"),
        workspace_root.join("crates/zmin-git-core/src/index.rs"),
        workspace_root.join("crates/zmin-git-core/src/init.rs"),
        workspace_root.join("crates/zmin-git-core/src/loose.rs"),
        workspace_root.join("crates/zmin-git-core/src/object.rs"),
        workspace_root.join("crates/zmin-git-core/src/pack.rs"),
        workspace_root.join("crates/zmin-git-core/src/refs.rs"),
        workspace_root.join("crates/zmin-git-core/src/tree.rs"),
    ];
    let allowed_cfg_test_module_files = [
        workspace_root.join("crates/zmin-cli/src/lib.rs"),
        workspace_root.join("crates/zmin-git-core/src/lib.rs"),
    ];

    let mut files = Vec::new();
    collect_rust_files(&workspace_root.join("crates/zmin-cli/src"), &mut files);
    collect_rust_files(&workspace_root.join("crates/zmin-git-core/src"), &mut files);
    files.sort();

    let audited_needles = [
        r#"Command::new("git")"#,
        r#"Command::new("/usr/bin/git")"#,
        r#"Command::new("/bin/git")"#,
        "stock_git_bin()",
        "stock_git_support::",
        "ZMIN_STOCK_GIT",
        r#"#[path = "../tests/support/stock_git.rs"]"#,
        "pub mod stock_git_support;",
    ];

    let mut unexpected = Vec::new();
    for path in files {
        let text = fs::read_to_string(&path).expect("read source");
        let hits = audited_needles
            .iter()
            .flat_map(|needle| {
                occurrence_lines(&text, needle)
                    .into_iter()
                    .map(move |line| (needle, line))
            })
            .collect::<Vec<_>>();
        if hits.is_empty() {
            continue;
        }

        if allowed_cfg_test_module_files
            .iter()
            .any(|allowed| allowed == &path)
        {
            for (needle, line) in hits {
                if *needle != r#"#[path = "../tests/support/stock_git.rs"]"#
                    && *needle != "pub mod stock_git_support;"
                {
                    unexpected.push(format!(
                        "{}:{}@{}",
                        path.strip_prefix(workspace_root)
                            .expect("workspace-relative path")
                            .display(),
                        needle,
                        line
                    ));
                }
            }
            continue;
        }

        if !allowed_test_only_files
            .iter()
            .any(|allowed| allowed == &path)
        {
            let rendered_hits = hits
                .iter()
                .map(|(needle, line)| format!("{needle}@{line}"))
                .collect::<Vec<_>>()
                .join(",");
            unexpected.push(format!(
                "{}:{}",
                path.strip_prefix(workspace_root)
                    .expect("workspace-relative path")
                    .display(),
                rendered_hits
            ));
            continue;
        }

        let first_test_module_line = first_test_module_line(&text).expect("test module line");

        for (needle, line) in hits {
            if line <= first_test_module_line {
                unexpected.push(format!(
                    "{}:{}@{}",
                    path.strip_prefix(workspace_root)
                        .expect("workspace-relative path")
                        .display(),
                    needle,
                    line
                ));
            }
        }
    }

    assert!(
        unexpected.is_empty(),
        "stock Git dependency patterns escaped test-only src zones:\n{}",
        unexpected.join("\n")
    );
}
