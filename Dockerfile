# Two images from one file: `--target server` (the default, last stage) and `--target plugins`.
# Each target builds only its own binary, so plugin dependencies never slow down server builds.
FROM rust:1.99.1-trixie@sha256:767c71592f7d1f3f242292000f8efd4e67b6732363da176772f4d220332e264f AS builder
WORKDIR /build
COPY . .

FROM builder AS build-plugins
# The target dir is a cache mount and gone after this step, hence the copy out of it.
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/build/target \
    cargo build --release --locked -p ferret-plugins \
    && cp target/release/ferret-plugins /ferret-plugins

FROM builder AS build-server
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/build/target \
    cargo build --release --locked -p ferret-server \
    && cp target/release/ferret /ferret

FROM debian:trixie-slim@sha256:62bd423aeee912639ca8ee681c2b762cb3e25137e0a92b20be455898a9f034ad AS runtime
# reqwest's rustls client loads the system's root certificates when it's built, and fails without
# any, even for plain-http calls like the server's to TEI. They also serve https git remotes.
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*

FROM runtime AS plugins
# The git plugin shells out to git.
RUN apt-get update && apt-get install -y --no-install-recommends git \
    && rm -rf /var/lib/apt/lists/*
COPY --from=build-plugins /ferret-plugins /usr/local/bin/ferret-plugins
USER nobody
CMD ["ferret-plugins"]

FROM runtime AS server
COPY --from=build-server /ferret /usr/local/bin/ferret
USER nobody
EXPOSE 8080
CMD ["ferret"]
