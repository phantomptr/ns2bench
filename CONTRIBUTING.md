# Contributing to NS2Bench

Thanks for helping. Bug reports, hardware reports (which board, capture card and console
firmware worked or failed) and translations are as welcome as code.

## How to contribute

1. Fork the repo and create a branch from `main`.
2. Make your change, with a test where it makes sense.
3. Run the quality gate below.
4. Open a pull request using the template. PRs are squash-merged.

## Dev setup

You need Rust (stable) and Node 20+ for the scripts. On macOS, Xcode command line tools
(the capture helper is Swift). On Linux, `ffmpeg` and the Tauri webview packages
(`libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev`).

```sh
cp examples/bench.example.json bench.json   # your devices; never committed
make serve                                  # engine + UI at http://127.0.0.1:7630
make app-dev                                # desktop app from source
```

## Quality gate

```sh
make test                                   # Rust tests
cd native && cargo clippy --workspace --all-targets -- -D warnings
node scripts/update-version.mjs --check     # version files agree
node scripts/i18n.mjs --check               # every language has every string
node --test scripts/*.test.mjs
node --test tests/web/*.test.mjs        # UI end to end (needs: cargo build -p ns2bench, ffmpeg, playwright)
```

## Guidelines

- Keep the console offline. Nothing may give it internet access (no forwarding, NAT,
  proxy or upstream DNS). See the security notes in `SECURITY.md`.
- Input must never stick: every path that presses must release, also on errors and
  disconnects.
- Interface text is English in `web/`; run `node scripts/i18n.mjs` after changing it and
  add translations to `web/i18n/<lang>.json` (or leave them to a translation PR).
- Don't commit personal data: `bench.json`, tokens, device serials, captures and logs stay
  in `~/.ns2bench/` or are git-ignored.
- Add a line under `## Unreleased` in `CHANGELOG.md`, written for users.

## License of contributions

NS2Bench is GPL-3.0-or-later. By sending a pull request you agree that your contribution is licensed under the same terms.

## Questions

Ask on [Discord](https://discord.gg/BqjZE6vZE) or open a discussion issue.
