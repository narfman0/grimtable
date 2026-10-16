#!/usr/bin/env bash
# Copy the data the browser client fetches at runtime out of the submodules.
# Output is a build artifact (gitignored): numbers and effect trees only.
set -euo pipefail
cd "$(dirname "$0")/.."
out=client/public/data
rm -rf "$out"
mkdir -p "$out/enrichment" "$out/fixtures"
cp data/40kdc-data/data/core/deployment-patterns.json "$out/"
for d in data/40kdc-data/data/enrichment/*/; do
  f=$(basename "$d")
  [ -f "$d/abilities.json" ] && mkdir -p "$out/enrichment/$f" && cp "$d/abilities.json" "$out/enrichment/$f/"
done
for d in data/40kdc-data/data/core/*/; do
  f=$(basename "$d")
  [ -f "$d/detachments.json" ] && mkdir -p "$out/core/$f" && cp "$d/detachments.json" "$out/core/$f/"
done
cp crates/roster/fixtures/*.ros "$out/fixtures/"
echo "synced to $out"
