Pinned Certificate Store
========================

The files in this directory define the root certificate that the CLI trusts when talking to skron.dev.

- `pinned_ca.pem` — PEM-encoded public certificate (no private key).
- `pinned_ca.sha3` — SHA3-256 fingerprint (uppercase hex, no separators).

When rotating the authentication CA:
1. Generate the new certificate under `tools/security/` (see `docs/security/tls_pinning.md`).
2. Replace `pinned_ca.pem` and update `pinned_ca.sha3` with the new fingerprint.
3. Commit both files so the fingerprint is versioned with the codebase.

The build embeds these files automatically; no runtime configuration is required.
