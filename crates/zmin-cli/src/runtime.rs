#![allow(unused_imports)]

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};
use std::fs;
use std::io::{self, BufRead, IsTerminal, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command as ProcessCommand, Stdio};
use std::sync::Arc;

#[path = "runtime/cli_support.rs"]
mod cli_support_primitives;
pub(crate) use cli_support_primitives::*;
#[path = "runtime/types.rs"]
mod type_primitives;
pub(crate) use type_primitives::*;
#[path = "runtime/repo.rs"]
mod repo_primitives;
pub(crate) use repo_primitives::*;

#[path = "runtime/config.rs"]
mod config_primitives;
pub(crate) use config_primitives::*;

#[path = "runtime/env.rs"]
mod env_primitives;
pub(crate) use env_primitives::*;

#[path = "runtime/local_time.rs"]
mod local_time_primitives;
pub(crate) use local_time_primitives::*;

#[path = "runtime/trace2.rs"]
mod trace2_primitives;
pub(crate) use trace2_primitives::*;

#[path = "runtime/abbrev.rs"]
mod abbrev_primitives;
pub(crate) use abbrev_primitives::*;

pub(crate) use zmin_cli_runtime::{
    PhaseTrace, phase_trace, phase_trace_emit, phase_trace_enabled, remove_file_if_exists,
    remove_path_if_exists, unique_temp_sibling, write_content_addressed_file,
};

#[path = "runtime/object.rs"]
mod object_primitives;
pub(crate) use object_primitives::*;

#[path = "runtime/mktree.rs"]
mod mktree_primitives;
pub(crate) use mktree_primitives::*;

#[path = "runtime/object_format.rs"]
mod object_format_primitives;
pub(crate) use object_format_primitives::*;

#[path = "runtime/pathspec.rs"]
mod pathspec_primitives;
pub(crate) use pathspec_primitives::*;

#[path = "runtime/refs.rs"]
mod ref_primitives;
pub(crate) use ref_primitives::*;

#[path = "runtime/graph.rs"]
mod graph_primitives;
pub(crate) use graph_primitives::*;

#[path = "runtime/tree_display.rs"]
mod tree_display_primitives;
pub(crate) use tree_display_primitives::*;

#[path = "runtime/commit_meta.rs"]
mod commit_meta_primitives;
pub(crate) use commit_meta_primitives::*;

#[path = "runtime/index.rs"]
mod index_primitives;
pub(crate) use index_primitives::*;

#[path = "runtime/index_stat.rs"]
mod index_stat_primitives;
pub(crate) use index_stat_primitives::*;

#[path = "runtime/worktree_index.rs"]
mod worktree_index_primitives;
pub(crate) use worktree_index_primitives::*;

#[path = "runtime/worktree_files.rs"]
mod worktree_files_primitives;
pub(crate) use worktree_files_primitives::*;

#[path = "runtime/clone_service.rs"]
mod clone_service_primitives;
pub(crate) use clone_service_primitives::*;

#[path = "runtime/pack_index.rs"]
mod pack_index_primitives;
pub(crate) use pack_index_primitives::*;

#[path = "runtime/commit_graph.rs"]
mod commit_graph_primitives;
pub(crate) use commit_graph_primitives::*;

#[path = "runtime/merge_worktree.rs"]
mod merge_worktree_primitives;
pub(crate) use merge_worktree_primitives::*;

#[path = "runtime/diff_render.rs"]
mod diff_render_primitives;
pub(crate) use diff_render_primitives::*;

#[path = "runtime/patch_id.rs"]
mod patch_id_primitives;
pub(crate) use patch_id_primitives::*;

#[path = "runtime/submodule.rs"]
mod submodule_primitives;
pub(crate) use submodule_primitives::*;

#[path = "runtime/transport_local.rs"]
mod transport_local_primitives;
pub(crate) use transport_local_primitives::*;

#[path = "runtime/partial_clone_filter.rs"]
mod partial_clone_filter_primitives;
pub(crate) use partial_clone_filter_primitives::*;

#[path = "runtime/pack_operation_lock.rs"]
mod pack_operation_lock;
pub(crate) use pack_operation_lock::*;

#[path = "runtime/bundle_uri.rs"]
mod bundle_uri_primitives;
pub(crate) use bundle_uri_primitives::*;

#[path = "runtime/lfs_config.rs"]
mod lfs_config;
pub(crate) use lfs_config::*;

#[path = "runtime/lfs_url_config.rs"]
mod lfs_url_config;
pub(crate) use lfs_url_config::*;

