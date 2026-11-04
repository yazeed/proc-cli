# Build stage: a static musl binary, the same build as the Linux binaries of
# each GitHub release, so the runtime stage needs no C library
FROM rust:1-alpine AS builder

WORKDIR /app

RUN apk add --no-cache musl-dev binutils

# Cargo's job count: unset (every core) unless a local build passes
# --build-arg CARGO_BUILD_JOBS=4
ARG CARGO_BUILD_JOBS

# Copy manifests
COPY Cargo.toml Cargo.lock ./

# Create dummy main.rs to build dependencies
RUN mkdir src && echo "fn main() {}" > src/main.rs

# Build dependencies only (for caching)
RUN cargo build --release && rm -rf src

# Copy actual source code (build.rs sets the version; the context has no .git,
# so the image reports the plain Cargo version)
COPY build.rs ./
COPY src ./src

# Build the actual binary, and fail the build if it isn't static
RUN touch src/main.rs && cargo build --release \
    && readelf -d target/release/proc > /tmp/dynamic \
    && ! grep -q NEEDED /tmp/dynamic

# Runtime stage: distroless static has no shell and no package manager, only
# what a static binary uses: CA certificates, time zones and an /etc/passwd
# with root, nobody and nonroot. It runs as root, so with --pid=host proc can
# see and signal the host's processes; seeing the open files and sockets of a
# process another user owns also needs --cap-add SYS_PTRACE. proc reads
# processes, ports and open files from /proc, so it needs no other program
FROM gcr.io/distroless/static-debian13

# Copy binary from builder
COPY --from=builder /app/target/release/proc /usr/local/bin/proc

# Set the entrypoint
ENTRYPOINT ["/usr/local/bin/proc"]

# Default command (show help)
CMD ["--help"]

# Labels
LABEL org.opencontainers.image.title="proc"
LABEL org.opencontainers.image.description="Semantic CLI tool for process management. Target by port, PID, name or path."
LABEL org.opencontainers.image.source="https://github.com/yazeed/proc-cli"
LABEL org.opencontainers.image.licenses="MIT"
