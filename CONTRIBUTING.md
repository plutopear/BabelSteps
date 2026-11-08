# Contributing

Thanks for your interest. BabelSteps is at an early stage and there is no release yet, so the design still changes often. Opening an issue to discuss an idea before writing code saves everyone time.

## Ground rules

- **No network code.** No networking crates, telemetry, analytics or accounts. This is the product's main promise, and CI checks it.
- **`core/` holds no platform code.** Platform code goes behind the backend traits described in `docs/03-architecture.md`.
- **Privacy rules for capture** are in `docs/04-privacy-and-security.md`. Changes to capture, redaction, storage or export need to follow them.
- **No private data in the repository.** Do not commit real screenshots, recordings, window titles or file paths from your own machine.

## Building and testing

See the "Environment" section of `CLAUDE.md` for the build commands. Run `cargo test --workspace` before opening a pull request. The Windows capture code needs Windows 11 to build and run.

## Pull requests

- Keep each pull request to one change, and say what you tested and what you could not test.
- CI must pass.
- By contributing, you agree that your contribution is licensed under the same terms as the project (MIT OR Apache-2.0), without additional terms.

## Reporting security or privacy problems

Please report these privately, as described in `SECURITY.md`.
