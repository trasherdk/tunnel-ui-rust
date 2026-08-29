# Release flow

## New version

From a clean `main` (or the branch you want to tag):

```bash
./scripts/release.sh patch    # 0.1.0 -> 0.1.1
./scripts/release.sh minor    # 0.1.1 -> 0.2.0
./scripts/release.sh major    # 0.2.0 -> 1.0.0
```

The script bumps `[package].version` in `Cargo.toml`, the matching entry in `Cargo.lock`, and the Inno Setup default version, then commits, creates an annotated `vX.Y.Z` tag, and pushes the commit and tag. GitHub Actions builds Linux/Windows artifacts and publishes the GitHub Release.

```bash
./scripts/release.sh patch --dry-run   # print next version only
./scripts/release.sh patch --no-push   # commit + tag locally, do not push
```

## Same version again (rebuild artifacts)

```bash
git checkout main
git pull
git tag -d v0.1.0
git tag -a v0.1.0 -m "Release v0.1.0"
git push origin :refs/tags/v0.1.0
git push origin v0.1.0
```
