# Security

Report vulnerabilities privately through GitHub's private vulnerability
reporting on this repository. Open the Security tab and choose Report a
vulnerability. Do not open a public issue for a security problem.

In scope: the SnappyRef app, the `snappyref-updater` helper and the Windows
self-replace path, the signed `latest.json` manifest verification in
`src/updater`, and AI control in `src/control`. AI control is off by default.
When on, the app listens on 127.0.0.1 and accepts only connections that send
the token from `control.json` in the Library folder. Out of scope: the third-party model weights and issues that
need an already compromised local machine.
