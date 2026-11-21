# Contributing

Read [AGENTS.md](AGENTS.md) and the [ground rules](docs/plan/00-ground-rules.md)
before making changes. Contributions must be your own work, written for this project.

Run `scripts/check.sh` from the repository root. It checks Rust formatting, runs Clippy
with warnings denied, runs the Rust and JavaScript tests, and audits Rust dependencies.
Install Node.js 24 or newer to run the full JavaScript suite locally.

Each dependency must offer an MIT licence, including development and build dependencies.
`scripts/license-check.sh` checks the resolved Cargo dependency graph and permits only the
approved build-time exception documented in the ground rules. Keep each vendored browser
library's licence and version under `web/vendor/` and record it in
[THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).
