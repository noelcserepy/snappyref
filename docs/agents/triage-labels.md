# Triage Labels

SnappyRef uses Matt Pocock's canonical triage roles as GitHub label strings.

Every triaged issue should have exactly one category label and exactly one state label.

## Category labels

| Role | GitHub label | Meaning |
| --- | --- | --- |
| `bug` | `bug` | Something is broken |
| `enhancement` | `enhancement` | New feature or improvement |

## State labels

| Role | GitHub label | Meaning |
| --- | --- | --- |
| `needs-triage` | `needs-triage` | Maintainer needs to evaluate this issue |
| `needs-info` | `needs-info` | Waiting on reporter or maintainer clarification |
| `ready-for-agent` | `ready-for-agent` | Fully specified and ready for an AFK agent |
| `ready-for-human` | `ready-for-human` | Requires human implementation or judgment |
| `wontfix` | `wontfix` | Will not be actioned |

`question` exists in GitHub's default labels, but do not use it as the triage state. Use `needs-info` for triage.
