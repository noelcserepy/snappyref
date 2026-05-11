# Auto-update signing setup

Operational notes for the signing keypair and GitHub Actions secrets that back the auto-update pipeline. See ADR-0015 for the design.

## Keypair

- Algorithm: ed25519
- Private key: stored in the maintainer's password manager. Never committed.
- Public key: [keys/signing-key.pub.pem](keys/signing-key.pub.pem). Embedded as a source constant in slice 4 for manifest verification.

## GHA secrets (source repo `noelcserepy/snappyref`)

Both secrets live on the source repo because the publish workflow runs here, not on the dist repo.

- `MANIFEST_SIGNING_KEY` — ed25519 private key (PEM). Used by `publish-full-release.yml` to sign `latest.json` before upload.
- `DIST_REPO_TOKEN` — fine-grained PAT scoped only to `noelcserepy/snappyref-dist` with **Contents: Read and write**. Used by the publish workflow to attach release assets to the dist repo's draft release.

## Dist repo

- `noelcserepy/snappyref-dist` — public, README only, no license. Holds published release artifacts (app-only + full-install zips per platform, `latest.json`).

## Rotation

If the private key is leaked:

1. Generate a new ed25519 keypair.
2. Replace `MANIFEST_SIGNING_KEY` on the source repo.
3. Replace [keys/signing-key.pub.pem](keys/signing-key.pub.pem) and ship a new app release embedding the new public key.

Clients on older versions will reject manifests signed with the new key and stay stuck on their current version until manually updated.
