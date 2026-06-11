#![allow(unused_imports)]

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};
use std::fs;
use std::io::{self, BufRead, IsTerminal, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::process::{Command as ProcessCommand, Stdio};
use std::sync::Arc;

use flate2::{
    Compression,
    read::ZlibDecoder,
    write::{GzEncoder, ZlibEncoder},
};
use regex::bytes::Regex;
use skron_git_core::{
    AttributeValue, CheckoutIndexOptions, CommitBuilder, CommitObject, CommitObjectCache,
    GitAttributes, GitHashAlgorithm, GitIgnore, GitIndex, GitObjectHash, GitObjectKind,
    GitObjectStore, IndexDiffEntry, IndexDiffStatus, IndexEntry, IndexMode, InitRepositoryOptions,
    LooseObject, LooseObjectStore, MergeFileLabels, ObjectId, PackEncodeOptions, PackIndexEntry,
    PackIndexVersion, PackRefsOptions, PackedObjectStore, RefStore, RefTarget, ResolveUndoStage,
    Signature, TagBuilder, TreeEntry, TreeMode, TreeObjectCache, check_ref_format, checkout_index,
    checkout_index_fresh, checkout_index_fresh_into_metadata, checkout_index_fresh_with_metadata,
    collect_reachable_objects_from_roots as collect_reachable_object_ids_from_roots, decode_commit,
    decode_pack_index, decode_pack_index_from_path, decode_pack_index_object_ids,
    decode_pack_index_object_ids_from_path, decode_tag, diff_indexes,
    diff_indexes_with_exact_renames, diff_indexes_with_exact_renames_and_copies,
    encode_loose_object, encode_pack_from_store_with_options, encode_tree, find_tree_entry,
    for_each_pack_index_entry, for_each_pack_index_entry_from_path,
    for_each_pack_index_object_id_from_path, for_each_pack_object_file, hash_object,
    index_pack_bytes, index_pack_bytes_with_store, index_pack_bytes_with_version, index_pack_file,
    index_pack_file_index_only, index_pack_file_index_only_with_version,
    index_pack_file_with_store, index_pack_file_with_version, init_repository,
    merge_file as merge_file_core, pack_index_object_count, pack_index_object_ids_all_from_path,
    pack_index_object_ids_are_subset_from_paths, read_index, repair_thin_pack_file_to_path,
    unpack_pack_file_to_loose, unpack_pack_to_loose, validate_pack_index_file,
    validate_pack_reverse_index_file, verify_pack_file, write_pack_from_store_with_options,
    write_tree_from_index, write_undeltified_pack_from_store,
};

pub(crate) mod admin;
pub(crate) mod archive;
pub(crate) mod commit;
pub(crate) mod config;
pub(crate) mod core;
pub(crate) mod credential;
pub(crate) mod diff;
pub(crate) mod grep;
pub(crate) mod history;
pub(crate) mod import;
pub(crate) mod mail;
pub(crate) mod maintenance;
pub(crate) mod merge;
pub(crate) mod notes;
pub(crate) mod pack;
pub(crate) mod patch;
pub(crate) mod reference;
pub(crate) mod sequencer;
pub(crate) mod text;
pub(crate) mod transport;
pub(crate) mod worktree;

#[path = "admin_impl.rs"]
pub(crate) mod admin_commands;
#[path = "archive_impl.rs"]
pub(crate) mod archive_commands;
#[path = "commit_impl.rs"]
pub(crate) mod commit_commands;
#[path = "config_impl.rs"]
pub(crate) mod config_commands;
#[path = "core_impl.rs"]
pub(crate) mod core_commands;
#[path = "credential_impl.rs"]
pub(crate) mod credential_commands;
#[path = "diff_impl.rs"]
pub(crate) mod diff_commands;
#[path = "grep_impl.rs"]
pub(crate) mod grep_commands;
#[path = "history_impl.rs"]
pub(crate) mod history_commands;
#[path = "import_impl.rs"]
pub(crate) mod import_commands;
#[path = "mail_impl.rs"]
pub(crate) mod mail_commands;
#[path = "maintenance_impl.rs"]
pub(crate) mod maintenance_commands;
#[path = "merge_impl.rs"]
pub(crate) mod merge_commands;
#[path = "notes_impl.rs"]
pub(crate) mod notes_commands;
#[path = "pack_impl.rs"]
pub(crate) mod pack_commands;
#[path = "patch_impl.rs"]
pub(crate) mod patch_commands;
#[path = "reference_impl.rs"]
pub(crate) mod reference_commands;
#[path = "scalar_impl.rs"]
pub(crate) mod scalar_commands;
#[path = "sequencer_impl.rs"]
pub(crate) mod sequencer_commands;
#[path = "text_impl.rs"]
pub(crate) mod text_commands;
#[path = "transport_impl.rs"]
pub(crate) mod transport_commands;
#[path = "worktree_impl.rs"]
pub(crate) mod worktree_commands;

pub(crate) use crate::runtime::*;
use commit_commands::{
    commit_tree_message, compare_mktree_entries, is_commit_message_line_blank, parse_mktree_entry,
    split_mktree_records, strip_commit_message_line_whitespace,
};
use core_commands::{
    collect_loose_object_stats, collect_pack_object_stats, print_tree_entries, print_tree_entry,
    update_server_info,
};
use history_commands::{blame, parse_reflog_entry, peel_to_commit};
use mail_commands::{interpret_trailers_content, parse_mail_author};
use pack_commands::{pack_encode_options, verify_tag};
use reference_commands::remote_repository_unavailable_error;
use sequencer_commands::apply_tree_delta;

pub(crate) fn dispatch(
    command: crate::runtime::Command,
    raw_args: &[String],
) -> std::result::Result<(), crate::runtime::CliError> {
    match command {
        crate::runtime::Command::Compatibility { profile, format } => {
            crate::compat::run(profile, format)
        }
        command @ (crate::runtime::Command::GetTarCommitId
        | crate::runtime::Command::Archive { .. }
        | crate::runtime::Command::UploadArchive { .. }) => archive::dispatch(command),
        command @ (crate::runtime::Command::Config { .. }
        | crate::runtime::Command::Var { .. }
        | crate::runtime::Command::Version { .. }) => config::dispatch(command),
        command @ (crate::runtime::Command::Commit { .. }
        | crate::runtime::Command::Citool { .. }
        | crate::runtime::Command::Gui { .. }
        | crate::runtime::Command::WriteTree
        | crate::runtime::Command::CommitTree { .. }
        | crate::runtime::Command::Mktree { .. }) => commit::dispatch(command),
        command @ (crate::runtime::Command::Credential { .. }
        | crate::runtime::Command::CredentialStore { .. }
        | crate::runtime::Command::CredentialCache { .. }) => credential::dispatch(command),
        command @ (crate::runtime::Command::Column { .. }
        | crate::runtime::Command::Stripspace { .. }) => text::dispatch(command),
        command @ (crate::runtime::Command::ForEachRepo { .. }
        | crate::runtime::Command::UpdateIndex { .. }
        | crate::runtime::Command::Bugreport { .. }
        | crate::runtime::Command::Diagnose { .. }
        | crate::runtime::Command::Backfill { .. }
        | crate::runtime::Command::Gitk { .. }
        | crate::runtime::Command::Gitweb { .. }
        | crate::runtime::Command::Scalar { .. }
        | crate::runtime::Command::Hook { .. }
        | crate::runtime::Command::ShI18n { .. }
        | crate::runtime::Command::ShSetup { .. }
        | crate::runtime::Command::Cvsserver { .. }
        | crate::runtime::Command::Cvsexportcommit { .. }
        | crate::runtime::Command::Cvsimport { .. }
        | crate::runtime::Command::Archimport { .. }
        | crate::runtime::Command::P4 { .. }
        | crate::runtime::Command::Svn { .. }
        | crate::runtime::Command::Instaweb { .. }) => admin::dispatch(command),
        command @ (crate::runtime::Command::Init { .. }
        | crate::runtime::Command::HashObject { .. }
        | crate::runtime::Command::CatFile { .. }
        | crate::runtime::Command::CountObjects { .. }
        | crate::runtime::Command::UnpackFile { .. }
        | crate::runtime::Command::ShowIndex
        | crate::runtime::Command::UpdateServerInfo { .. }
        | crate::runtime::Command::CheckRefFormat { .. }
        | crate::runtime::Command::CheckIgnore { .. }
        | crate::runtime::Command::CheckMailmap { .. }
        | crate::runtime::Command::CheckAttr { .. }
        | crate::runtime::Command::UnpackObjects { .. }) => core::dispatch(command),
        command @ (crate::runtime::Command::Clone { .. }
        | crate::runtime::Command::LsRemote { .. }
        | crate::runtime::Command::Fetch { .. }
        | crate::runtime::Command::Pull { .. }
        | crate::runtime::Command::Push { .. }
        | crate::runtime::Command::Daemon { .. }
        | crate::runtime::Command::UploadPack { .. }
        | crate::runtime::Command::HttpFetch { .. }
        | crate::runtime::Command::HttpPush { .. }
        | crate::runtime::Command::FetchPack { .. }
        | crate::runtime::Command::SendPack { .. }
        | crate::runtime::Command::HttpBackend
        | crate::runtime::Command::ReceivePack { .. }
        | crate::runtime::Command::Shell { .. }) => transport::dispatch(command, raw_args),
        command @ (crate::runtime::Command::LsFiles { .. }
        | crate::runtime::Command::Add { .. }
        | crate::runtime::Command::Stage { .. }
        | crate::runtime::Command::Rm { .. }
        | crate::runtime::Command::Mv { .. }
        | crate::runtime::Command::Status { .. }
        | crate::runtime::Command::ReadTree { .. }
        | crate::runtime::Command::Checkout { .. }
        | crate::runtime::Command::CheckoutIndex { .. }
        | crate::runtime::Command::Switch { .. }
        | crate::runtime::Command::Restore { .. }
        | crate::runtime::Command::Clean { .. }
        | crate::runtime::Command::Reset { .. }
        | crate::runtime::Command::Stash { .. }
        | crate::runtime::Command::Worktree { .. }
        | crate::runtime::Command::SparseCheckout { .. }
        | crate::runtime::Command::Submodule { .. }) => worktree::dispatch(command),
        command @ (crate::runtime::Command::Diff { .. }
        | crate::runtime::Command::Difftool { .. }
        | crate::runtime::Command::DiffFiles { .. }
        | crate::runtime::Command::DiffIndex { .. }
        | crate::runtime::Command::DiffTree { .. }
        | crate::runtime::Command::DiffPairs { .. }) => diff::dispatch(command),
        command @ (crate::runtime::Command::PackObjects { .. }
        | crate::runtime::Command::Bundle { .. }
        | crate::runtime::Command::IndexPack { .. }
        | crate::runtime::Command::Fsck { .. }
        | crate::runtime::Command::VerifyPack { .. }
        | crate::runtime::Command::PackRedundant { .. }
        | crate::runtime::Command::VerifyCommit { .. }
        | crate::runtime::Command::VerifyTag { .. }
        | crate::runtime::Command::Mktag { .. }
        | crate::runtime::Command::CommitGraph { .. }
        | crate::runtime::Command::MultiPackIndex { .. }) => pack::dispatch(command),
        command @ (crate::runtime::Command::Remote { .. }
        | crate::runtime::Command::PackRefs { .. }
        | crate::runtime::Command::UpdateRef { .. }
        | crate::runtime::Command::SymbolicRef { .. }
        | crate::runtime::Command::Refs { .. }
        | crate::runtime::Command::Repo { .. }
        | crate::runtime::Command::ShowRef { .. }
        | crate::runtime::Command::ForEachRef { .. }
        | crate::runtime::Command::LsTree { .. }
        | crate::runtime::Command::Branch { .. }
        | crate::runtime::Command::Tag { .. }
        | crate::runtime::Command::Replace { .. }
        | crate::runtime::Command::PatchId { .. }
        | crate::runtime::Command::RevParse { .. }) => reference::dispatch(command, raw_args),
        command @ (crate::runtime::Command::Replay { .. }
        | crate::runtime::Command::History { .. }
        | crate::runtime::Command::RangeDiff { .. }
        | crate::runtime::Command::FilterBranch { .. }
        | crate::runtime::Command::Shortlog { .. }
        | crate::runtime::Command::Blame { .. }
        | crate::runtime::Command::Annotate { .. }
        | crate::runtime::Command::ShowBranch { .. }
        | crate::runtime::Command::Cherry { .. }
        | crate::runtime::Command::RequestPull { .. }
        | crate::runtime::Command::Describe { .. }
        | crate::runtime::Command::NameRev { .. }
        | crate::runtime::Command::Reflog { .. }
        | crate::runtime::Command::Log { .. }
        | crate::runtime::Command::Whatchanged { .. }
        | crate::runtime::Command::Show { .. }
        | crate::runtime::Command::RevList { .. }
        | crate::runtime::Command::MergeBase { .. }
        | crate::runtime::Command::LastModified { .. }) => history::dispatch(command),
        command @ crate::runtime::Command::Grep { .. } => grep::dispatch(command),
        command @ (crate::runtime::Command::CherryPick { .. }
        | crate::runtime::Command::Revert { .. }
        | crate::runtime::Command::Bisect { .. }
        | crate::runtime::Command::Rerere { .. }
        | crate::runtime::Command::Rebase { .. }) => sequencer::dispatch(command),
        command @ (crate::runtime::Command::InterpretTrailers { .. }
        | crate::runtime::Command::Mailsplit { .. }
        | crate::runtime::Command::Mailinfo { .. }
        | crate::runtime::Command::FmtMergeMsg { .. }
        | crate::runtime::Command::Am { .. }
        | crate::runtime::Command::FormatPatch { .. }
        | crate::runtime::Command::SendEmail { .. }
        | crate::runtime::Command::ImapSend { .. }) => mail::dispatch(command),
        command @ (crate::runtime::Command::Quiltimport { .. }
        | crate::runtime::Command::FastExport { .. }
        | crate::runtime::Command::FastImport) => import::dispatch(command),
        command @ (crate::runtime::Command::Merge { .. }
        | crate::runtime::Command::Mergetool { .. }
        | crate::runtime::Command::MergeTree { .. }
        | crate::runtime::Command::MergeFile { .. }
        | crate::runtime::Command::MergeOneFile { .. }
        | crate::runtime::Command::MergeIndex { .. }) => merge::dispatch(command),
        command @ (crate::runtime::Command::Maintenance { .. }
        | crate::runtime::Command::PrunePacked { .. }
        | crate::runtime::Command::Repack { .. }
        | crate::runtime::Command::Gc { .. }
        | crate::runtime::Command::Prune { .. }) => maintenance::dispatch(command),
        command @ crate::runtime::Command::Notes { .. } => notes::dispatch(command),
        command @ crate::runtime::Command::Apply { .. } => patch::dispatch(command),
    }
}
