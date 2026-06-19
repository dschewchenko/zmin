//! From-scratch Git-compatible core primitives for Zmin.
//!
//! This crate does not depend on the existing encrypted `zmin` prototype.
//! It is the canonical base for public/private Git compatibility and the future
//! secret Git adapter.

pub mod attributes;
pub mod checkout;
pub mod commit;
pub mod diff;
pub mod ignore;
pub mod index;
pub mod init;
pub mod loose;
pub mod merge_file;
pub mod object;
pub mod object_store;
pub mod pack;
pub mod reachable;
pub mod refs;
pub mod tag;
pub mod tree;

pub use attributes::{
    AttributeValue, GitAttributes, apply_eol_clean_to_lf, apply_eol_smudge_to_crlf,
    apply_ident_clean, apply_ident_smudge,
};
pub use checkout::{
    CheckoutIndexOptions, checkout_index, checkout_index_fresh, checkout_index_fresh_into_metadata,
    checkout_index_fresh_with_metadata,
};
pub use commit::{
    CommitBuilder, CommitLinks, CommitObject, CommitObjectCache, Signature, decode_commit,
    decode_commit_links, encode_commit,
};
pub use diff::{
    IndexDiffEntry, IndexDiffStatus, TreeDiffEntry, TreeDiffFileEntry, diff_index_to_tree,
    diff_indexes, diff_indexes_with_exact_renames, diff_indexes_with_exact_renames_and_copies,
    diff_trees, for_each_tree_diff,
};
pub use ignore::GitIgnore;
pub use index::{
    GitIndex, IndexEntry, IndexMode, ResolveUndoEntry, ResolveUndoStage, read_index, write_index,
};
pub use init::{InitRepositoryOptions, InitRepositoryResult, init_repository};
pub use loose::{LooseObject, LooseObjectStore, encode_loose_object};
pub use merge_file::{MergeFileLabels, MergeFileResult, merge_file};
pub use object::{
    GitHashAlgorithm, GitObjectHash, GitObjectKind, GitObjectWriter, ObjectId,
    decode_object_header, encoded_object_len, hash_object, write_encoded_object,
};
pub use object_store::{GitObjectSink, GitObjectStore, InMemoryObjectStore};
pub use pack::{
    IndexedPack, IndexedPackIndexOnly, PackEncodeOptions, PackIndexEntry, PackIndexVersion,
    PackObjectData, PackVerifyEntry, PackedObjectStore, ThinPackFileRepair, ThinPackRepair,
    UnpackPackStats, VerifiedPack, decode_pack_index, decode_pack_index_from_path,
    decode_pack_index_object_ids, decode_pack_index_object_ids_from_path, decode_pack_objects,
    decode_pack_objects_with_store, encode_pack_from_store, encode_pack_from_store_with_options,
    for_each_pack_index_entry, for_each_pack_index_entry_from_path,
    for_each_pack_index_object_id_from_path, for_each_pack_object, for_each_pack_object_file,
    index_pack_bytes, index_pack_bytes_index_only, index_pack_bytes_index_only_with_version,
    index_pack_bytes_with_store, index_pack_bytes_with_store_and_version,
    index_pack_bytes_with_version, index_pack_file, index_pack_file_index_only,
    index_pack_file_index_only_with_version, index_pack_file_with_store,
    index_pack_file_with_store_and_version, index_pack_file_with_version, pack_index_object_count,
    pack_index_object_ids_all_from_path, pack_index_object_ids_are_subset_from_paths,
    repair_thin_pack_bytes, repair_thin_pack_file_to_path, unpack_pack_file_to_loose,
    unpack_pack_to_loose, validate_pack_index_bytes, validate_pack_index_file,
    validate_pack_reverse_index, validate_pack_reverse_index_file, verify_pack_bytes_with_version,
    verify_pack_file, verify_pack_file_matches_index, verify_pack_file_matches_index_with_version,
    verify_pack_file_with_version, write_pack_from_store_with_options,
    write_undeltified_pack_from_store,
};
pub use reachable::collect_reachable_objects_from_roots;
pub use refs::{PackRefsOptions, RefStore, RefTarget, ServerInfoRef, check_ref_format};
pub use tag::{TagBuilder, TagObject, decode_tag, encode_tag};
pub use tree::{
    TreeEntry, TreeMode, TreeObjectCache, TreeObjectRef, decode_tree, decode_tree_object_refs,
    encode_tree, find_tree_entry, for_each_tree_object_ref, read_tree, read_tree_to_index,
    read_tree_to_index_uncached, write_tree_from_index,
};
