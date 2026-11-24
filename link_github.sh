#!/usr/bin/env bash
# One-time: links this eifel-central folder to its GitHub repository (ReckyReck/eifel-central), then uploads it.
# First create the repository on github.com: "+" > New repository > name: eifel-central, PUBLIC, and NO readme /
# licence / .gitignore (it must start empty). Then:  cd ~/eifel-central && bash link_github.sh
set -euo pipefail
cd "$(dirname "$0")"
REPO="https://github.com/ReckyReck/eifel-central.git"
BRANCH="main"

say() { printf '\n==> %s\n' "$*"; }
die() { printf '\nERROR: %s\n' "$*" >&2; exit 1; }
command -v git >/dev/null 2>&1 || die "git is missing - install Apple's Command Line Tools with: xcode-select --install"

if [ -d .git ]; then
  say "This folder is already linked:"; git remote -v; exit 0
fi

say "Linking this folder to $REPO"
git init -q -b "$BRANCH"
git config user.name  "ReckyReck"
git config user.email "ReckyReck@users.noreply.github.com"
git config credential.helper osxkeychain           # uses the token already in the Mac's keychain
git remote add origin "$REPO"

say "Reading what is on GitHub"
heads="$(git ls-remote --heads origin 2>/dev/null)" || { rm -rf .git; die "Could not reach $REPO. Does the repository exist (name eifel-central)? If your token is limited to some repositories, add eifel-central to it on github.com (Settings > Developer settings > Personal access tokens), then run this again."; }

git add -A
if [ -z "$heads" ]; then                            # new, empty repository: the first upload
  git commit -q -m "Eifel Mastering Central: first version (catalog view)"
  git push -q -u origin "$BRANCH" || { rm -rf .git; die "Upload failed - send me the lines above."; }
  say "Done: linked and uploaded."
else
  git fetch -q origin "$BRANCH" || { rm -rf .git; die "The repository has no branch '$BRANCH' - send me this message."; }
  git reset -q "origin/$BRANCH"                    # GitHub's history, your files untouched
  git branch -q --set-upstream-to="origin/$BRANCH"
  git add -A
  if git diff --cached --quiet; then
    say "Your folder and GitHub are identical. Linked."
  else
    say "Uploading the differences:"; git diff --cached --stat | tail -n 15
    git commit -q -m "Update from Recky's Mac"
    git push -q origin "$BRANCH" || die "Upload failed - send me the lines above."
    say "Done: linked and uploaded."
  fi
fi
printf '\nFrom now on, after each change:  cd ~/eifel-central && bash push_github.sh\nBuild the app: github.com > eifel-central > Actions > "Build app" > Run workflow.\n'
