# Managed local library beside app-owned release files

SnappyRef stores user-owned library data in a managed `library/` directory beside the installed app, with `library.db`, managed originals, and canvas cache files owned by the user rather than the release artifact. Release zips replace only app-owned files such as the executable, model bundle, app bundle, and update notes; they must not include, delete, or recreate `library/`.

This supersedes the earlier planning idea that originals would stay wherever they were on disk. Copying originals into a managed local library makes imports durable across source-file moves and keeps the manual update contract simple: app updates can be file replacement, while the user’s **Library**, **Assets**, **Boards**, and **User Session** remain in place.

## Consequences

- The app may copy imported source files into `library/originals/`.
- Canvas cache derivatives live under the user-owned library, not inside release artifacts.
- Database migrations are forward-only; older app versions are not guaranteed to open a library after a newer version migrates it.
- Tests and inspection harnesses may override the library root, but production startup resolves it from the install root.
