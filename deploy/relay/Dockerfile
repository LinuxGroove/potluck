# iroh-relay, built from crates.io at the version the game uses (iroh 1.5).
FROM rust:1-bookworm AS build
RUN cargo install iroh-relay@1.5.0 --features server --locked

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates libcap2-bin && rm -rf /var/lib/apt/lists/*
COPY --from=build /usr/local/cargo/bin/iroh-relay /usr/local/bin/iroh-relay
# ports 80 and 443 without running as root
RUN setcap cap_net_bind_service=+ep /usr/local/bin/iroh-relay \
 && useradd --system --home /var/lib/iroh-relay relay && mkdir -p /var/lib/iroh-relay/certs && chown -R relay /var/lib/iroh-relay
USER relay
ENTRYPOINT ["iroh-relay", "--config-path", "/etc/iroh-relay/relay.toml"]
