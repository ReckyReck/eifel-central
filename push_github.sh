#!/usr/bin/env bash
# Uploads your eifel-central folder's changes to GitHub. Afterwards build the app on github.com:
# eifel-central > Actions > "Build app" > Run workflow.
# Usage:  cd ~/eifel-central && bash push_github.sh
set -euo pipefail
cd "$(dirname "$0")"
[ -d .git ] || { printf 'This folder is not linked yet. Run:  bash link_github.sh\n' >&2; exit 1; }

git add -A
if git diff --cached --quiet; then printf 'Nothing new to upload.\n'; exit 0; fi
printf 'Uploading:\n'; git diff --cached --stat | tail -n 15
git commit -q -m "Update $(date '+%Y-%m-%d %H:%M')"
if ! git push -q origin main; then
  printf '\nERROR: GitHub has changes this folder does not have (edited on the website?). Send me the lines above.\n' >&2
  exit 1
fi
printf '\nUploaded. Build the app on github.com: eifel-central > Actions > "Build app" > Run workflow.\n'
