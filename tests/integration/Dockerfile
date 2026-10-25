FROM rust:1.99-bookworm@sha256:59037199c44290f2befcdd58dcc540164763fc296950255aaefeef096a1866b0 AS builder

WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY LICENSE ./LICENSE
COPY src ./src
COPY integrations ./integrations
COPY tests ./tests
RUN cargo build --release --locked \
    && cargo test --release --locked --test integration --no-run \
    && cp "$(find target/release/deps -maxdepth 1 -type f -name 'integration-*' -executable)" /agentlink-integration-test

FROM node:22-bookworm-slim@sha256:c3de60bf2f9dd0ac6370e6117950ff62d6e339527e7472301c9c78a017978392

RUN apt-get update \
    && apt-get install --yes --no-install-recommends ca-certificates git \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /src/target/release/agentlink /usr/local/bin/agentlink
COPY --from=builder /agentlink-integration-test /usr/local/libexec/agentlink-integration-tests
COPY --chmod=755 tests/integration/entrypoint /usr/local/bin/agentlink-integration-test
COPY LICENSE /usr/local/share/licenses/agentlink/LICENSE
ENV AGENTLINK_BIN=/usr/local/bin/agentlink

ENTRYPOINT ["agentlink-integration-test"]

CMD ["--include-ignored", "--test-threads=1"]
