#!/bin/sh
# Applies the repository settings, the release deploy key and the rulesets.
# Needs `gh` logged in with admin rights on the repository. Each step can run
# on its own: `tools/github-setup.sh OWNER/REPO settings|deploy-key|rulesets`;
# `all` runs settings and deploy-key, never rulesets.
#
# Order matters on a new repository: settings, then deploy-key, then push a
# branch so every required check reports once, then rulesets. A required
# check that has never reported blocks every merge.
set -eu

repo="${1:?usage: $0 OWNER/REPO [settings|deploy-key|rulesets]}"
step="${2:-all}"

settings() {
  # Squash only. The pull request title becomes the commit subject and the
  # release entry; the description, with the authorship disclosure, becomes
  # the commit body.
  gh repo edit "$repo" --enable-squash-merge --enable-merge-commit=false \
    --enable-rebase-merge=false --delete-branch-on-merge --enable-auto-merge \
    --allow-update-branch --enable-wiki=false --enable-discussions=false
  gh api -X PATCH "repos/$repo" -f squash_merge_commit_title=PR_TITLE \
    -f squash_merge_commit_message=PR_BODY >/dev/null

  gh repo edit "$repo" --enable-secret-scanning --enable-secret-scanning-push-protection
  gh api -X PUT "repos/$repo/vulnerability-alerts"
  gh api -X PUT "repos/$repo/automated-security-fixes"
  gh api -X PUT "repos/$repo/private-vulnerability-reporting"

  # Jobs ask for wider grants themselves.
  gh api -X PUT "repos/$repo/actions/permissions/workflow" \
    -f default_workflow_permissions=read -F can_approve_pull_request_reviews=false

  # A published release's tag and assets can no longer change.
  gh api -X PUT "repos/$repo/immutable-releases"

  # Pages deploys from the CI workflow, not from a branch.
  if gh api "repos/$repo/pages" >/dev/null 2>&1; then
    gh api -X PUT "repos/$repo/pages" -f build_type=workflow
  else
    gh api -X POST "repos/$repo/pages" -f build_type=workflow >/dev/null
  fi
}

deploy_key() {
  # The release job pushes the version commit and tag with this key; the
  # main ruleset lists deploy keys as its only bypass actor. The private key
  # exists only in the repository secret.
  dir="$(mktemp -d)"
  trap 'rm -rf "$dir"' EXIT
  ssh-keygen -q -t ed25519 -N '' -C "release deploy key" -f "$dir/key"
  gh repo deploy-key add "$dir/key.pub" --repo "$repo" --allow-write \
    --title "release: pushes the version commit and tag"
  gh secret set RELEASE_DEPLOY_KEY --repo "$repo" < "$dir/key"
}

rulesets() {
  for file in .github/rulesets/main.json .github/rulesets/tags.json; do
    name="$(sed -n 's/^  "name": "\(.*\)",$/\1/p' "$file")"
    id="$(gh api "repos/$repo/rulesets" --jq ".[] | select(.name == \"$name\") | .id")"
    if [ -n "$id" ]; then
      gh api -X PUT "repos/$repo/rulesets/$id" --input "$file" >/dev/null
    else
      gh api -X POST "repos/$repo/rulesets" --input "$file" >/dev/null
    fi
    echo "applied ruleset $name"
  done
}

case "$step" in
  settings) settings ;;
  deploy-key) deploy_key ;;
  rulesets) rulesets ;;
  all) settings; deploy_key ;;
  *) echo "unknown step: $step" >&2; exit 2 ;;
esac
