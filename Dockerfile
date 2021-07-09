FROM rust:1.53.0 as build
ENV PKG_CONFIG_ALLOW_CROSS=1

WORKDIR /usr/src/nails
COPY ./Cargo.toml ./Cargo.toml
COPY ./src ./src

RUN cargo install --path .

FROM gcr.io/distroless/cc-debian10

COPY --from=build /usr/local/cargo/bin/nails /usr/local/bin/nails

CMD ["nails"]