# Developing AgentLink

## Install from source

Building needs Rust/Cargo 1.99 or newer and a C linker. Runtime needs Git; see the [usage guide](usage.md#configuration) for registry access requirements. Linux x86-64 is tested; macOS is unverified and Windows is unsupported.

From the repository root:

```sh
cargo install --path . --locked &&
export PATH="${CARGO_HOME:-$HOME/.cargo}/bin:$PATH" &&
agentlink --version
```

The binary embeds its integration assets. To upgrade or roll back, check out the desired version and repeat the install, then run `agentlink install-hooks` to refresh installed integrations. Configuration and the registry checkout stay in your home directory.

If `agentlink` is missing, check `PATH`; linker errors indicate missing system build tools.

## Run the checks

From the repository root with Rust, rustfmt, Clippy and Git installed:

```sh
cargo fmt --check &&
cargo clippy --locked --all-targets -- -D warnings &&
cargo test --locked &&
sh tests/install.sh
```

The installation test builds a release binary into a temporary prefix and tests local setup, updates and offline operation in an isolated home. Default Cargo tests need no third-party agent CLIs; tests that invoke them are ignored until those CLIs are installed.

To run the same checks without a host Rust toolchain:

```sh
docker run --rm -v "$PWD:/source:ro" rust:1.99-bookworm sh -ec 'cp -a /source /tmp/agentlink; cd /tmp/agentlink; rustup component add rustfmt clippy; cargo fmt --check; cargo clippy --locked --all-targets -- -D warnings; cargo test --locked; sh tests/install.sh'
```

## Test the harness integrations

Build the integration image and run it with Docker Buildx:

```sh
docker buildx bake --load &&
docker run --rm agentlink-integration:local
```

At startup, the entrypoint installs the latest Claude, Codex, Gemini and Pi CLIs from npm, logs their versions, and runs the integration tests sequentially. It needs internet access, but no host home directory, credentials or model account. Upstream CLI changes can affect results between runs.

To run only the core integration suite in the same image:

```sh
docker run --rm --entrypoint /usr/local/libexec/agentlink-integration-tests agentlink-integration:local --test-threads=1
```

`IMAGE_REPOSITORY` and `IMAGE_TAG` configure the Bake tag; `CI_COMMIT_SHA` overrides the tag. Publishing requires `--push`; the default build needs no registry or private infrastructure.

For a disposable Kubernetes Job, `tests/integration/run` accepts `IMAGE_REPOSITORY`, `BUILDX_BUILDER`, and optionally `BAKE_DEFAULTS` and `AGENTLINK_TEST_NAMESPACE`. It requires Docker Buildx, kubectl, jq, access to the selected registry, and permission to create the test namespace and Jobs. Use a registry reachable by both the builder and cluster. The launcher runs the complete suite, records its image digest and cleans up the Job.
