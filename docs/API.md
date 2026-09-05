# API reference

Base URL: `http://<host>:8080`. All REST payloads are JSON with `camelCase` keys. There is no
authentication, by design, so the service should only be exposed to a trusted network.

## Error format

Every failing request returns the same envelope with an appropriate status code.

```json
{
  "error": {
    "code": "not_found",
    "message": "no segment covers timestamp 1757030400000"
  }
}
```

| Code | Status | Meaning |
| --- | --- | --- |
| `bad_request` | 400 | Malformed or out of range parameters |
| `not_found` | 404 | Unknown device, session, or timestamp |
| `conflict` | 409 | Action not valid in the current state, for example starting an active capture |
| `audio_error` | 503 | The host audio system rejected the operation |
| `internal` | 500 | Unexpected failure, details are in the server log |

## Service

### `GET /api/health`

```json
{ "status": "ok", "version": "0.1.0", "uptimeMs": 1834221 }
```

### `GET /api/status`

The one call the UI polls for the state of the world.

```json
{
  "capture": {
    "state": "recording",
    "sessionId": 4,
    "deviceId": "MacBook Pro Microphone",
    "deviceName": "MacBook Pro Microphone",
    "sampleRate": 48000,
    "channels": 1,
    "frameMs": 100,
    "startedAtMs": 1757030400000,
    "droppedFrames": 0,
    "error": null
  },
  "levels": { "rms": 0.062, "peak": 0.31 },
  "listeners": 2,
  "serverTimeMs": 1757034000000,
  "liveEdgeMs": 1757033999900
}
```

`capture.state` is one of `idle`, `starting`, `recording`, or `error`.

### `POST /api/capture/start`

Starts capture on the currently selected device. Returns the same body as `GET /api/status`.
Responds `409` if capture is already running.

### `POST /api/capture/stop`

Stops capture, closes and indexes the open segment. Returns the status body.

## Devices

### `GET /api/devices`

```json
{
  "devices": [
    {
      "id": "MacBook Pro Microphone",
      "name": "MacBook Pro Microphone",
      "isDefault": true,
      "isSelected": true,
      "channels": 1,
      "sampleRate": 48000,
      "available": true
    }
  ]
}
```

The `id` is the host device name, which is the only identifier `cpal` exposes on all three platforms. A
device that is stored in settings but not currently present is still listed with `available: false`, so the
UI can show that the configured microphone is unplugged.

### `POST /api/devices/select`

```json
{ "deviceId": "Scarlett Solo USB" }
```

Persists the choice and, if capture is running, restarts it on the new device. A device change always starts
a new recording session because the sample rate may differ. Returns the status body.

## Settings

### `GET /api/settings`

```json
{
  "inputDeviceId": "Scarlett Solo USB",
  "gain": 1.0,
  "segmentSeconds": 10,
  "retentionHours": 24,
  "autoStart": true,
  "frameMs": 100
}
```

### `PATCH /api/settings`

Accepts any subset of the settings object and returns the full updated object.

```json
{ "gain": 1.5, "retentionHours": 48 }
```

| Field | Type | Range | Applied |
| --- | --- | --- | --- |
| `inputDeviceId` | string or null | any device id | On next capture start |
| `gain` | number | 0.0 to 4.0 | Immediately |
| `segmentSeconds` | integer | 5 to 300 | On next segment rollover |
| `retentionHours` | integer | 1 to 8760 | On next janitor pass, which runs every minute |
| `autoStart` | boolean | | On next service start |
| `frameMs` | integer | 20 to 500 | On next capture start |

## Timeline

### `GET /api/timeline/range`

The extent of the recorded material and where the gaps are.

```json
{
  "earliestMs": 1757030400000,
  "latestMs": 1757034000000,
  "liveEdgeMs": 1757034000000,
  "serverTimeMs": 1757034000120,
  "coverage": [
    { "startMs": 1757030400000, "endMs": 1757031900000 },
    { "startMs": 1757032200000, "endMs": 1757034000000 }
  ]
}
```

Adjacent segments are merged into coverage bands, with a gap declared when segments are more than one frame
apart.

### `GET /api/timeline/days`

Every calendar day that holds recordings, newest first. This is what the day picker is built from, so it
never offers a date with nothing behind it.

