#!/usr/bin/env bash
# Fetch the FINOS Common Domain Model (CDM) `.rosetta` corpus for sigil's
# golden conformance suite (issue #10).
#
# Usage: scripts/fetch-cdm.sh [master|legacy [tag]] [--force]
#
#   master   pin at MASTER_SHA below (Phase 1 golden snapshot)
#   legacy   pin at a tag, default LEGACY_TAG below (Phase 2 reverse-drift)
#   --force  refetch even if the corpus is already present
#
# Default cache location: <repo>/target/cdm-corpus/<master|legacy> (gitignored).
# Override with SIGIL_CDM_CACHE=<dir> (the <master|legacy> subdir is still used).
# Idempotent: skips when the target revision is already checked out.

set -euo pipefail

MASTER_SHA="eb0eea955ef8409f034e1a9c28714d00a511a61a"
LEGACY_TAG="6.7.0"
REMOTE="https://github.com/finos/common-domain-model.git"
SPARSE_PATH="rosetta-source/src/main/rosetta"

variant="master"
tag=""
force=0
for arg in "$@"; do
	case "$arg" in
	master) variant="master" ;;
	legacy) variant="legacy" ;;
	--force) force=1 ;;
	-*)
		echo "fetch-cdm.sh: unknown option '$arg'" >&2
		exit 2
		;;
	*)
		if [ "$variant" = "legacy" ] && [ -z "$tag" ]; then
			tag="$arg"
		else
			echo "fetch-cdm.sh: unexpected argument '$arg'" >&2
			exit 2
		fi
		;;
	esac
done

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && git rev-parse --show-toplevel)"
cache_root="${SIGIL_CDM_CACHE:-$repo_root/target/cdm-corpus}"
dest="$cache_root/$variant"

if [ "$variant" = "master" ]; then
	revision="$MASTER_SHA"
	refspec="$MASTER_SHA"
	label="master @ $MASTER_SHA"
else
	tag="${tag:-$LEGACY_TAG}"
	revision="refs/tags/$tag"
	refspec="refs/tags/$tag:refs/tags/$tag"
	label="legacy tag $tag"
fi

verify_head() {
	local expected="$1"
	local actual
	actual="$(git -C "$dest" rev-parse HEAD)"
	if [ "$actual" != "$expected" ]; then
		echo "fetch-cdm.sh: $dest is at $actual but $label was expected" >&2
		echo "  refetch with: $0 $variant${tag:+ $tag} --force" >&2
		return 1
	fi
}

if [ -d "$dest/.git" ] && [ "$force" != 1 ]; then
	if verify_head "$revision"; then
		echo "fetch-cdm.sh: corpus for $variant already present at $dest (SHA matches) — skipping"
		exit 0
	else
		exit 1
	fi
fi

if [ "$force" = 1 ]; then
	rm -rf "$dest"
fi

echo "fetch-cdm.sh: fetching CDM corpus ($label) into $dest"
mkdir -p "$cache_root"
git init -q "$dest"
git -C "$dest" remote add origin "$REMOTE"
# Sparse + blobless: check out only the .rosetta tree without downloading the
# rest of the repo's history or blobs.
git -C "$dest" sparse-checkout init --cone
git -C "$dest" sparse-checkout set "$SPARSE_PATH"
git -C "$dest" -c advice.detachedHead=false \
	fetch -q --depth 1 --filter=blob:none origin "$refspec"
git -C "$dest" -c advice.detachedHead=false checkout -q --detach FETCH_HEAD

# Legacy tags may be annotated: compare against the commit the ref points to.
expected_head="$(git -C "$dest" rev-parse --verify "$revision^{commit}")"
verify_head "$expected_head"

notice="$dest/$SPARSE_PATH/NOTICE.md"
cat >"$notice" <<EOF
# NOTICE

This directory contains material copied from the **FINOS Common Domain Model
(CDM)**, fetched by sigil's \`scripts/fetch-cdm.sh\` for conformance testing.

- Material: FINOS Common Domain Model, \`$SPARSE_PATH\`
- Version: $label
- Source: $REMOTE
- License: Community Specification License 1.0
  (see <https://github.com/finos/common-domain-model> — attribution required
  by its Section 1.2)

Sigil is not affiliated with FINOS; this copy exists solely to test sigil
against the published CDM corpus.
EOF

echo
cat "$notice"
echo "fetch-cdm.sh: done ($(find "$dest/$SPARSE_PATH" -name '*.rosetta' | wc -l) .rosetta files)"
