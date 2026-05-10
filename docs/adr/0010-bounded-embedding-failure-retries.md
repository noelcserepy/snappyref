# Bounded embedding failure retries

SnappyRef treats semantic-search embedding failures as either **Retryable Embedding Failure** or **Terminal Embedding Failure** so background indexing does not repeatedly spend idle CPU on assets that are unlikely to recover. Retryable failures get one **Embedding Retry** after never-tried embedding work has drained, then one delayed retry after five minutes; another failure becomes an **Exhausted Embedding Failure** with no further automatic retries.

## Decision

- Never-tried embeddings run before retries.
- A first retryable failure is eligible for retry only when the normal embedding queue is otherwise empty.
- A second retryable failure is eligible once after a five-minute delay.
- A third retryable failure becomes exhausted and is not retried automatically.
- Terminal failures are not retried automatically.
- Demand embedding may force one retry for terminal or exhausted failures, and any success clears the failure state.

## Consequences

- Corrupt or unsupported assets stay quiet instead of recurring at every startup or idle window.
- Transient model, storage, or I/O issues still get a small recovery window.
- Failure state belongs to the asset/source and model, not to app lifetime; restarting the app does not reset retry state.
