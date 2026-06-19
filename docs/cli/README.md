# CLI Notes

`crates/zmin-cli` is the user-facing Git-compatible command surface in this repository.

The important references are:

- [compatibility acceptance](compatibility_acceptance.md)
- [command compatibility audit](command_compatibility_audit.md)
- [performance benchmark](performance_benchmark_2026-05-18.md)

Implementation boundaries:

- `src/main.rs` is the thin entrypoint.
- `src/cli/schema.rs` defines the public command tree.
- `src/compat/mod.rs` derives compatibility reports from the same command graph.
- `src/runtime.rs` and `src/runtime/` hold the reusable command implementation.
