# Release flow


## When updating a existing release - Same version as last push

```bash

# local
git checkout main
git pull
git tag -d v0.1.0
git tag -a v0.1.0 -m "Release v0.1.0"

# remote (delete, then push)
git push origin :refs/tags/v0.1.0
git push origin v0.1.0

```