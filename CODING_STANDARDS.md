# Coding standards

Read by the PR reviewer, not loaded into implementation sessions.

- Model checking devices stay in the model. When a touched `.specs/<module>/<module>.yml` names `model:`, the model's category strings and coverage variable names must not appear in `src/`.
