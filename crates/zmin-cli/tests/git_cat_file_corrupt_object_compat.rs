mod common;

use std::{fs, io::Write};

use flate2::{Compression, write::ZlibEncoder};
use zmin_git_core::{GitHashAlgorithm, GitObjectHash};

use common::{command_any_output, configure_identity, git, git_init, zmin_bin};

fn write_loose_object(repo: &std::path::Path, raw: &[u8]) -> String {
    let mut hasher = GitObjectHash::new(GitHashAlgorithm::Sha1);
    hasher.update(raw);
    let oid = hasher.finalize().to_hex();
    let object_dir = repo.join(".git/objects").join(&oid[..2]);
    fs::create_dir_all(&object_dir).expect("create object dir");
    let object_path = object_dir.join(&oid[2..]);
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(raw).expect("write object payload");
    fs::write(object_path, encoder.finish().expect("finish zlib")).expect("write loose object");
    oid
}

#[test]
fn cat_file_rejects_unsupported_loose_object_type_like_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());
    fs::write(repo.path().join("anchor.txt"), b"anchor\n").expect("write anchor");
    git(repo.path(), ["add", "-A"]);
    git(repo.path(), ["commit", "-m", "anchor"]);

    let oid = write_loose_object(repo.path(), b"badtype 0\0");
    let args = ["cat-file", "-t", oid.as_str()];
    let stock = command_any_output("git", repo.path(), &args, "git cat-file corrupt type");
    let zmin = command_any_output(zmin_bin(), repo.path(), &args, "zmin cat-file corrupt type");

    assert_eq!(zmin.0, stock.0, "exit code");
    assert_eq!(zmin.1, stock.1, "stdout");
    assert_eq!(zmin.2, stock.2, "stderr");
    assert_ne!(stock.0, 0, "stock Git must reject the corrupt object type");
}
