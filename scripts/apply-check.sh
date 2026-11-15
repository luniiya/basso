#!/usr/bin/env bash
# Whether basso's bundle applies on a Horizon commit, as the registry's "newest Nightly" check applies it.
#
#   scripts/apply-check.sh <horizon-git-repo> <commit> [horizon-mixins]
#
# Exports <commit> from the Horizon repo (without its symlinks, which horizon-mixins refuses), rewrites the
# bundle's pin to that commit (as the launcher and the registry check do), and runs `horizon-mixins apply`.
# horizon-mixins defaults to the newest one the launcher has built. Prints its report; exits non-zero when an
# injection doesn't apply. It does not compile the game: run `cargo check -p horizon-client` in the printed
# stage folder for that.
set -euo pipefail

repo=${1:?usage: apply-check.sh <horizon-git-repo> <commit> [horizon-mixins]}
commit=$(git -C "$repo" rev-parse "${2:?usage: apply-check.sh <horizon-git-repo> <commit> [horizon-mixins]}^{commit}")
tool=${3:-$(ls -t "$HOME"/.local/share/horizon-launcher/native/tools/*/bin/horizon-mixins | head -1)}
root=$(cd "$(dirname "$0")/.." && pwd)

work=$(mktemp -d "${TMPDIR:-/tmp}/basso-apply.XXXXXX")
mkdir -p "$work/horizon" "$work/mods/basso"
git -C "$repo" archive "$commit" | tar -x -C "$work/horizon"
find "$work/horizon" -type l -delete
printf '%s' "$commit" > "$work/horizon/.horizon-revision"

cp -r "$root/horizon-mixin.toml" "$root/mixins" "$root/basso-engine" "$work/mods/basso/"
rm -rf "$work/mods/basso/basso-engine/target"
sed -i -E "s/horizon_revision\\s*=\\s*\"[0-9a-f]{40}\"/horizon_revision = \"$commit\"/" \
    "$work/mods/basso/horizon-mixin.toml"

echo "horizon-mixins: $tool"
echo "Horizon: $commit"
"$tool" apply --workspace "$work/horizon" --mods "$work/mods" --out "$work/stage"
echo "stage: $work/stage"
