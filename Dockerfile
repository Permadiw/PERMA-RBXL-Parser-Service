FROM rust:1-bookworm AS builder
WORKDIR /app
COPY Cargo.toml Cargo.toml
COPY src src
RUN cargo build --release

FROM debian:bookworm-slim
RUN useradd --system --uid 10001 parser
WORKDIR /app
COPY --from=builder /app/target/release/perma-rbxl-parser /usr/local/bin/perma-rbxl-parser
USER parser
ENV PORT=8080
EXPOSE 8080
CMD ["perma-rbxl-parser"]