```json
{
  "days": [
    {
      "day": "2026-09-05",
      "startMs": 1757030400000,
      "endMs": 1757052040000,
      "dayStartMs": 1757008800000,
      "dayEndMs": 1757095200000,
      "segmentCount": 342,
      "bytes": 328212480,
      "recordedMs": 3420000
    }
  ]
}
```

| Field | Meaning |
| --- | --- |
| `day` | Local calendar day on the host, `YYYY-MM-DD` |
| `startMs` / `endMs` | First and last moment actually recorded that day |
| `dayStartMs` / `dayEndMs` | Local midnight bounds, for framing the whole day |
| `segmentCount` / `bytes` | What is on disk for the day |
| `recordedMs` | Audio actually captured, which is less than `endMs - startMs` whenever the recorder was stopped part way through the day |

Days are **local to the host machine**, not UTC, and a segment belongs to the day it *started* in. All
sessions recorded on one day are reported as one entry, however many times the recorder was restarted.

### `GET /api/timeline/peaks`

| Query parameter | Required | Default | Notes |
| --- | --- | --- | --- |
| `fromMs` | yes | | Window start, epoch milliseconds |
| `toMs` | yes | | Window end, must be greater than `fromMs` |
| `buckets` | no | 1000 | 16 to 4000, the number of output columns |

```json
{
  "fromMs": 1757030400000,
  "toMs": 1757034000000,
  "bucketMs": 3600,
  "peaks": [0, 0, 12, 48, 91, 63, 0]
}
```

Each value is `0..255`. A zero means either silence or no recording, so the UI reads `coverage` from
`/api/timeline/range` to tell the two apart.

## Sessions and storage

### `GET /api/sessions`

```json
{
  "sessions": [
    {
      "id": 4,
      "deviceId": "Scarlett Solo USB",
      "deviceName": "Scarlett Solo USB",
      "sampleRate": 48000,
      "channels": 1,
      "startedAtMs": 1757030400000,
      "endedAtMs": null,
      "segmentCount": 342,
      "bytes": 328212480
    }
  ]
}
```

### `GET /api/storage`

```json
{
  "bytes": 328212480,
  "segmentCount": 342,
  "oldestMs": 1757030400000,
  "newestMs": 1757034000000,
  "retentionHours": 24,
  "dataDir": "/Users/me/on-air-record/data"
}
```

## WebSocket `GET /api/ws/stream`

One socket carries both the live broadcast and DVR playback. Binary messages are audio frames in the format
described in [AUDIO_PIPELINE.md](AUDIO_PIPELINE.md). Text messages are JSON control messages.

### Server to client control messages

| `type` | Payload | Meaning |
| --- | --- | --- |
| `stream-info` | `sampleRate`, `channels`, `frameMs`, `mode`, `serverTimeMs`, `liveEdgeMs`, `earliestMs`, `capturing` | Sent on connect, and again if capture stops while a listener is attached |
| `mode` | `mode`, `positionMs` | The session changed between `live`, `playback`, and `paused` |
| `switched-to-live` | `timestampMs` | Playback caught up with the live edge |
| `gap` | `fromMs`, `toMs` | No recording exists in this range, playback skipped it |
| `end-of-recording` | `timestampMs` | Playback reached the newest data while capture is stopped |
| `level` | `rms`, `peak` | Input meter, emitted about ten times per second in live mode |
| `error` | `code`, `message` | Request could not be honoured, the socket stays open |

### Client to server control messages

| `type` | Payload | Meaning |
| --- | --- | --- |
| `live` | | Jump to the live edge and follow it |
| `seek` | `timestampMs` | Start playback from this timestamp |
| `pause` | | Stop sending audio, keep the position |
| `resume` | | Continue from the paused position |
| `ping` | `clientTimeMs` | Keep alive, answered with `pong` carrying both clocks |

### Example session

```
C -> connect
S -> {"type":"stream-info","sampleRate":48000,"channels":1,"frameMs":100,"mode":"live","capturing":true, ...}
S -> <binary frame> <binary frame> ...
C -> {"type":"seek","timestampMs":1757031000000}
S -> {"type":"mode","mode":"playback","positionMs":1757031000000}
S -> <binary frames flagged historic> ...
S -> {"type":"switched-to-live","timestampMs":1757034000000}
S -> <binary frames flagged live> ...
```
