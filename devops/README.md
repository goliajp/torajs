# torajs / devops

Files committed here are reference inputs for the v0.1 deployment.
They live with the repo so they're versioned alongside the code that
depends on them; the actual operator action (DNS edits, CaddyStore
updates, GitHub release-secret config) happens out-of-band.

## Files

| File | Purpose |
|---|---|
| `torajs.com.caddyfile` | Caddy site block for the production landing page — serves `web/dist/` as a static bundle, handles SPA routing, redirects `www.torajs.com → torajs.com` (308) |
| `install.torajs.com.caddyfile` | Caddy site block for the vanity install URL — 302-redirects to `raw.githubusercontent.com/goliajp/torajs/main/install.sh` |

## Current state (audited 2026-05-04)

- `devops dns list torajs.com` → **parked, no records**. The domain
  is registered (visible in DnsStore) but DNS-wise it doesn't
  resolve anywhere yet.
- `devops caddy list` → **no torajs.com / install.torajs.com
  sites**. The CaddyStore on t01 has every other golia.jp /
  golia.ai property but not torajs.com.
- `web/dist/` → builds cleanly via `bun run build`; ready to rsync
  to `t01:/apps/torajs/web/` once Caddy is wired.
- `github.com/goliajp/torajs` → repo state unverified from this
  side (no public access via `gh` from the dev box).

## v0.1.0-beta release runbook

Pre-flight (one-time, takagi or devops):

1. **DNS** (requires DnsStore admin access; CLI is `devops dns sync`
   after the records exist in the store, but adding them goes through
   the devops web UI / API). Add the following A records for
   `torajs.com` (use the same target IP every other golia.jp
   property uses — `18.179.107.143` based on the bitreits.com
   reference):

   ```
   @            A    18.179.107.143
   www          A    18.179.107.143
   install      A    18.179.107.143
   ```

   Then `devops dns sync torajs.com` to push to Cloudflare.

2. **Caddy** (CaddyStore admin). Add the two committed site blocks
   to the store:

   - `devops/torajs.com.caddyfile` → site id `torajs-com`
   - `devops/install.torajs.com.caddyfile` → site id `install-torajs-com`

   Then `devops caddy deploy t01` to generate + push the merged
   Caddyfile and reload.

3. **Web bundle on t01**: from a workstation with rsync access,

   ```sh
   cd web && bun install && bun run build
   rsync -az --delete dist/ t01:/apps/torajs/web/
   ```

4. **GitHub repo**: confirm `github.com/goliajp/torajs` exists,
   is public (or visible to the install audience), and the
   `develop` branch is pushed. The CI in
   `.github/workflows/release.yml` runs on tag push and writes
   release artifacts via the default `GITHUB_TOKEN`.

5. **Verify** before tagging:

   ```sh
   curl -fsSI https://torajs.com           # → HTTP/2 200
   curl -fsSI https://www.torajs.com       # → HTTP/2 308 → torajs.com
   curl -fsSI https://install.torajs.com   # → HTTP/2 302 → raw.githubusercontent.com
   ```

Release ceremony (per release):

```sh
# from `develop`, sync `main` to the release point
git checkout main
git merge --ff-only develop
git tag v0.1.0-beta
git push origin main v0.1.0-beta
```

The tag push triggers `.github/workflows/release.yml`:

- builds tr binary for `aarch64-apple-darwin` + `x86_64-unknown-linux-gnu`
- tarballs each (incl. `docs/`, `examples/`, `README.md`)
- uploads to a GitHub release named after the tag, marked
  `prerelease=true` because the tag contains `beta`

Smoke test after the release publishes:

```sh
curl -fsSL https://install.torajs.com | bash
~/.torajs/bin/tr --version
~/.torajs/bin/tr run ~/.torajs/share/examples/sha256/sha256.ts
```

## Open questions for devops

- Does t01 already have an outbound HTTPS path to fetch the GitHub
  raw URL on every install? (302 is client-side, so the answer is
  yes — but worth confirming nothing in the network egress filters
  blocks `raw.githubusercontent.com`.)
- Do we want install hits logged to the same Datadog / log-aggregation
  pipeline as the rest of `torajs.com`, or is the Caddy-local log
  enough for v0.1?
- Should we also add `torajs.com/install` as a URL alias (same
  redirect target) so users without the vanity subdomain get the
  same UX?
