# Git Hooks

Versioned hooks. Enforce the same rules CI does, but at commit/push time
so you catch violations before the feedback loop widens.

## Install

```bash
./scripts/install-hooks.sh
```

Points `git config core.hooksPath` at this directory. Run once per
clone. Safe to re-run.

## What each hook does

### pre-commit (fast, < 5s)

Runs on every `git commit`. Blocks on violation.

- Commit quiet-hours guard: blocks commits from 10:00–18:00 KST (Asia/Seoul)
- Secret scan (AWS keys, private keys, Stripe / OpenAI / Anthropic keys)
- `.env*` file guard (only `.env.example` allowed)
- Terraform state file guard (see `docs/terraform-state-migration-plan.md`)
- `cargo fmt --check` if any `.rs` file is staged
- `scripts/check-layers.sh` (server-rs) if `server-rs/` changed
- `bun run check:layers` (workers) if `workers/src/` changed
- `bun run check:layers` (frontend) if `frontend/src/` changed

Skip with `--no-verify` only if you understand which guardrail you're
turning off. Secrets + layer rules are there for a reason.

### pre-push (medium, < 30s)

Runs on every `git push`. Blocks on violation.

- Contract drift — regenerates Rust bindings from OpenAPI, fails on diff
- `bun run typecheck` for workers
- `bun run typecheck` for frontend
- `cargo check --all-targets` for server-rs

Unit tests + E2E smoke intentionally stay in CI. Pre-push should not
block on anything slower than a typecheck.

### post-merge (informational, no block)

Runs after `git pull` or `git merge`. Prints reminders about things you
might need to reconcile locally:

- Lockfile changes → `bun install`
- D1 migrations → `bun run --cwd workers migrate:local`
- `infra/*.tf` changes → `terraform plan`
- `contracts/` changes → `bun run contracts:generate`
- Dockerfile / compose changes → rebuild stacks
- Hook file changes → re-run install (no-op if unchanged)
- `ARCHITECTURE.md` / `claude.md` → re-read

Never blocks, never fails. Commit is already yours when this runs.

## Why hooks instead of just relying on CI

CI is authoritative but slow. A broken layer rule caught in CI =
already pushed, already failed, already need a follow-up commit. Caught
in pre-commit = fixed before it leaves the branch.

The contract drift check in particular is worth paying 10s for locally
rather than 3 minutes in CI.

## Why not husky / lefthook / pre-commit-framework

All fine tools. For a 3-person-or-less repo with no cross-stack Node
tooling requirement, versioned bash hooks + `core.hooksPath` is simpler:

- No new dependency to install
- Hooks are diff-reviewable in normal `git log`
- One install command, one uninstall command
- Works identically on every machine with bash

If the team grows or the hook set grows complex, migrate to `lefthook`
(single Go binary, YAML config) — not husky (heavier, npm-specific).

## Disabling for one commit

```bash
git commit --no-verify -m "emergency fix, CI will backstop"
git push --no-verify
```

Use sparingly. If you reach for `--no-verify` more than once a week,
fix the hook instead — it's miscalibrated.

## Uninstall

```bash
git config --unset core.hooksPath
```

This stops git from running the versioned hooks. Hooks under `.git/hooks/`
(if any) take over again.
