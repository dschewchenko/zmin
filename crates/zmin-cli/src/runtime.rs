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

pub(crate) use zmin_cli_runtime::{
    PhaseTrace, phase_trace, phase_trace_emit, phase_trace_enabled, remove_file_if_exists,
    remove_path_if_exists, unique_temp_sibling, write_content_addressed_file,
};

#[path = "runtime/object.rs"]
mod object_primitives;
pub(crate) use object_primitives::*;

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
    LooseObjectStore, MergeFileLabels, ObjectId, PackEncodeOptions, PackIndexEntry,
    PackIndexVersion, PackRefsOptions, PackedObjectStore, RefStore, RefTarget, ResolveUndoStage,
    Signature, TagBuilder, TreeEntry, TreeMode, TreeObjectCache, apply_eol_clean_to_lf,
    apply_eol_smudge_to_crlf, apply_ident_clean, check_ref_format, checkout_index,
    checkout_index_fresh, checkout_index_fresh_into_metadata, checkout_index_fresh_with_metadata,
    collect_reachable_objects_from_roots as collect_reachable_object_ids_from_roots, decode_commit,
    decode_pack_index, decode_pack_index_from_path, decode_pack_index_object_ids,
    decode_pack_index_object_ids_from_path, decode_tag, diff_indexes,
    diff_indexes_with_exact_renames, diff_indexes_with_exact_renames_and_copies,
    encode_loose_object, encode_pack_from_store_with_options, encode_tree, find_tree_entry,
    for_each_pack_index_entry_from_path, for_each_pack_index_object_id_from_path, hash_object,
    index_pack_bytes, index_pack_bytes_with_store, index_pack_bytes_with_version, index_pack_file,
    index_pack_file_with_version, init_repository, merge_file as merge_file_core,
    pack_index_object_count, read_index, read_tree_to_index_uncached,
    repair_thin_pack_file_to_path, unpack_pack_to_loose, validate_pack_reverse_index,
    write_tree_from_index, write_undeltified_pack_from_store,
};
