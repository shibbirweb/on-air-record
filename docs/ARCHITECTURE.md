# Architecture

This document explains how On Air Record is put together, which layer owns which responsibility, and which
design patterns are used where. Read this before adding a feature so the new code lands in the right layer.

## 1. Two deployables, one process

The product is a single Rust binary plus a static React bundle that the binary serves. There is no separate
web server, no reverse proxy requirement, and no database server.

```
+--------------------------------------------------------------+
|  on-air-record binary                                         |
|                                                               |
|  axum HTTP server                                             |
|    /api/*        REST controllers                             |
|    /api/ws/*     WebSocket controllers                        |
|    /*            static file service (frontend/dist)          |
|                                                               |
|  background tasks                                             |
|    capture stream (cpal callback thread)                      |
|    segment recorder (tokio task)                              |
|    retention janitor (tokio interval task)                    |
|                                                               |
|  storage                                                      |
|    SQLite  data/on-air-record.sqlite                          |
|    PCM     data/recordings/<session>/<seq>.pcm                |
+--------------------------------------------------------------+
```

## 2. Backend layering

The backend follows a layered MVC style separation. Dependencies always point downwards, never sideways and
never upwards.

| Layer | Directory | Responsibility | May depend on |
| --- | --- | --- | --- |
| Routes | `src/routes` | Compose the axum router, mount middleware | Controllers |
| Controllers | `src/controllers` | Parse requests, call services, map to DTOs | Services, DTOs |
| DTOs | `src/dto` | Serde shapes crossing the HTTP boundary | Models |
| Services | `src/services` | Business rules, orchestration, background tasks | Repositories, audio, models |
| Audio | `src/audio` | Device access, framing, encoding, peaks | Models |
| Repositories | `src/repositories` | SQL statements, row mapping | Db, models |
| Db | `src/db` | Connection handling and migrations | - |
| Models | `src/models` | Domain entities, value objects | - |

The rule of thumb: a controller never writes SQL, a repository never knows about HTTP, and a service never
serialises JSON.

## 3. Design patterns in use

### Repository pattern (`src/repositories`)

Every table is reached through a repository object that owns SQL and row mapping. Controllers and services
speak in domain models such as `Segment` and `RecordingSession`, never in `rusqlite::Row`. This keeps the
storage engine swappable and makes the services testable with in memory doubles.

- `SettingsRepository` reads and writes the typed key value settings table.
- `SessionRepository` manages recording sessions (one per capture start).
- `SegmentRepository` indexes segment files and their peak envelopes.

### Service layer / facade (`src/services`, `src/app.rs`)

`AppState` is the composition root and the facade the controllers see. It is constructed once in `main.rs`,
wraps every service in `Arc`, and is handed to axum as shared state. Controllers therefore need one import
to reach the whole application.

### Observer / publish subscribe (`src/services/broadcast_hub.rs`)

`BroadcastHub` wraps a `tokio::sync::broadcast` channel of `AudioFrame` values. The capture service is the
single publisher. Live WebSocket sessions, the recorder, and the level meter are subscribers. Adding a new
consumer of live audio means subscribing to the hub, never modifying the capture code.

### Strategy (`src/audio/encoder.rs`)

`FrameEncoder` is a trait with `encode(&AudioFrame) -> Vec<u8>` plus format metadata. `PcmS16Encoder` is the
shipping implementation. A future Opus encoder implements the same trait and is selected by configuration,
so neither the recorder nor the streaming layer changes.

### State machine (`src/ws/session.rs`)

A streaming WebSocket session is a small state machine with three states, `Live`, `Playback`, and `Paused`,
driven by client control messages and by the playback cursor reaching the live edge. Keeping the transitions
in one enum avoids the tangle of booleans that DVR code usually grows.

### Actor per connection (`src/ws`)

Each WebSocket connection owns a tokio task, a `select!` loop over the inbound socket, the audio subscription,
and a pacing timer. Connections share no mutable state, so a slow listener cannot stall the recorder or the
other listeners. A lagging broadcast receiver is resynchronised rather than disconnected.

### Builder (`src/config`)

`AppConfig` is assembled from defaults, then environment variables, then CLI flags, with each stage returning
the partially built config. That ordering is expressed once and is easy to extend.

### Data transfer objects (`src/dto`)

Wire shapes are separate types from domain models and are `camelCase` on the wire. Renaming a database column
therefore never breaks the frontend contract by accident.

## 4. Concurrency model

- The `cpal` input callback runs on a realtime audio thread owned by the operating system. It must never
  block, allocate heavily, or touch SQLite. It only converts samples and does a non blocking send.
- A bounded `std::sync::mpsc` style channel (a `crossbeam` channel) hands frames from the audio thread to the
  async runtime. If the consumer falls behind, the oldest frames are dropped and a counter is incremented,
  which is reported as `droppedFrames` in the status API.
