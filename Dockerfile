FROM lukemathwalker/cargo-chef:latest-rust-1.62 AS chef

RUN apt-get install g++-aarch64-linux-gnu libc6-dev-arm64-cross
RUN rustup target add aarch64-unknown-linux-gnu

WORKDIR app

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder 
COPY --from=planner /app/recipe.json recipe.json
# Build dependencies - this is the caching Docker layer!
RUN cargo chef cook --release --recipe-path recipe.json
# Build application
COPY . .
RUN cargo build --release --bin centarr

# We do not need the Rust toolchain to run the binary!
FROM debian:buster-slim AS runtime
WORKDIR app
COPY --from=builder /app/target/release/centarr /usr/local/bin
EXPOSE 3000
ENTRYPOINT ["/usr/local/bin/centarr"]