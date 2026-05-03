# torajs / devops

Files committed here are reference inputs for the v0.1 deployment.
They live with the repo so they're versioned alongside the code that
depends on them; the actual operator action (DNS edits, CaddyStore
updates, GitHub release-secret config) happens out-of-band.

## Files

| File | Purpose |
|---|---|
| `install.torajs.com.caddyfile` | Caddy site block for the vanity install URL — 302-redirects to `raw.githubusercontent.com/goliajp/torajs/main/install.sh` |

## v0.1.0-beta release runbook

Pre-flight (one-time, takagi or devops):

1. **GitHub repo**: confirm `github.com/goliajp/torajs` exists and is
   public (or visible to the install audience). The CI in
   `.github/workflows/release.yml` runs on tag push and writes
   release artifacts via the default `GITHUB_TOKEN`.
2. **DNS**: add `install.torajs.com → t01` (CNAME or A record per
   the rest of the torajs.com setup). Same for `www.install.torajs.com`
   if we want the redundant alias.
3. **Caddy**: paste `devops/install.torajs.com.caddyfile` into the
   CaddyStore site config and run `devops caddy deploy t01`.
4. **Verify**: `curl -fsSI https://install.torajs.com` should return
   a 302 to the GitHub raw URL.

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
