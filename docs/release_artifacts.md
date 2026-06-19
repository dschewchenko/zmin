# Release Artifacts

The release workflow builds native preview archives for:

- Linux x86_64
- Linux ARM64
- macOS Intel
- macOS Apple Silicon
- Windows x86_64
- Windows ARM64

## Publish

The normal preview path is a tag push. Preview tags include UTC time so more
than one preview can be published on the same day:

```bash
tag="v0.0.1-preview.$(date -u +%Y%m%dT%H%M%SZ)"
git tag -f "$tag"
git push --force origin "$tag"
```

The workflow creates or updates the matching GitHub Release and uploads
`SHA256SUMS`. It also supports manual `workflow_dispatch` from the GitHub UI for
an existing tag.

## Verify

```bash
tools/check-release-artifacts.sh
```

Without an argument, the verifier reads `Current preview` from `README.md`. It
uses direct release asset URLs, so it does not require the local GitHub CLI. It
prints GitHub Release `download_count` values when the REST API is available;
set `GITHUB_TOKEN` or `GH_TOKEN` if unauthenticated API calls are rate limited.
Zmin binaries do not send install telemetry.