#[path = "runtime/lfs_http_policy.rs"]
mod lfs_http_policy;
pub(crate) use lfs_http_policy::*;

#[path = "runtime/lfs_pointer.rs"]
mod lfs_pointer;
pub(crate) use lfs_pointer::*;

#[path = "runtime/lfs_store.rs"]
mod lfs_store;
pub(crate) use lfs_store::*;

#[path = "runtime/lfs_endpoint.rs"]
mod lfs_endpoint;
pub(crate) use lfs_endpoint::*;

#[path = "runtime/lfs_auth.rs"]
mod lfs_auth;
pub(crate) use lfs_auth::*;

#[path = "runtime/lfs_filter_process.rs"]
mod lfs_filter_process;
pub(crate) use lfs_filter_process::*;

#[path = "runtime/lfs_batch.rs"]
mod lfs_batch;
pub(crate) use lfs_batch::*;

#[path = "runtime/lfs_transfer.rs"]
mod lfs_transfer;
pub(crate) use lfs_transfer::*;

#[path = "runtime/lfs_runtime_adapters.rs"]
mod lfs_runtime_adapters;
pub(crate) use lfs_runtime_adapters::*;

#[path = "runtime/lfs_network_session.rs"]
mod lfs_network_session;
pub(crate) use lfs_network_session::*;

#[path = "runtime/lfs_reachability.rs"]
mod lfs_reachability;
pub(crate) use lfs_reachability::*;

pub(crate) fn resolve_repack_pack_kept_objects(
    command_line_enabled: bool,
    config_entry: Option<&ConfigEntry>,
) -> Result<bool> {
    if command_line_enabled {
        return Ok(true);
    }
    let Some(config_entry) = config_entry else {
        return Ok(false);
    };
    config_entry.bool_value().ok_or_else(|| CliError::Stderr {
        code: 128,
        text: format!(
            "fatal: bad boolean config value '{}' for 'repack.packKeptObjects'\n",
            config_entry.value
        ),
    })
}

#[path = "runtime/primitive_adapters.rs"]
mod primitive_adapters;
pub(crate) use primitive_adapters::*;

#[path = "runtime/primitive_runtime.rs"]
mod primitive_runtime;
pub(crate) use primitive_runtime::*;

pub(crate) use crate::cli::schema::*;
use flate2::{
    Compression,
    read::ZlibDecoder,
    write::{GzEncoder, ZlibEncoder},
};
use regex::bytes::Regex;
use zmin_git_core::{
    AttributeValue, CheckoutIndexOptions, CommitBuilder, CommitObject, CommitObjectCache,
    GitAttributes, GitHashAlgorithm, GitIgnore, GitIndex, GitObjectHash, GitObjectKind,
    IndexDiffEntry, IndexDiffStatus, IndexEntry, IndexMode, InitRepositoryOptions, LooseObject,
    LooseObjectStore, MergeFileLabels, ObjectId, PackBlobSource, PackEncodeOptions, PackIndexEntry,
    PackIndexVersion, PackRefsOptions, PackedObjectStore, RefStore, RefTarget, ResolveUndoStage,
    Signature, TagBuilder, TreeEntry, TreeMode, TreeObjectCache, apply_eol_clean_to_lf,
    apply_eol_smudge_to_crlf, apply_ident_clean, check_ref_format, checkout_index,
    checkout_index_fresh, checkout_index_fresh_into_metadata, checkout_index_fresh_with_metadata,
    collect_reachable_objects_from_roots as collect_reachable_object_ids_from_roots, decode_commit,
    decode_pack_index, decode_pack_index_from_path, decode_pack_index_object_ids,
    decode_pack_index_object_ids_from_path, decode_tag, decode_tree, diff_indexes,
    diff_indexes_with_exact_renames, diff_indexes_with_exact_renames_and_copies,
    encode_loose_object, encode_pack_from_store_with_options, encode_tree, find_tree_entry,
    for_each_pack_index_entry_from_path, for_each_pack_index_object_id_from_path, hash_object,
    index_pack_bytes, index_pack_bytes_with_store, index_pack_bytes_with_version, index_pack_file,
    index_pack_file_with_version, init_repository, merge_file as merge_file_core,
    pack_index_object_count, read_index, read_index_with_algorithm, read_tree_to_index_uncached,
    repair_thin_pack_file_to_path, unpack_pack_to_loose, validate_pack_reverse_index,
    write_pack_from_store_with_options, write_single_undeltified_blob_pack_with_options,
    write_tree_from_index, write_undeltified_blob_pack_with_options,
    write_undeltified_pack_from_store,
};
