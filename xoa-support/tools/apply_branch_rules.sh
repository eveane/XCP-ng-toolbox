#!/bin/bash
# apply_branch_rules.sh — create the 'human-only-main' ruleset that blocks
# direct pushes to main (agents must use PR branches; only humans merge).
#
# Run ONCE by a human repo admin (the automation token cannot create
# rulesets). Requires: gh CLI, admin on the repo.
#
#   ./xoa-support/tools/apply_branch_rules.sh [owner/repo]
set -euo pipefail
REPO=${1:-eveane/XCP-ng-toolbox}

gh api -X POST "repos/$REPO/rulesets" --input - <<'JSON'
{
  "name": "human-only-main",
  "target": "branch",
  "enforcement": "active",
  "conditions": {"ref_name": {"include": ["refs/heads/main"], "exclude": []}},
  "rules": [
    {"type": "pull_request", "parameters": {
      "required_approving_review_count": 1,
      "dismiss_stale_reviews_on_push": true,
      "require_code_owner_reviews": true,
      "required_review_thread_resolution": true
    }},
    {"type": "required_status_checks", "parameters": {
      "required_status_checks": [
        {"context": "ci"},
        {"context": "xoa-support-ci"}
      ],
      "strict_required_status_checks_policy": true
    }},
    {"type": "non_fast_forward"},
    {"type": "deletion"},
    {"type": "required_linear_history"}
  ]
}
JSON

echo "ruleset 'human-only-main' created/updated on $REPO"
echo "verify: gh api repos/$REPO/rulesets --jq '.[].name'"
