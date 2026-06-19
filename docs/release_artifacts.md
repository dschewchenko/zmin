# Release Artifacts

The release workflow builds native preview archives for:

- Linux x86_64
- Linux ARM64
- macOS Intel
- macOS Apple Silicon
- Windows x86_64
- Windows ARM64

## Publish

The normal preview path is a tag push. The tag must point at the commit that
should own the release assets:

```bash
git tag -f v0.0.1-preview.20260619
git push --force origin v0.0.1-preview.20260619
```

The workflow creates or updates the matching GitHub Release and uploads
`SHA256SUMS`. It also supports manual `workflow_dispatch` from the GitHub UI for
an existing tag.

## Verify

```bash
tools/check-release-artifacts.sh v0.0.1-preview.20260619
```

The verifier uses direct release asset URLs, so it does not require the local
GitHub CLI. It prints GitHub Release `download_count` values when the REST API is
available; set `GITHUB_TOKEN` or `GH_TOKEN` if unauthenticated API calls are rate
limited. Zmin binaries do not send install telemetry.
