#!/usr/bin/env bash
#
# Normalize Tauri bundle artifacts to stable, version-less filenames and
# emit a SHA256SUMS manifest over them. See issue #24.
#
# Tauri names its bundles with the embedded app version, e.g.
#
#     Turnstile_1.0.0_universal.dmg
#     Turnstile_1.0.0_amd64.deb
#     Turnstile-1.0.0-1.x86_64.rpm
#     Turnstile_1.0.0_amd64.AppImage
#
# so a `releases/latest/download/<name>` link the website deep-links to would
# break on every release. This script copies each built artifact to a durable,
# version-less name keyed purely on its file type:
#
#     *.dmg       -> Turnstile-macOS-universal.dmg
#     *.deb       -> Turnstile-linux-x86_64.deb
#     *.rpm       -> Turnstile-linux-x86_64.rpm
#     *.AppImage  -> Turnstile-linux-x86_64.AppImage
#
# The originals are left untouched (we copy, never move), so the versioned
# names remain available on the Release alongside the stable ones.
#
# Usage:
#
#     normalize-artifacts.sh --out DIR [--in DIR] [--checksums]
#
#   --in DIR       Search DIR recursively for built artifacts and copy each to
#                  its stable name in --out. The macOS (.dmg) and Linux
#                  (.deb/.rpm/.AppImage) bundles are produced on different
#                  runners, so an --in tree is expected to hold only some of
#                  the four types; whichever are present are normalized.
#   --checksums    After any normalization, (re)write SHA256SUMS in --out over
#                  every regular file there (excluding SHA256SUMS itself).
#   --out DIR      Destination directory for stable-named copies and the
#                  SHA256SUMS manifest. Created if missing.
#
# Typical release wiring (see issue #20): each platform leg runs with --in
# pointing at its `src-tauri/target/.../bundle` tree to stage stable names,
# then a final assembly job collects every stable artifact into one directory
# and runs `--checksums` over the lot so SHA256SUMS spans all four.
#
# Exit status is non-zero on any ambiguity (more than one artifact of a given
# type) or if the requested work would produce nothing, so a silently broken
# build fails the release rather than shipping a partial set.

set -euo pipefail

IN=""
OUT=""
DO_CHECKSUMS=0

die() {
  echo "normalize-artifacts: $*" >&2
  exit 1
}

while [ "$#" -gt 0 ]; do
  case "$1" in
    --in)
      [ "$#" -ge 2 ] || die "--in requires a directory argument"
      IN="$2"
      shift 2
      ;;
    --out)
      [ "$#" -ge 2 ] || die "--out requires a directory argument"
      OUT="$2"
      shift 2
      ;;
    --checksums)
      DO_CHECKSUMS=1
      shift
      ;;
    -h | --help)
      sed -n '2,/^set -euo/p' "$0" | sed 's/^# \{0,1\}//; s/^#$//'
      exit 0
      ;;
    *)
      die "unknown argument: $1"
      ;;
  esac
done

[ -n "$OUT" ] || die "--out DIR is required"
[ -n "$IN" ] || [ "$DO_CHECKSUMS" -eq 1 ] || die "nothing to do: pass --in, --checksums, or both"

mkdir -p "$OUT"

# Resolve a sha256 tool that emits the standard "<hash>  <name>" format. GNU
# coreutils ships `sha256sum`; macOS ships `shasum`, whose `-a 256` output is
# byte-identical. Either works with `sha256sum -c` / `shasum -a 256 -c`.
sha256_tool() {
  if command -v sha256sum >/dev/null 2>&1; then
    echo "sha256sum"
  elif command -v shasum >/dev/null 2>&1; then
    echo "shasum -a 256"
  else
    die "no sha256 tool found (need sha256sum or shasum)"
  fi
}

MATCHED=0

# Copy the single artifact of a given type to its stable name. Zero matches is
# fine (that type is built on another runner); more than one is fatal, since we
# can't know which to publish.
normalize_one() {
  pattern="$1"
  stable="$2"

  found=""
  count=0
  while IFS= read -r f; do
    [ -n "$f" ] || continue
    found="$f"
    count=$((count + 1))
  done < <(find "$IN" -type f -name "$pattern" | LC_ALL=C sort)

  if [ "$count" -eq 0 ]; then
    return 0
  fi
  if [ "$count" -gt 1 ]; then
    echo "normalize-artifacts: more than one '$pattern' under $IN:" >&2
    find "$IN" -type f -name "$pattern" >&2
    die "ambiguous artifact for $stable"
  fi

  cp -f "$found" "$OUT/$stable"
  echo "  $found -> $stable"
  MATCHED=$((MATCHED + 1))
}

if [ -n "$IN" ]; then
  [ -d "$IN" ] || die "--in directory does not exist: $IN"
  echo "Normalizing artifacts from $IN into $OUT:"
  normalize_one "*.dmg" "Turnstile-macOS-universal.dmg"
  normalize_one "*.deb" "Turnstile-linux-x86_64.deb"
  normalize_one "*.rpm" "Turnstile-linux-x86_64.rpm"
  normalize_one "*.AppImage" "Turnstile-linux-x86_64.AppImage"
  [ "$MATCHED" -gt 0 ] || die "no .dmg/.deb/.rpm/.AppImage artifacts found under $IN"
  echo "Normalized $MATCHED artifact(s)."
fi

if [ "$DO_CHECKSUMS" -eq 1 ]; then
  tool="$(sha256_tool)"
  echo "Writing $OUT/SHA256SUMS with $tool:"
  (
    cd "$OUT"
    rm -f SHA256SUMS
    wrote=0
    # Hash every regular file except the manifest itself, in a stable C-locale
    # order so the manifest is reproducible. Basenames are used so that
    # `sha256sum -c SHA256SUMS` verifies from within this directory.
    while IFS= read -r f; do
      [ -f "$f" ] || continue
      [ "$f" = "SHA256SUMS" ] && continue
      # shellcheck disable=SC2086 # $tool may be "shasum -a 256"; word-split intentionally.
      $tool "$f" >>SHA256SUMS
      wrote=$((wrote + 1))
    done < <(find . -maxdepth 1 -type f -exec basename {} \; | LC_ALL=C sort)
    [ "$wrote" -gt 0 ] || die "no files to checksum in $OUT"
  )
  while IFS= read -r line; do
    echo "  $line"
  done <"$OUT/SHA256SUMS"
fi
