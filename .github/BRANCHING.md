# Branching & merge policy (enforced)

## Rule: no agent ever commits directly to `main` (or any release branch)

- **Agents (AI coding agents) may only commit to task/PR branches**
  (`vibe/<slug>-43fef9`, `fix/…`, `feat/…`), push them, and open **draft PRs**.
  All agent work must go through a pull request with human review.
- **Only humans may merge into `main`** or cut release tags
  (`xoa-support-v*`). Direct pushes to `main` are rejected by the
  `human-only-main` ruleset (see "How it is enforced" below).
- Release branches, if ever created (`release/*`), follow the same rule:
  human-only merges.

## How it is enforced

1. **Repo ruleset `human-only-main`** (target: `refs/heads/main`, enforcement:
   active) — requires a pull request with ≥1 approving review, required
   status checks (`ci`, `xoa-support-ci`) green, linear history, and blocks
   force-pushes, deletions, and non-fast-forward updates.
   - *Configure in repo UI*: Settings → Rulesets → New ruleset → Branch,
     add `refs/heads/main`, enable "Require a pull request before merging"
     (1 approval), "Require status checks to pass" (`ci`, `xoa-support-ci`),
     "Do not allow force pushes", "Require linear history", block branch
     deletion. (The sandbox API token cannot create rulesets, so this must be
   set once by a human admin. Two equivalent ways:
   - CLI: `./xoa-support/tools/apply_branch_rules.sh` (posts
     `xoa-support/tools/ruleset-human-only-main.json` via the API)
   - UI import: Settings → Rulesets → New ruleset → Branch → select target
     `main`, then use the JSON import with
     `xoa-support/tools/ruleset-human-only-main.json`.
   - The JSON includes a **bypass list for RepositoryAdmin** (`bypass_mode:
     always`): GitHub never lets a PR author approve their own PR, and with
     `* @eveane` as sole code owner that would deadlock PRs authored by the
     owner. Admins on the bypass list can still **merge** a PR without the
     review requirement — direct pushes to `main` remain blocked for
     everyone, agents included.
2. **`.github/CODEOWNERS`** — `@eveane` owns everything; with "require code
   owner reviews" enabled on the protection rule, a human must approve.
3. **Workflows run on `pull_request`** (not only `push`), so agent branches
   are gated by CI before a human reviews them.
4. **Release tags** are pushed by humans only; the
   `xoa-support-release` workflow then publishes the `.deb` automatically.

## What agents should do (workflow)

1. `git checkout -b vibe/<short-slug>-43fef9` (never work on `main`)
2. commit with conventional messages (`feat:`/`fix:`/`docs:`/…)
3. `git push -u origin <branch>`
4. open a **draft PR**; report the PR URL
5. iterate on CI failures by pushing more commits to the same branch
6. stop — a human reviews and merges

## Accidental direct pushes

If an agent (or anyone) pushes directly to `main`, the ruleset rejects it.
If a push somehow lands (e.g. ruleset temporarily disabled), revert it with
a normal PR (`git revert`) — never force-push main.
