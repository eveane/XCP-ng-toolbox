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
HERE=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)

gh api -X POST "repos/$REPO/rulesets" --input "$HERE/ruleset-human-only-main.json"

echo "ruleset 'human-only-main' created/updated on $REPO"
echo "verify: gh api repos/$REPO/rulesets --jq '.[].name'"
echo
echo "UI alternative: Settings > Rulesets > New ruleset > Branch, then import"
echo "  $HERE/ruleset-human-only-main.json (GitHub accepts JSON import on the ruleset form)"