- Inside the runtime, `BroadcastHub` fans frames out to N subscribers. Broadcast lag is visible to each
  subscriber and is handled per connection.
- SQLite is accessed through a single connection behind a mutex. Write volume is one row per segment (once
  every few seconds), so contention is not a concern, and it avoids the write lock errors that concurrent
  connections cause on some filesystems.

## 5. Frontend layering

```
src/
  api/         Transport only. REST client and the WebSocket wrapper.
  store/       Zustand stores. All shared mutable state lives here.
  lib/         Framework free helpers: audio scheduler, ring buffer, formatters.
  hooks/       React bindings that glue stores, transports, and effects together.
  components/  Presentational components, including the shadcn/ui primitives.
  features/    Feature folders that assemble components into panels.
  pages/       Route level composition.
```

Rules:

- Components never call `fetch` directly. They call a store action, which calls the API client.
- The Web Audio graph is never rebuilt by React rendering. It lives in `lib/audio` and is owned by a hook
  that mounts it once, because tearing an `AudioContext` down on every render causes clicks and drift.
- Zustand stores are sliced by concern so that a waveform repaint does not re render the settings panel:

  | Store | Owns |
  | --- | --- |
  | `useStatusStore` | Polled service and capture status, plus the record and stop actions |
  | `useConnectionStore` | WebSocket liveness, the negotiated stream format, the input meter |
  | `useTransportStore` | Play state, mode, volume, and the transport actions |
  | `useDeviceStore` | The host input list and the current selection |
  | `useSettingsStore` | Runtime preferences, updated optimistically |
  | `useTimelineStore` | The visible window, coverage bands, recorded days and the fetched envelope |
  | `useStorageStore` | Disk usage and recent sessions |
- High frequency data (audio frames, level meters, playhead position at 60fps) bypasses React state and is
  pushed through refs and imperative canvas drawing. Only low frequency state changes go through Zustand.
- The imperative half of the transport is registered into the store rather than imported by it.
  `useStreamEngine` builds the socket and the audio graph, then calls `attachController`, so any component
  can call `seek` without being handed a WebSocket and the store stays free of side effects.

## 6. Data flow for the three core scenarios

### Listening live

1. UI opens `GET /api/ws/stream`.
2. Server subscribes the session to `BroadcastHub` and sends a `stream-info` control message.
3. Each capture frame is encoded, wrapped in the binary header, and pushed to the socket.
4. The browser scheduler queues each frame into an `AudioBufferSourceNode` at a computed start time, keeping
   a small jitter buffer so playback does not stutter.

### Seeking into history

1. UI sends `{"type":"seek","timestampMs":...}` on the same socket.
2. The session switches to the `Playback` state and asks `PlaybackService` for a segment cursor.
3. The cursor streams PCM from disk in frame sized chunks, paced by a tokio interval, flagged as historic.
4. When the cursor reaches the newest segment the session emits `switched-to-live` and rejoins the hub.

### Drawing the timeline

1. UI calls `GET /api/timeline/peaks?fromMs=..&toMs=..&buckets=..`.
2. `TimelineService` loads the overlapping segments, decodes their stored envelopes, and resamples them to
   the requested bucket count.
3. The canvas draws the envelope, the recorded coverage bands, the playhead, and the live edge.

The window is the only state the timeline really has, and every navigation is a transform of it: `panBy`
slides it, `zoomTo` rescales it about an anchor, `showDay` fits it to a day. The anchor is what stops
zooming from throwing away the moment the operator is looking at, so the toolbar passes the marker and the
scroll wheel passes the pointer. Any navigation to a specific moment also clears `followingLive`, or the
next range poll would drag the window back to the live edge and undo it.

## 7. Extension points

| I want to | Touch this |
| --- | --- |
| Add a REST endpoint | `dto`, `controllers`, `routes` |
| Offer a new way to navigate history | `TimelineService`, `dto/timeline_dto.rs`, `useTimelineStore`, a component under `features/timeline` |
| Add a stored preference | `models/settings.rs`, `repositories/settings_repository.rs`, settings DTO, frontend `settingsStore` |
| Support a new codec | Implement `FrameEncoder`, register it in `audio::encoder::build_encoder` |
| Add a live audio consumer | Subscribe to `BroadcastHub`, nothing else |
| Change the storage backend | Reimplement the repository traits, leave the services alone |

## 8. Testing

The backend carries unit tests next to the code they cover, run with `cargo test` from `backend/`. They
concentrate on the parts where a mistake is silent rather than loud: timestamp arithmetic in the frame
builder, byte offset maths in `Segment`, the segment index queries, envelope rendering, and the playback
cursor stepping across a real data directory of real PCM files.

The frontend tests the framework free half with `npm test` (Vitest) from `frontend/`: the binary frame
decoder, the timeline geometry, and the formatters. Components are not unit tested, because what would
break in them is canvas drawing and Web Audio scheduling, and neither is meaningfully exercised in jsdom.
Those are verified by running the service and listening.
