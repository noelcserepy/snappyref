Rules for the pr-reviewer runs on this repository's pull requests, read from the base branch.

- Bugs: read the [check policy](AGENTS.md#build-check-test) and CI results for the head you review. Run the prescribed checks yourself only when CI has no result for that head. Do not run a release build. Standards and Spec have no network, so they leave CI to Bugs.
- Apply the [testing rule](AGENTS.md#testing). Flag expected values computed by
  the code under test, absence-only assertions without a positive case,
  `is_some`/`is_ok`/count-only checks, and restated constants. Require concrete
  inputs and literal outputs or effects at user or process boundaries. Keep
  catalogue relations and proportional tests. Flag duplicates and incidental
  implementation checks. A bug regression must fail before the fix and pass
  after it.
- The reviewer host reaches no Windows machine. Skip `docs/skills/use-windows/SKILL.md`. Windows CI runs on `main` after the merge, and on a branch only when someone runs `gh workflow run ci.yml --ref <branch>`, so the pull request's own Windows job is always skipped. Bugs: look for a dispatched CI run at the exact head, and report its Windows result, or that it is still running. When the diff touches platform code and no such run passed, list Windows under Not verified.
- Bugs: the `CARGO_TARGET_DIR` the reviewer sets is a build slot locked to this review. Keep it, and export `SNAPPYREF_EXCLUSIVE_TARGET=1` before running cargo or inspect scripts so the inspect doctor and runner accept it.
- Run `docs/skills/inspect/scripts/run-scenario.sh --list` and only the scenarios that cover the touched behavior. Read the artifacts, not only the exit code. Add no scenarios. If none covers the behavior, probe it another way and say so.
- egui seams worth probing: same-frame ordering (a draft committed in the same frame as a control click), focus loss, repaint requests, input routing precedence (modifier clicks, pan gestures, double-click hit tests), and history recording around multi-step edits (undo after paste, duplicate, transfer). The app tests in `src/app/tests/` show how to drive the harness. A probe test appended there, run, and deleted is the fastest reproduction.
- Reviewers never run the [update smokes](docs/release-operations.md#update-smokes) or `semantic-search-real-install`. [AGENTS.md](AGENTS.md#donts) owns the author scope for these networked checks. Never write into `library/` or `models/`.
