#!/usr/bin/env bash
# Checks that TAG can be released and prints its release notes.
#
#   scripts/release_notes.sh vX.Y.Z [REPO_DIR] > notes.md
#
# REPO_DIR (default: this script's repository) is the checkout to read
# Cargo.toml and CHANGELOG.md from, so the script can check a tag made
# before it existed. TAG must be v + Cargo.toml's version, and
# CHANGELOG.md must have a dated section for that version
# (`## X.Y.Z - YYYY-MM-DD`) with something in it.
# The notes are that section's body. Any problem exits 1 with a message on
# stderr; see docs/versioning.md.
set -euo pipefail

if [[ $# -lt 1 || $# -gt 2 ]]; then
	echo "usage: $0 vX.Y.Z [REPO_DIR]" >&2
	exit 2
fi
tag=$1
repo=${2:-$(cd "$(dirname "$0")/.." && pwd)}

version=$(sed -n 's/^version = "\(.*\)"$/\1/p' "$repo/Cargo.toml" | head -n 1)
if [[ -z $version ]]; then
	echo "no version in Cargo.toml" >&2
	exit 1
fi
if [[ $tag != "v$version" ]]; then
	echo "tag $tag does not match Cargo.toml's version $version (want v$version)" >&2
	exit 1
fi

notes=$(awk -v v="$version" '
	index($0, "## " v " - ") == 1 {
		found = 1
		date = substr($0, length("## " v " - ") + 1)
		next
	}
	found && /^## / { exit }
	found { print }
	END {
		if (!found) { print "no \"## " v " - YYYY-MM-DD\" section in CHANGELOG.md" > "/dev/stderr"; exit 1 }
		if (date !~ /^[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]$/) {
			print "CHANGELOG.md section " v " is dated \"" date "\", not YYYY-MM-DD" > "/dev/stderr"; exit 1
		}
	}' "$repo/CHANGELOG.md")

# Trim leading and trailing blank lines.
notes=$(printf '%s\n' "$notes" | sed -e '/./,$!d')
if [[ -z ${notes//[[:space:]]/} ]]; then
	echo "CHANGELOG.md section $version is empty" >&2
	exit 1
fi
printf '%s\n' "$notes"
