# Minimal runtime image for in-cluster `aivcs infra` reconcilers (lornu-ai/infra-code#221).
FROM rust:1.83-bookworm AS builder
WORKDIR /src
COPY . .
RUN cargo build --release -p aivcs-cli

FROM gcr.io/distroless/cc-debian12:nonroot
COPY --from=builder /src/target/release/aivcs /usr/local/bin/aivcs
USER nonroot:nonroot
ENTRYPOINT ["/usr/local/bin/aivcs"]
