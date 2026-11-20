# cairn-plan: built and tested in a Rust image, run on the same Debian release.
FROM rust:1.85-slim-bookworm AS build
WORKDIR /src
COPY . .
RUN cargo test --release --locked && cargo build --release --locked

FROM debian:bookworm-slim
COPY --from=build /src/target/release/cairn-plan /usr/local/bin/cairn-plan
USER nobody
ENTRYPOINT ["/usr/local/bin/cairn-plan"]
CMD ["/etc/cairn-plan/config.json"]
