# User data fallback for protected installs

SnappyRef originally kept the managed **Library**, **Local Model Store**, and Semantic Search download staging under the executable's install root. That preserves the portable-folder workflow, but it fails when users place the app in protected locations such as `/Applications` on macOS or `Program Files` on Windows. In those installs the app can launch, but image import and Semantic Search installation both fail because they need to create sibling `library/`, `models/`, and `updates/` directories.

## Decision

- Treat the executable install root as the preferred **data root** only when it is writable.
- If the install root is not writable, use the user's app-data directory instead:
  - macOS: `~/Library/Application Support/SnappyRef/`
  - Windows: `%LOCALAPPDATA%\SnappyRef\`, falling back to `%APPDATA%\SnappyRef\`
  - other platforms: `$XDG_DATA_HOME/SnappyRef/` or `~/.local/share/SnappyRef/`
- `SNAPPYREF_LIBRARY_ROOT` continues to override only the managed **Library** path for tests and diagnostics.
- `SNAPPYREF_DATA_ROOT` overrides the shared data root for the **Library**, **Local Model Store**, and Semantic Search staging.
- Auto-update still resolves the executable install root directly. App bundle swaps operate on the app install location, not the data root fallback.

## Consequences

- Fresh protected installs can import images and install Semantic Search without requiring the user to move the app folder.
- Portable installs keep their existing sibling `library/`, `models/`, and `updates/` layout.
- Users who move from a writable portable install to a protected install may have separate data roots unless they explicitly move their library or set an override.
- Semantic Search install/remove/status must use the shared data root, not the updater install root.
