# Third-party notices

Vendored browser libraries (when present) are listed below with their licence files beside them
under `web/vendor/`.

Rust crate licences are checked by `scripts/license-check.sh`. Every crate must offer MIT,
except the approved build-time exception for `unicode-ident`.

| Name | Version | Licence | Where |
|---|---|---|---|
| @xterm/xterm | 6.0.0 | MIT | `web/vendor/xterm/` (`xterm.mjs`, `xterm.css`, `LICENSE-xterm`) |
| @xterm/addon-fit | 0.11.0 | MIT | `web/vendor/xterm/` (`addon-fit.mjs`, `LICENSE-addon-fit`) |
| uiGradients | commit `c07376ba587c6607bfb01465a88b26ee5f92d80f` | MIT | `web/vendor/uigradients/` (`gradients.json`, `LICENSE`) |
| CodeMirror 6 (bundled) | 6.0.2 (see `web/vendor/codemirror/VERSION` for language packages) | MIT | `web/vendor/codemirror/` (`codemirror.js`, `LICENSES.txt`) |
