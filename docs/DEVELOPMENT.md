# Development guide

## Prerequisites

| Tool | Version | Notes |
| --- | --- | --- |
| Rust | 1.82+ | Install with [rustup](https://rustup.rs) |
| Node.js | 18+ | 22 is what the project is built and tested against |
| npm | 9+ | Ships with Node |

`frontend/.nvmrc` pins the major version, so `nvm use` inside `frontend/` picks the right toolchain:

```bash
cd frontend
nvm use          # reads .nvmrc, installs with `nvm install` if it is missing
```

Platform specific audio dependencies:

```bash
# Debian, Ubuntu, Raspberry Pi OS
sudo apt install build-essential pkg-config libasound2-dev

# Fedora
sudo dnf install alsa-lib-devel

# macOS and Windows
# nothing extra, CoreAudio and WASAPI are part of the system
```

A headless Linux server still needs a real or virtual input device. `arecord -l` should list something. For
testing without hardware, create a loopback device with `sudo modprobe snd-aloop`.

## Running in development

Two terminals:

```bash
# terminal 1: backend with debug logging, API and WebSocket on :8080
cd backend
OAR_LOG_LEVEL=debug cargo run

# terminal 2: Vite dev server on :5173, proxies /api to :8080
cd frontend
npm run dev
```

Open `http://localhost:5173`. The proxy in `vite.config.ts` forwards both REST and WebSocket traffic, so the
frontend code always talks to same origin `/api` paths and never needs a base URL.

## Running as it ships

```bash
cd frontend && npm run build
cd ../backend && cargo run --release
# http://localhost:8080
```

**Order matters.** A release build embeds `frontend/dist` into the executable at compile time, so the UI
build has to happen first. Build the backend against a stale `dist` and the binary ships a stale UI.

### How the UI is found at runtime

`routes::static_files` tries three things in order:

1. `--static-dir` pointing at a directory that contains `index.html`, served from disk. This is the
   default in development, where `--static-dir` resolves to `../frontend/dist`, and it is also the escape
   hatch for swapping the UI on a deployed machine without a recompile.
2. The copy embedded in the binary, which is what a downloaded release serves.
3. The build instructions page, when there is neither.

Debug builds do not really embed anything: `rust-embed` reads the same directory from disk, so editing the
UI never means recompiling the backend. Only a release build carries the assets.

`backend/build.rs` exists for two reasons that are easy to trip over:

- It creates `frontend/dist` if it is missing, because the embed macro will not compile against a folder
  that does not exist and a fresh clone has no `dist`.
- It emits `cargo:rerun-if-changed` for that directory. A proc macro cannot tell Cargo what it read, so
  without this a `cargo build --release` after a UI only change would reuse the previous binary and its
  previous UI, with nothing to indicate anything was wrong.

## Useful commands

| Command | What it does |
| --- | --- |
| `cargo check` | Fast type check of the backend |
| `cargo clippy --all-targets -- -D warnings` | Lint, treated as errors |
| `cargo fmt` | Format Rust code |
| `cargo test` | Run backend unit tests |
| `npm run build` | Type check with `tsc` and build the UI |
| `npm test` | Vitest over the framework free frontend logic |
| `npm run lint` | oxlint over the frontend |
| `cargo build --release --target <triple>` | What the release workflow runs per platform |

## Continuous integration and releases

[`.github/workflows/ci.yml`](../.github/workflows/ci.yml) runs on every push and pull request: the
backend matrix does `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings` and `cargo test` on
Ubuntu, macOS and Windows, and a separate job lints, tests and builds the frontend. Ubuntu installs
`libasound2-dev` because `cpal` links against ALSA there.

The Windows leg is the point of the matrix. It cannot be reproduced locally on macOS, so a change that
compiles here can still fail there and CI is the only warning you will get.

[`.github/workflows/release.yml`](../.github/workflows/release.yml) runs on a `v*` tag and produces one
archive per target with a `sha256` alongside. The web UI is built once in its own job and shared, so all
four archives ship identical assets. Trigger it manually with `workflow_dispatch` to rehearse the build
without publishing anything.

## Code conventions

Shared:

- No em dash characters anywhere in code, comments, docs, or commit messages.
- Name variables after the domain concept, so `segmentId` rather than `id` when the type is ambiguous.
- Defend against missing external data with a default rather than an unwrap or a throw.

Rust:

- `if` always uses braces, even for a single statement.
- No `unwrap()` or `expect()` outside `main.rs`, tests, and mutex locks that cannot be poisoned in practice.
  Everything else returns `AppResult<T>`.
- Public items in `services`, `repositories`, and `audio` carry a doc comment explaining the why, not the what.
- Long argument lists become a parameter struct.

TypeScript:

- Semicolons at the end of statements, trailing comma after the last item in multi line literals.
- `type` for object shapes, `interface` only when declaration merging is actually needed.
- No `any`. Use `unknown` and narrow it.
- Components are function components with typed props, no default exports except for route level pages.

## Commit conventions

```
feat:[OAR-12] add retention janitor
fix:[OAR-19] correct segment offset rounding when seeking
docs:[OAR-3] document the websocket protocol
test:[OAR-21] cover the playback cursor across a recording gap
```

The ticket goes in square brackets immediately after the colon with no space. One logical change per commit.
Do not add co author trailers.

## Project data directory

```
data/
  on-air-record.sqlite      settings, sessions, segment index
  recordings/
    2026-09-05/             local calendar day
      1/                    session id
        000000.pcm          segment sequence, zero padded
        000001.pcm
      2/                    a second session on the same day
        000000.pcm
    2026-09-04/
      ...
```

Grouping by day first means a day's audio can be archived or dropped as a unit, and it survives the
recorder being stopped and restarted several times within one day.

Deleting `data/` resets the service completely. Deleting only `recordings/` while keeping the database leaves
orphaned index rows, which the janitor cleans up on the next pass.

## Troubleshooting

**No devices are listed.** On Linux check that the user is in the `audio` group and that `arecord -l` sees the
card. On macOS the first run triggers a microphone permission prompt. If it was denied, enable it under
System Settings, Privacy and Security, Microphone, then restart the service.

**Capture starts and immediately errors.** Another application may hold the device exclusively. The error
message from the host API is passed through in `capture.error` and in the server log at `warn` level.

**The browser shows a connected socket but plays nothing.** Browsers require a user gesture before audio can
start. Click the play control. If it still silent, check that the level meter moves, which tells you whether
the problem is capture side or playback side.

**Playback stutters over Wi-Fi.** The browser holds a 150 ms jitter buffer, set by `DEFAULT_JITTER_SECONDS`
in `frontend/src/lib/audio/audioEngine.ts`. Raising it to 300 ms is comfortable on a congested network at
the cost of the same amount of extra latency. The engine also counts every resynchronisation, so a rising
`resyncs` in `AudioEngine.stats()` is the signal that the buffer is too small for the link.

**The timeline is empty although recording is running.** Only closed segments are indexed. Wait one segment
length, 10 seconds by default, or stop capture to flush the open segment.

## Testing the audio path without a microphone

```bash
# macOS, route system audio into an input with BlackHole
brew install blackhole-2ch

# Linux, create a null sink and a loopback source
pactl load-module module-null-sink sink_name=oar
pactl load-module module-loopback source=oar.monitor
```

Both appear in the device list like any other input.
