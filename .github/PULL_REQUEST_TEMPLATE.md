## Summary

- 

## Checks

- [ ] `bun run contracts:generate` ran after any contract/API change
- [ ] `contracts/CHANGELOG.md` updated for any contract change
- [ ] `contracts/src/meta.ts` version bumped for any breaking contract change
- [ ] `bun run --cwd workers test`
- [ ] `bun run --cwd frontend test`
- [ ] `cargo test -p server-rs` if internal Workers API changed
