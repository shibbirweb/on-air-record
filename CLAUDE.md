# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

A Rust service that captures a microphone on the host machine, records it continuously to disk, and
broadcasts it over WebSocket to browsers on the local network. The React UI plays the live feed and lets
you scrub back to any point in the retention window on a CCTV style timeline. One binary serves the API,
the WebSocket, and the compiled UI. No authentication, by design. See `README.md` for the feature status
checklist, which is kept current as work lands.

## Environment

Neither toolchain is on `PATH` by default in every shell here.

```sh
# Node is managed by nvm and pinned in frontend/.nvmrc
cd frontend && nvm use          # Node 22

# Cargo lives at ~/.cargo/bin, sourced from ~/.zshrc
. "$HOME/.cargo/env"
```

Node 16 is the fallback `node` on this machine and **will fail** the Vite build with a `styleText` export
error. If a build dies there, the wrong Node is active.

## Commands

```sh
# Backend, from backend/
cargo run                                    # API on :8080, reads ../frontend/dist
cargo test                                   # ~204 unit tests
cargo test day_bounds                        # single test by name substring
cargo test --lib services::playback_service  # one module
cargo clippy --all-targets -- -D warnings    # must be clean
cargo fmt

# Frontend, from frontend/
npm run dev      # Vite on :5173, proxies /api and the WebSocket to :8080
npm run build    # tsc -b then vite build, writes dist/, which a release backend embeds
npm test         # Vitest, ~90 tests
npm run lint     # oxlint
```

Full stack locally: `npm run build` once, then `cargo run`, then open `http://localhost:8080`. For hot
reload run the backend and `npm run dev` side by side and use `:5173`.

Runtime config is CLI flags or `OAR_*` environment variables (`OAR_PORT`, `OAR_DATA_DIR`,
`OAR_STATIC_DIR`, `OAR_LOG_LEVEL`). Useful for running a throwaway instance:
`OAR_PORT=8099 OAR_DATA_DIR=/tmp/oar-test cargo run`.

## Architecture

### Backend layering

Dependencies point one way only: `routes` -> `controllers` -> `services` -> `repositories` -> `db`, with
`models` and `audio` underneath. A controller never writes SQL, a repository never knows about HTTP, a
service never serialises JSON. `app.rs` is the composition root: it builds every service once and hands
them to axum as `AppState`, which is the facade handlers see.

`src/audio` is the only place that knows `cpal` exists. Everything above it works with `AudioFrame`
values, which is what keeps the rest testable without hardware.

### The three threads that matter

1. **The cpal callback**, owned by the OS audio thread. Downmixes to mono, applies gain, converts to i16,
   and does one non blocking send. It must never block, allocate in a loop, or panic.
2. **The recorder** (`services/recorder_service.rs`), its own OS thread, not tokio. Every step it takes is
   blocking: channel receive, file write, SQLite insert. It is also the *single* publisher into
   `BroadcastHub`, which is why live listeners hear exactly what is written to disk, in order.
3. **Tokio**, serving HTTP and one task per WebSocket connection.

`cpal::Stream` is not `Send` on every backend, so it lives on a dedicated thread for its whole life
(`audio/capture.rs`).

### How audio reaches a browser

Capture -> `FrameBuilder` cuts fixed 100 ms frames -> recorder publishes to `BroadcastHub` (a tokio
broadcast channel, observer pattern) and appends to a `SegmentWriter`. Each WebSocket session subscribes
to the hub. A slow listener lags its own receiver and is resynchronised; it cannot stall the recorder or
other listeners.

DVR playback is a `PlaybackCursor` reading segments off disk, paced by a tokio interval, flagged historic
in the frame header. When it runs out of disk while capture is running it emits `switched-to-live` and
rejoins the hub.

`ws/session.rs` is a three state machine (`Live`, `Playback`, `Paused`). Keep transitions in the enum.
DVR code tracked with booleans always grows a state nobody meant to allow.

### Storage invariants

```
data/recordings/<YYYY-MM-DD>/<session_id>/<sequence>.pcm
```

- **Raw headerless PCM is deliberate.** The byte offset of any timestamp is one multiplication, so seeking
  costs no index and no decoder warm up. The price is size, bounded by the retention window. Compression
  is isolated behind the `FrameEncoder` strategy trait for later.
- **Only closed segments are indexed and seekable.** The last few seconds of live audio are not scrubbable
  until the segment rolls over. This surprises people; it is not a bug.
- **`day` is the local calendar day the segment started in**, stored rather than computed so it survives a
  host timezone change and always names the directory the file is actually in. `util/day.rs` is the single
  place that converts between instants and days; do not reimplement date maths elsewhere.
- **Peak envelopes** are one byte per 100 ms bucket stored on the segment row. An hour of waveform is 36 KB
  to draw instead of 338 MB. Computed once during recording, while the samples are already in cache.
- Retention deletes the file first, then the index row, so a crash between them leaves a repairable
  orphan row rather than a file nothing remembers.

