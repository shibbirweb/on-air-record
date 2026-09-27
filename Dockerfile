# The published container image: the service with its web UI compiled in, on a slim Debian.
#
# Only a Linux host can hand a container its microphone, by passing /dev/snd through. Docker Desktop on
# macOS and Windows runs containers in a virtual machine that has no sound hardware at all, so there the
# installers are the way to run it. docs/SETUP.md has the commands for both.
#
# Built by .github/workflows/release.yml for linux/amd64 and linux/arm64 on native runners, and by
# `make docker` locally. The context is the repository root, trimmed by .dockerignore.

# The UI is plain files, identical on every platform, so it is built once on the build machine's own
# architecture rather than emulated per target.
FROM --platform=$BUILDPLATFORM node:22-bookworm-slim AS ui
WORKDIR /src/frontend
COPY frontend/package.json frontend/package-lock.json ./
RUN npm ci
COPY frontend/ ./
RUN npm run build

FROM rust:1-bookworm AS build
RUN apt-get update \
    && apt-get install -y --no-install-recommends libasound2-dev pkg-config \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /src
COPY backend/ backend/
# Must land before cargo build: a release build embeds whatever frontend/dist holds at compile time.
COPY --from=ui /src/frontend/dist frontend/dist
WORKDIR /src/backend
# The cache mounts only speed up rebuilds on one machine. The binary is copied out because the target
# directory is a mount and is gone once this step ends.
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/backend/target \
    cargo build --release --locked \
    && cp target/release/on-air-record /usr/local/bin/on-air-record

FROM debian:bookworm-slim
# libasound2 is loaded at run time by cpal. tzdata because recordings are filed under the local calendar
# day, and without zone data TZ is ignored and every day silently becomes a UTC day. No CA bundle: the
# update check carries its own roots.
RUN apt-get update \
    && apt-get install -y --no-install-recommends libasound2 tzdata \
    && rm -rf /var/lib/apt/lists/*

# Never root. Membership of the image's audio group only helps when the host's audio group has the same
# number (29 on Debian, Ubuntu and Raspberry Pi OS); anywhere else the host's number is added at run time
# with --group-add, which is what the compose file does.
RUN useradd --system --uid 10001 --user-group --groups audio --home-dir /data --no-create-home on-air-record \
    && install -d -o on-air-record -g on-air-record /data

COPY --from=build /usr/local/bin/on-air-record /usr/local/bin/on-air-record

# OAR_CONTAINER is how the service knows to tell admins to pull a new image rather than to run an
# installer that would be lost when the container is recreated. TZ is deliberately not set: it would
# override a host's /etc/localtime mounted over the image's, which is UTC.
ENV OAR_CONTAINER=docker \
    OAR_DATA_DIR=/data \
    OAR_HOST=0.0.0.0 \
    OAR_PORT=8080

LABEL org.opencontainers.image.title="On Air Record" \
      org.opencontainers.image.description="Records a microphone around the clock and broadcasts it to browsers on the local network" \
      org.opencontainers.image.source="https://github.com/shibbirweb/on-air-record" \
      org.opencontainers.image.licenses="MIT"

USER on-air-record
WORKDIR /data
VOLUME /data
EXPOSE 8080

# The program probes its own /api/health, so the image needs no curl. It checks the service is answering,
# not that a microphone is recording: a recorder waiting for its first microphone is healthy, and marking
# it otherwise would have an orchestrator restart it in a loop for something a restart cannot fix. The
# thirty second start period covers the migrations and crash repair a start can run first.
HEALTHCHECK --interval=30s --timeout=10s --start-period=30s --retries=3 \
    CMD ["on-air-record", "health"]

# The program is PID 1 and handles SIGTERM itself, closing and indexing the segment in progress, so no
# init wrapper is needed. Give it time to: the compose file raises the default ten second grace period.
# Arguments after the image name go to the program, so `docker exec <name> on-air-record auth ...` and
# `docker run --rm ... auth ...` both reach the recovery commands.
ENTRYPOINT ["on-air-record"]
