# Release Artifacts

The release workflow builds native preview archives for:

- Linux x86_64
- Linux ARM64
- macOS Intel
- macOS Apple Silicon
- Windows x86_64
- Windows ARM64

## Publish

Requires permission to dispatch GitHub Actions workflows:

```bash
gh workflow run release-artifacts.yml -f tag=v0.0.1-preview.20260619
```

The tag must already exist. The workflow creates or updates the matching GitHub
Release and uploads `SHA256SUMS`.

## Verify

```bash
tools/check-release-artifacts.sh v0.0.1-preview.20260619
```

The verifier fails if any expected asset is missing. It also prints GitHub
Release `download_count` values for the assets. Zmin binaries do not send
install telemetry.
