# reference

## Principles

1. Fast above all. The app should feel almost instant across the board: startup, paste, arrange, crop, drag, and every other core interaction.
2. Layout first. UI structure and hierarchy should come primarily from layout, spacing, alignment, and positioning. Borders and background fills should be used only when they add real clarity.
3. Immediate and shortcut-driven. The app is meant to be used through well-designed shortcuts, with interactions that are direct and available without friction.
4. Sensible defaults. Repeated flows should be handled automatically whenever possible, so users do not need to repeatedly make the same decisions.
5. Simplicity. Keep both the product and the codebase as lightweight and direct as possible. If something feels complex, first ask whether it can be done in a simpler way.

## Builds And Releases

This repo uses GitHub Actions for verification and packaging.

### Pull requests

- Open a PR into `main`
- The `CI` workflow runs on both Windows and macOS
- It checks formatting and runs a lightweight `cargo check`
- Nothing is published from PRs

### Manual test builds

- Open the `Build Artifacts` workflow in GitHub Actions
- Trigger it manually with the branch or tag you want to test
- It produces two zip files:
  - `reference-windows-<sha>.zip`
  - `reference-macos-<sha>.zip`

### Tagged releases

- Push a version tag like `v0.1.0`
- The `Release Artifacts` workflow builds Windows and macOS artifacts
- It uploads both zip files to the GitHub Release for that tag

### Notes

- The Windows build is produced on a Windows runner.
- The macOS build is produced on a macOS runner.
- Release artifacts include the semantic-search model next to the executable under `models/`.
- Development builds still fall back to downloading the model from Hugging Face when packaged model files are absent.
