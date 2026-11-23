# cairn-plan: built and tested in a Rust image, run on the same Debian release.
FROM rust:1.85-slim-bookworm AS build
WORKDIR /src
COPY . .
RUN cargo test --release --locked && cargo build --release --locked

# The page itself, in a headless browser, against the binary just built
# (tests/screen.py). On x86-64 only: Chromium under emulation is too slow.
FROM debian:bookworm-slim AS screen
COPY --from=build /src/target/release/cairn-plan /usr/local/bin/cairn-plan
COPY tests/screen.py /screen.py
RUN if [ "$(uname -m)" = x86_64 ]; then \
      apt-get update -q && apt-get install -y -q --no-install-recommends chromium python3 && rm -rf /var/lib/apt/lists/* \
      && python3 /screen.py; \
    else echo "skipped on $(uname -m)" > /screen.ok; fi

FROM debian:bookworm-slim
COPY --from=build /src/target/release/cairn-plan /usr/local/bin/cairn-plan
# the image exists only if the page passed its test
COPY --from=screen /screen.ok /usr/share/cairn-plan/screen.ok
USER nobody
ENTRYPOINT ["/usr/local/bin/cairn-plan"]
CMD ["/etc/cairn-plan/config.json"]
