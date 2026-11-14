#!/bin/sh
# Publishes the web build (./web) to the gh-pages branch, which GitHub Pages serves.
# Run ./build_web.sh first.
set -e
cd "$(dirname "$0")"
REMOTE=$(git remote get-url origin)
TMP=$(mktemp -d)
cp -R web/. "$TMP"/
touch "$TMP/.nojekyll"
cd "$TMP"
git init -q -b gh-pages
git add -A
git commit -q -m "Deploy web build

Co-Authored-By: Claude Fable 5.2 <noreply@anthropic.com>"
git push -q -f "$REMOTE" gh-pages
cd - >/dev/null
rm -rf "$TMP"
echo "Deployed to gh-pages"
