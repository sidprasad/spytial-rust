# Releasing spytial

This repo publishes two crates to crates.io:

- `spytial_export_macros` — the proc-macro sub-crate (must publish first)
- `spytial` — the main crate (depends on the above)

Releases run via OIDC-based [trusted publishing](https://crates.io/docs/trusted-publishing).
After a one-time bootstrap, no long-lived crates.io tokens live anywhere —
GitHub Actions exchanges a short-lived OIDC token for a 30-minute crates.io
token on each release.

## One-time bootstrap (token-based)

Crates.io's trusted publishing requires each crate to have been published
at least once via a regular API token before OIDC can be configured.
Skip this section if both `spytial` and `spytial_export_macros` are
already on crates.io.

1. Get a token from https://crates.io/me with the `publish-new` scope.
   Log in once:

   ```bash
   cargo login <token>
   ```

2. Publish the macros sub-crate first:

   ```bash
   cd macros
   cargo publish
   ```

3. Wait ~60 seconds for the crates.io index to propagate.

4. Publish the main crate:

   ```bash
   cd ..
   cargo publish
   ```

5. Once both crates are live, configure trusted publishing (next section)
   so future releases don't need a token.

## Configure trusted publishing (one-time per crate)

For each crate (`spytial` and `spytial_export_macros`):

1. Sign in to https://crates.io with the GitHub account that owns the crate
2. Navigate to the crate's settings → **Trusted Publishers**
3. Add a new GitHub Actions publisher with:
   - **Repository owner**: `sidprasad`
   - **Repository name**: `spytial`
   - **Workflow filename**: `release.yml`
   - **Environment**: (leave empty)

Once both crates have trusted publishing configured, you can optionally
disable token-based publishing in each crate's settings — that prevents
any leaked token from being able to publish.

## Releasing a new version

1. On a branch, bump versions in `Cargo.toml` (both `version` and the
   `spytial_export_macros` dependency) and in `macros/Cargo.toml`. Keep
   them in sync.
2. Open and merge the version-bump PR.
3. From `main`, tag and push:

   ```bash
   git checkout main
   git pull
   git tag v0.0.2
   git push origin v0.0.2
   ```

4. The `Release` workflow at `.github/workflows/release.yml` runs
   automatically and:
   - Authenticates to crates.io via OIDC (no token used)
   - Publishes `spytial_export_macros`
   - Waits 60 seconds for the index to update
   - Publishes `spytial`

Watch progress at https://github.com/sidprasad/spytial-rust/actions.

The workflow also supports manual triggering via `workflow_dispatch` if a
release needs to be re-run (e.g. after a transient crates.io failure on
the second publish — the macros crate is already up; trigger the workflow
again and the macros publish will fail-fast on "already exists" while the
main crate publish goes through).

## Troubleshooting

**`error: failed to verify package tarball` during the second publish.**
The crates.io index is eventually consistent. The workflow sleeps 60s
between publishes; if you're running manually, wait longer between the two
`cargo publish` calls.

**`error: api errors (status 403 Forbidden)`** during the OIDC step.
The trusted publisher configuration on crates.io doesn't match the
workflow run trying to publish. Verify that the repository owner,
repository name, and workflow filename match exactly (case sensitive).

**Workflow doesn't run on tag push.** Check that the tag name starts with
`v` (e.g. `v0.0.2`, not `0.0.2`). The trigger is `tags: ['v*']`.
