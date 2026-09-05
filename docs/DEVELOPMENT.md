# Development guide

## Prerequisites

| Tool | Version | Notes |
| --- | --- | --- |
| Rust | 1.75+ | Install with [rustup](https://rustup.rs) |
| Node.js | 18+ | 20 or 22 recommended |
| npm | 9+ | Ships with Node |

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

The backend serves `../frontend/dist` by default. Point `--static-dir` somewhere else if you relocate it.

## Useful commands

| Command | What it does |
| --- | --- |
| `cargo check` | Fast type check of the backend |
| `cargo clippy --all-targets -- -D warnings` | Lint, treated as errors |
| `cargo fmt` | Format Rust code |
| `cargo test` | Run backend unit tests |
| `npm run build` | Type check with `tsc` and build the UI |
| `npm run lint` | ESLint over the frontend |

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
```

The ticket goes in square brackets immediately after the colon with no space. One logical change per commit.
Do not add co author trailers.

## Project data directory

```
data/
  on-air-record.sqlite      settings, sessions, segment index
  recordings/
    1/                      session id
      000000.pcm            segment sequence, zero padded
      000001.pcm
```

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

**Playback stutters over Wi-Fi.** Raise the jitter buffer in the settings panel. 150 ms is the default and
300 ms is comfortable on a congested network.

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