### Wire protocol

One WebSocket carries both live and DVR. Binary messages are audio, a fixed 24 byte little endian header
(`ws/protocol.rs`) plus payload. Text messages are JSON control (`ws/messages.rs`). The header repeats
sample rate and channels on every frame so the input device can change mid stream.

`frontend/src/lib/audio/frameCodec.ts` is the mirror of `ws/protocol.rs`. **Change one and you must change
the other**, and both have tests that encode and decode the documented layout.

### Frontend

`api/` is transport only, `store/` holds all shared state as Zustand slices, `lib/` is framework free
logic, `features/` assembles panels. Components call store actions, never `fetch`.

Two rules that the audio path depends on:

- **The Web Audio graph never enters the render cycle.** `AudioEngine` is built once via lazy `useState`
  in `useStreamEngine` and lives in that hook. Rebuilding an `AudioContext` on a render clicks audibly and
  loses the scheduling clock.
- **High frequency data bypasses React.** Frames, meters, and the 60 fps playhead go through refs and
  imperative canvas drawing. Only low frequency state goes through Zustand. The timeline and waveform read
  the playhead through a `getPlayheadMs()` callback so a moving playhead triggers no renders.

The imperative half of the transport registers itself into `useTransportStore` via `attachController`, so
any component can call `seek` without being handed a socket.

Playback position comes from the audio clock (`AudioEngine.currentPlayheadMs()`), not from the newest
frame received, because the newest frame is a jitter buffer ahead of what the listener hears.

## Conventions

Org standards in the managed settings apply here (commit format, no em dash, braces on every `if`,
semicolons and trailing commas in TS). Project specifics on top of those:

- There is no Jira project, so commits use a sequential `OAR-N` ticket: `feat:[OAR-15] add opus encoding`.
- No `unwrap()` or `expect()` outside `main.rs`, tests, and mutex locks. Everything else returns
  `AppResult<T>`.
- Doc comments explain **why**, not what. The existing code is dense with rationale; match that.
- Long argument lists become a parameter struct (`SegmentLocation`, `RecorderContext`).
- Sources are ASCII only. Use `&middot;` and friends in JSX rather than literal glyphs.
- **Never edit a version by hand.** `backend/Cargo.toml` is the source of truth and the same number is
  recorded in four files; `node scripts/version.mjs bump` picks the next one and moves all of them, and
  `version.mjs check` is a CI job. A release tag is validated against the manifest, not trusted.
- Repository tooling in `scripts/` is Node (`.mjs`, no dependencies), not Python. Node is already required
  to build the UI, so it is the one toolchain the workflows can assume.

## Testing

Backend tests sit next to the code. They target the arithmetic that fails silently rather than loudly:
frame timestamping and drift correction, segment byte offsets, the index queries, envelope rendering, and
the playback cursor walking a real directory of real PCM across a recording gap. Several build a real temp
data directory plus an in memory SQLite, because the interaction between the two is the thing worth
testing.

Frontend tests cover only `lib/`. Components are not unit tested, because what would break in them is
canvas drawing and Web Audio scheduling and neither is meaningfully exercised in jsdom.

Verify audio changes by running the service and listening. Browsers require a user gesture before audio
starts, so headless checks cannot confirm playback. A useful trick for exercising the DVR without waiting
hours: insert PCM files and matching segment rows directly into SQLite for past days, and raise
`retentionHours` first or the janitor will prune them within a minute.

## Platform

No platform specific code beyond the SIGTERM handler: `cpal` covers CoreAudio, ALSA and WASAPI, and
`rusqlite` bundles SQLite. What has actually been exercised differs from what compiles:

- **macOS**: built and run end to end, including capture, DVR playback and WAV export.
- **Linux x86_64**: built and run end to end in a container, full test suite, WAV export validated with
  `ffprobe`. Needs `libasound2-dev` and `pkg-config`.
- **Windows x86_64**: built and tested by CI only. Nobody has run it on a Windows desktop. Do not claim
  otherwise. Cross compiling from macOS cannot close this, because bundled SQLite needs a Windows C
  toolchain, which is why `.github/workflows/ci.yml` runs the suite on a Windows runner.

## Packaging

A release build embeds `frontend/dist` into the executable, so `npm run build` must precede
`cargo build --release` or the binary ships a stale UI. `backend/build.rs` creates the directory when it
is missing and emits `cargo:rerun-if-changed` for it, because a proc macro cannot tell Cargo what it read.

At runtime `routes::static_files` prefers a `--static-dir` containing `index.html`, then the embedded
copy, then the build instructions page. Debug builds read from disk either way.

## Further reading

`docs/ARCHITECTURE.md` (layers and patterns), `docs/AUDIO_PIPELINE.md` (capture, framing, storage, DVR
timing, latency budget), `docs/API.md` (REST and WebSocket contract), `docs/DEVELOPMENT.md` (setup and
troubleshooting).
