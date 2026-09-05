# On Air Record

A cross platform audio broadcast and DVR service written in Rust, with a browser based control room UI
built in React. The service captures audio from a microphone on the host machine, streams it live to any
browser on the local network, records it continuously to disk, and lets you scrub back to any point in the
past on a CCTV style timeline.

Think of it as a small FM station plus a digital video recorder for sound:

- The host machine is the studio (it owns the microphone).
- The browser is the radio receiver (it plays the live signal).
- The timeline is the tape (you can rewind, play back history, and jump back to live).

## Table of contents

- [Highlights](#highlights)
- [How it works](#how-it-works)
- [Requirements](#requirements)
- [Quick start](#quick-start)
- [Configuration](#configuration)
- [Project layout](#project-layout)
- [Feature status](#feature-status)
- [Testing](#testing)
- [Documentation](#documentation)
- [Troubleshooting](#troubleshooting)
- [License](#license)

## Highlights

- **Single binary service.** The Rust backend serves the REST API, the WebSocket audio stream, and the
  compiled web UI from one process on macOS, Linux (desktop and headless server), and Windows.
- **Live broadcast.** Captured audio is fanned out to every connected browser through a WebSocket, so
  several listeners on the LAN hear the same signal at the same time.
- **Always on recording.** Audio is written to disk in fixed length segments and indexed in SQLite, so the
  recorder keeps running even when nobody is listening.
- **DVR timeline.** Seek to any timestamp inside the retention window, play history at real time pace, and
  return to the live edge with one click.
- **Day by day history.** Recordings are filed on disk by calendar day and indexed the same way, so you can
  pick a day and play from any point in it, the way you would scrub a CCTV recording.
- **Waveform with timeline.** Amplitude peaks are computed while recording and stored alongside each
  segment, so the UI can draw the waveform for both live audio and history.
- **Device picker.** All host input devices are enumerated at runtime, and the selected device is persisted
  in SQLite so the service comes back on the same microphone after a restart.
- **No authentication.** Designed for a trusted local network, so there is nothing to log into.

## How it works

```
 host machine                                          browser (LAN)
+-----------------------------------------+           +----------------------------+
|  cpal input stream (selected mic)       |           |  React + Zustand UI        |
|            |                            |           |            |               |
|            v                            |           |            v               |
|  CaptureService  --(mono i16 frames)-->  BroadcastHub  --WS-->  Web Audio player  |
|            |                            |           |            ^               |
|            v                            |           |            |               |
|  SegmentRecorder --> data/recordings/*.pcm          |     timeline scrubber      |
|            |                            |           |            |               |
|            v                            |    REST   |            v               |
|  SQLite (sessions, segments, peaks, settings) <----------- peaks and device API   |
+-----------------------------------------+           +----------------------------+
```

1. `CaptureService` opens the selected input device with `cpal`, downmixes to mono, and converts every
   callback into 16 bit PCM frames stamped with a wall clock timestamp.
2. Frames go to a `BroadcastHub` (a tokio broadcast channel) that every live listener subscribes to, and to
   the `SegmentRecorder`, which appends them to the current segment file.
3. When a segment reaches its configured length the recorder closes it, computes its peak envelope, and
   writes one row to SQLite describing the time range, the file, and the envelope blob.
4. A browser asks for history over the same WebSocket. The `PlaybackService` reads the matching segments
   from disk and paces them out at real time, then hands the client over to the live hub once it catches up.

## Requirements

- Rust 1.82 or newer (stable toolchain).
- Node.js 18 or newer and npm, only needed to build the web UI.
- Linux additionally needs ALSA development headers: `sudo apt install libasound2-dev pkg-config`.
- macOS and Windows need no extra audio packages (CoreAudio and WASAPI are used through `cpal`).

Portability comes from the dependencies rather than from conditional code: `cpal` covers CoreAudio, ALSA
and WASAPI behind one API, `rusqlite` bundles SQLite, and there is no platform specific code beyond the
SIGTERM handler. The service has so far been built and run end to end on macOS. Linux and Windows builds
are expected to work but have not been exercised yet, which is what the release workflow in milestone 6 is
for. Cross compiling from macOS is not a substitute, because the bundled SQLite needs a C toolchain for
the target.

## Quick start

```bash
# 1. build the web UI
cd frontend
npm install
npm run build

# 2. run the service
cd ../backend
cargo run --release
```

Then open `http://localhost:8080` on the host, or `http://<host-lan-ip>:8080` from any other machine on the
same network.

For development with hot reload, run the backend and the Vite dev server side by side:

```bash
cd backend && cargo run          # terminal 1, API on :8080
cd frontend && npm run dev       # terminal 2, UI on :5173, proxied to :8080
```

## Configuration

Every option can be set with a CLI flag or an environment variable. CLI flags win.

| Flag | Environment variable | Default | Description |
| --- | --- | --- | --- |
| `--host` | `OAR_HOST` | `0.0.0.0` | Address the HTTP server binds to |
| `--port` | `OAR_PORT` | `8080` | HTTP port |
| `--data-dir` | `OAR_DATA_DIR` | `./data` | Root directory for recordings and the database |
| `--static-dir` | `OAR_STATIC_DIR` | `../frontend/dist` | Compiled web UI to serve |
| `--log-level` | `OAR_LOG_LEVEL` | `info` | `error`, `warn`, `info`, `debug`, or `trace` |

Runtime preferences (input device, segment length, retention window, gain, auto start) live in the SQLite
`settings` table and are editable from the UI.

## Project layout

```
on-air-record/
  backend/            Rust service (API, capture, recorder, playback)
    src/
      audio/          Capture, encoding, peaks, segment writer
      config/         CLI and environment configuration
      controllers/    HTTP and WebSocket request handlers
      db/             Connection pool and schema migrations
      dto/            Request and response payloads
      models/         Domain entities
      repositories/   SQLite data access
      routes/         Router composition
      services/       Application services and orchestration
      ws/             Streaming protocol and session actors
  frontend/           React 19 + Vite + Tailwind + shadcn/ui + Zustand
    src/
      api/            REST client and WebSocket transport
      components/     Reusable UI, including shadcn primitives
      features/       Feature modules (broadcast, timeline, devices, settings)
      hooks/          Shared React hooks
      lib/            Audio scheduling, formatting, utilities
      store/          Zustand stores
  docs/               Developer documentation
```

## Feature status

Legend: `[x]` done, `[~]` in progress, `[ ]` planned.

### Milestone 1: foundations

- [x] Repository scaffolding, README, and developer docs
- [x] Configuration layer (CLI flags plus environment variables)
- [x] SQLite connection handling and versioned schema migrations
- [x] Repository layer for settings, sessions, and segments
- [x] Structured logging and graceful shutdown

### Milestone 2: audio capture

- [x] Cross platform input device enumeration
- [x] Microphone capture through `cpal` with automatic format negotiation
- [x] Downmix to mono and conversion to 16 bit PCM
- [x] Software gain control
- [x] Hot swap of the input device without restarting the service
- [x] Capture level metering (RMS and peak)

### Milestone 3: recording and storage

- [x] Continuous segmented recorder writing raw PCM to disk
- [x] Day based storage layout, `recordings/<YYYY-MM-DD>/<session>/`
- [x] Segment index in SQLite with precise time ranges
- [x] Waveform peak envelope computed while recording
- [x] Retention janitor that prunes segments past the retention window
- [x] Storage usage reporting
- [ ] Opus compression for segments instead of raw PCM
- [ ] Export a time range as a downloadable WAV file

### Milestone 4: streaming

- [x] Binary WebSocket framing protocol for PCM audio
- [x] Live fan out to multiple simultaneous listeners
- [x] DVR playback of stored segments paced at real time
- [x] Seek to an arbitrary timestamp inside the retention window
- [x] Automatic hand off from history back to the live edge
- [x] Playback transport controls (pause, resume, jump to live)
- [ ] Variable speed playback
- [ ] Plain HTTP progressive stream for non JavaScript clients

### Milestone 5: web UI

- [x] Vite, React, TypeScript, Tailwind, and shadcn/ui setup
- [x] Zustand stores for connection, transport, devices, and settings
- [x] Web Audio playback scheduler with jitter buffer
- [x] Live waveform visualiser
- [x] Scrubbable CCTV style timeline with recorded range shading
- [x] Day picker that jumps the timeline to a chosen day and plays from its first moment
- [x] Input device selector backed by the settings API
- [x] Recorder and listener status panel
- [x] Settings panel (gain, retention, segment length, auto start)
- [x] Dark first responsive layout
- [ ] Timeline bookmarks and named markers
- [ ] Multi track view when several sessions overlap

### Milestone 6: packaging

- [ ] Embed the compiled UI into the binary for a single file distribution
- [ ] Release workflow producing macOS, Linux, and Windows artifacts
- [ ] Optional systemd unit and Windows service wrapper

## Testing

```bash
cd backend  && cargo test                              # 110 unit tests
cd backend  && cargo clippy --all-targets -- -D warnings
cd frontend && npm test                                # Vitest, framework free logic
cd frontend && npm run lint
```

Backend tests concentrate on the arithmetic that fails quietly rather than loudly: frame timestamping and
drift correction, byte offsets inside a segment, the segment index queries, envelope rendering, and the
playback cursor walking a real directory of real PCM files across a recording gap. Frontend tests cover the
binary frame decoder, the timeline geometry, and the formatters.

What the automated tests do not cover, and what to check by hand after a change to the audio path:

- Press play and confirm sound comes out, since browsers only start audio from a user gesture.
- Click somewhere in a shaded band on the timeline and confirm playback jumps there.
- Pick an earlier day from the day selector and confirm it frames that day and starts playing it.
- Let it play forward to the live edge and confirm the badge flips back to `on air` on its own.

## Documentation

- [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md): layers, design patterns, and data flow.
- [docs/AUDIO_PIPELINE.md](docs/AUDIO_PIPELINE.md): capture, framing, storage format, and DVR timing.
- [docs/API.md](docs/API.md): REST endpoints and the WebSocket protocol.
- [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md): local setup, conventions, and troubleshooting.

## Troubleshooting

The two most common first run problems:

- **The page says the web UI has not been built.** The backend is running but `frontend/dist` is missing.
  Run `npm install && npm run build` in `frontend/`.
- **No devices are listed on macOS.** The first run triggers a microphone permission prompt. If it was
  denied, enable it under System Settings, Privacy and Security, Microphone, then restart the service.

[docs/DEVELOPMENT.md](docs/DEVELOPMENT.md) has the rest.

## License

MIT
