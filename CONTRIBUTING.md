# Contributing

Read [AGENTS.md](AGENTS.md) first. It has the process model, the module map,
and the rules the code follows.

## Setup

```sh
git clone https://github.com/noelcserepy/snappyref.git
cd snappyref
git config core.hooksPath .githooks
```

Platform prerequisites (libjpeg-turbo on macOS, vcpkg or
`--no-default-features` on Windows) are in
[README](README.md#build-from-source).

## Checks

```sh
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo check --locked
cargo test --locked
```

The pre-push hook runs the first three plus one update-preservation test, and
CI runs the same gate on every pull request. For UI behavior, run the matching inspect
scenario with `docs/skills/inspect/scripts/run-scenario.sh <scenario>` and
keep the artifact path for the PR.

## Pull requests

- Keep the scope small. One behavior or one cleanup per PR.
- A behavior change needs evidence, an inspect artifact or a test.
- Do not include `library/`, `models/`, or any personal image data.
- Follow the existing names. The codebase is the glossary.

Issues are welcome, including bug reports from the released app. PRs are
reviewed as time allows.
