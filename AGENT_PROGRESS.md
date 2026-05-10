# AGENT_PROGRESS

## Current task
Update Brivva landing page to reflect voice cloning tiers.

## Checklist
- [x] Inspect landing page and tests.
- [x] Add landing copy for instant 2-minute cloning, 10-minute voice upload, and enterprise B2B high-quality cloning.
- [x] Run focused frontend tests/typecheck.
- [x] Commit and push.
- [x] Deploy frontend.

## Notes
- User screenshot path is local to their Mac and not accessible from this environment, but requested copy was clear.
- Previous Yuna/Gitae preset code was deployed to Workers/Pages/server after user reported options were not visible.

## Tests run
- `bun run --cwd frontend test home-page` ✅
- `bun run typecheck:frontend` ✅

## Commits
- `0b252ad feat(landing): show voice cloning tiers`

## Deployment
- Frontend deployed to Cloudflare Pages.
- Preview URL: `https://6a51b18c.brivva.pages.dev`

## Next action
None.
