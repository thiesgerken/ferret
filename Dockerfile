FROM rust:1.99.0-trixie@sha256:81015672d6939b0265f09adf96d5de125482bb1330106076956b0518e74edc90 AS builder

WORKDIR /build
COPY . .
# The target dir is a cache mount and gone after this step, hence the copy out of it.
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/build/target \
    cargo build --release --locked -p ferret-server \
    && cp target/release/ferret /ferret

FROM debian:trixie-slim@sha256:a29215f6a35e51e22adffa17f89e9d2ef06214e64a2bad10d765c46aea49f11f

COPY --from=builder /ferret /usr/local/bin/ferret
USER nobody
EXPOSE 8080
CMD ["ferret"]
