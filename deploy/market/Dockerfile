# potluck-share: built from the workspace, run with git (remixes and
# suggestions copy history with it) and nothing else.
FROM rust:1-bookworm AS build
WORKDIR /src
COPY . .
RUN cargo build --release -p potluck-share

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends git ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=build /src/target/release/potluck-share /usr/local/bin/potluck-share
RUN useradd --system --home /var/lib/potluck-share potluck && mkdir -p /var/lib/potluck-share && chown potluck /var/lib/potluck-share
USER potluck
EXPOSE 47180
ENTRYPOINT ["potluck-share"]
