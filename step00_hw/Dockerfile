# Build the binaries statically, and then run them from a separate container.
FROM alpine AS build

# Intentionally not installing `cargo`, etc. But it does need `clang`.
RUN apk add rustup clang musl

# Init local rust via `rustup`, including the static target.
RUN rustup-init -y -v -t x86_64-unknown-linux-musl --no-modify-path 
RUN mv /root/.cargo/bin/* /usr/local/bin/

# Build the project.
COPY ./code /code
WORKDIR /code
RUN cargo build --release --target x86_64-unknown-linux-musl

# Prepare the static binary.
RUN cp ./target/x86_64-unknown-linux-musl/release/hw /

# The resulting container with the static binary only.
FROM scratch
COPY --from=build /hw /hw
ENTRYPOINT ["/hw"]
