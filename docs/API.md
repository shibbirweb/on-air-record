# API reference

Base URL: `http://<host>:8080`. All REST payloads are JSON with `camelCase` keys.

## Access

Each install is in one of three modes, chosen once by whoever opens the page first:

| Mode | Meaning |
| --- | --- |
| `undecided` | Nobody has answered the first run question yet. Behaves exactly like `open`. |
| `open` | No login. Every route works for anyone who can reach the service. |
| `accounts` | Every route except the public ones below needs a signed in session. |

With accounts on there are two roles. **Listeners** may call every `GET` (listen, scrub, export, read
state) and change their own password. **Admins** may call everything, including the account routes. A
listener calling an admin route gets `403 forbidden`; a request with no valid session gets
`401 unauthenticated`. The rule is "reading is listening, changing is administering", enforced by one guard
in front of the whole API (`backend/src/routes/guard.rs`), so it also covers any route added later.

Public in every mode: `GET /api/health`, `GET /api/auth/state`, `POST /api/auth/open`,
`POST /api/auth/setup`, `POST /api/auth/login`, `POST /api/auth/login/verify`, `POST /api/auth/logout`.

**Sessions** are an `oar_session` cookie: `HttpOnly`, `SameSite=Strict`, 30 days. It is marked `Secure`
when a reverse proxy sends `X-Forwarded-Proto: https`. The token is never in a response body, and only its
SHA-256 is stored. Every request checks it against the database, so signing out, removing an account or
changing a password takes effect on the next request. An open stream re-checks every 15 seconds and is
closed once its session is no longer valid.

**Other websites** are refused in every mode. A request that changes state (anything but `GET`, `HEAD`,
`OPTIONS`), and the WebSocket handshake, must carry either no `Origin` header, like `curl` or a script, or
an `Origin` whose host and port match the `Host` header. Otherwise the answer is `403 forbidden`. There is
no CORS policy, so another site's scripts cannot read any response either.

**Failed logins** are limited to 5 per client address in 15 minutes, after which that address gets
`429 rate_limited` until the window passes. The count is kept in memory, so restarting the service clears
it.

**Scrape token**: `GET /api/metrics` also accepts `Authorization: Bearer <token>`, for a Prometheus
scraper, which cannot sign in. The token opens that one route and nothing else, in every mode. A token that
is present but wrong, revoked or not a bearer token gets `401` even on an open recorder, rather than being
ignored. See [Metrics](#metrics).

**Scripting with accounts on**: log in once, keep the cookie, send it with each request:

```sh
curl -c jar -H 'content-type: application/json' \
  -d '{"email":"owner@example.com","password":"..."}' http://recorder:8080/api/auth/login
curl -b jar http://recorder:8080/api/status
```

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
| `unauthenticated` | 401 | Accounts are on and there is no valid session, or the email or password was wrong |
| `forbidden` | 403 | A listener asked for an admin route, or the request came from another website |
| `not_found` | 404 | Unknown device, session, account, or timestamp, or a path under `/api` that is not an endpoint |
| `conflict` | 409 | Action not valid in the current state, for example starting an active capture, or removing the only admin |
| `rate_limited` | 429 | Too many failed logins from this address; wait out the window the message names |
| `audio_error` | 503 | The host audio system rejected the operation |
| `internal` | 500 | Unexpected failure, details are in the server log |

A path under `/api` that matches no endpoint answers `404` with this envelope, whether or not anybody is signed
in, rather than the web interface's page. A known path asked with the wrong method answers `405`.

## Login and accounts

### `GET /api/auth/state`

What the page needs to decide between the first run question, the login page and the app. `user` is the
signed in account, and always `null` without accounts.

```json
{ "mode": "accounts", "user": { "id": 1, "email": "owner@example.com", "role": "admin", "createdAtMs": 1790000000000, "twoFactorEnabled": true }, "pendingTwoFactor": false }
```

### `POST /api/auth/open`

Answers the first run question with "keep it open". Returns the new state. `409` once the question has been
answered either way.

### `POST /api/auth/setup`

Creates the first admin, switches accounts on, and signs that admin in (`Set-Cookie`). Works while the mode
is `undecided` or `open`; `409` once accounts are on.

```json
{ "email": "owner@example.com", "password": "at least eight characters" }
```

Passwords are 8 to 128 characters with no other rules. The email is only a login name and is never sent
anything; it is unique regardless of case.

### `POST /api/auth/login`

Same body as setup. Returns the state and sets the cookie. A wrong email and a wrong password fail
identically, with `401`, and take the same time. `409` without accounts.

For an account with two factor sign in, a right password does **not** create a session. The answer is the
state with `"pendingTwoFactor": true` and `user` still `null`, plus an `oar_challenge` cookie (`HttpOnly`,
`SameSite=Strict`, `Path=/api/auth`, 5 minutes). `GET /api/auth/state` keeps answering
`pendingTwoFactor: true` while that cookie is valid, so a reload stays on the code step.

### `POST /api/auth/login/verify`

The second step, with the challenge cookie. `{ "code": "123456" }` takes the 6 digit code from the
authenticator app or, in its place, one of the recovery codes (case, spaces and the dash do not matter).
Returns the state, sets the session cookie and clears the challenge cookie.

A wrong code is `401` and counts towards the same per address lockout as a wrong password. After 5 wrong
codes against one challenge, or once it expires, the answer is `401` asking for the password again. A code
that already signed somebody in is refused for the rest of its 30 seconds. Codes one step either side of
now are accepted, to forgive a phone clock that has drifted slightly.

### Two factor sign in (any signed in account, for itself)

Codes are RFC 6238 TOTP: HMAC-SHA1, 6 digits, 30 second steps, the only settings every authenticator app
supports.

| Route | Body | Answer |
| --- | --- | --- |
| `GET /api/auth/two-factor` | | `{ "enabled": true, "recoveryCodesLeft": 9 }` |
| `POST /api/auth/two-factor/setup` | | `{ "secretKey": "GXAS 3KKW ...", "otpauthUri": "otpauth://totp/...", "qrSvg": "<svg ...>" }`. Nothing changes until `enable`. `409` if already on. |
| `POST /api/auth/two-factor/enable` | `{ "code": "123456" }` from the app | `{ "recoveryCodes": ["abcde-fghjk", ...] }`, ten of them, sent only this once. `400` for a wrong code. |
| `POST /api/auth/two-factor/disable` | `{ "password": "..." }` | `204`. `400` for a wrong password. |
| `POST /api/auth/two-factor/recovery-codes` | `{ "password": "..." }` | Ten new codes; the old ones stop working. |

The QR code is an SVG document meant to be shown as an image (`<img src="data:image/svg+xml,...">`), not
inserted as markup. The secret is stored readable, because the server has to recompute codes to check
them, as every authenticator app does; recovery codes are stored as SHA-256 hashes.

### `POST /api/auth/logout`

Ends the session, if there is one, and always clears the cookie.

### `POST /api/auth/password`

Change your own password. Signs the account out everywhere except this session. Any signed in role. `204`.

```json
{ "currentPassword": "...", "newPassword": "..." }
```

### `GET /api/updates` (admin)

Whether a newer release exists, from the service's last check. Never waits on the network: the service
asks GitHub a minute after it starts and every six hours after, while `checkForUpdates` is on.

```json
{
  "currentVersion": "0.6.0",
  "channel": "stable",
  "automatic": true,
  "checkedAtMs": 1790385176043,
  "error": null,
  "available": { "version": "0.7.0", "tag": "v0.7.0", "prerelease": false, "publishedAtMs": 1790900000000, "notes": "### Added\n\n- ...", "url": "https://github.com/shibbirweb/on-air-record/releases/tag/v0.7.0" },
  "releases": [ { "version": "0.7.0", "...": "..." } ],
  "install": { "kind": "installer", "dir": "/home/pi/on-air-record", "os": "linux", "target": "x86_64-unknown-linux-gnu" },
  "releasesUrl": "https://github.com/shibbirweb/on-air-record/releases"
}
```

- `channel` is `stable` or `beta`: `OAR_CHANNEL` in the environment, else the installer's `config` file
  beside the program, else `beta` for a beta build and `stable` otherwise. A stable channel is never
  offered a beta.
- `available` is the newest newer release, or `null`. `releases` is every newer release on the channel,
  newest first, each with its notes in Markdown as written on GitHub.
- `error` is why the last check failed, with the previous answer kept. `checkedAtMs` is `null` until the
  first check.
- `install.kind` is `installer` (a folder with the installer's start script beside the program; `dir`
  names it), `systemd` (started by systemd, detected by `INVOCATION_ID`), `docker` (the published
  container image, which sets `OAR_CONTAINER`), or `manual`. `target` is the
  build's target triple, which names its release download.

### `POST /api/updates/check` (admin)

Ask GitHub now, and answer with the same body as `GET /api/updates`. Works with automatic checks off.
GitHub being unreachable is not an error here: the answer carries it in `error`.

### `GET /api/users` (admin)

```json
{ "users": [ { "id": 1, "email": "owner@example.com", "role": "admin", "createdAtMs": 1790000000000, "twoFactorEnabled": false } ] }
```

### `POST /api/users` (admin)

`{ "email": "...", "password": "...", "role": "listener" }`, role `admin` or `listener`. Returns the account
with `201`. `409` if the email is taken or accounts are off.

### `PATCH /api/users/{id}` (admin)

`{ "role": "admin" }`. `409` when it would demote the only admin.

### `POST /api/users/{id}/password` (admin)

`{ "password": "..." }`. For somebody who forgot theirs; signs that account out everywhere. `204`.

### `DELETE /api/users/{id}` (admin)

Removes the account and ends its sessions and streams. `409` for the only admin. `204`.

### `DELETE /api/users/{id}/two-factor` (admin)

Switches off somebody's two factor sign in and deletes their recovery codes, for a lost phone with no
codes left. They sign in with their password alone afterwards and can set up a new app. `204`.

### Recovery on the host

Somebody locked out of the web interface uses the program itself, against the same data directory, whether
or not the service is running:

```sh
on-air-record auth reset-password owner@example.com   # prints a generated password
on-air-record auth reset-2fa owner@example.com         # two factor sign in off for that account
on-air-record auth disable                            # accounts off, every account deleted
```

Each is written to the activity log as done by the host, with no address.

### `GET /api/activity` (admin)

The activity log, newest first: every sign in and sign out, failed attempts, changes to accounts, listening
sessions, downloads, and changes to the recorder. Admin only, like the account list, because it says who
signed in from where and what they listened to. The scrape token does not open it.

Query, every field optional:

| Field | Meaning |
| --- | --- |
| `limit` | Entries in the page, 50 when left out, at most 200 |
| `beforeId` | Only entries older than this id, for the next page. Paging by id rather than by page number means entries logged while somebody reads never shift the next page |
| `email` | Only what this account did, however the email is cased |
| `group` | `access` (signing in and out), `accounts`, `listening` or `recorder`. Any other word is refused with `400` |
| `fromMs`, `toMs` | Only entries in this span, both ends included |

```json
{
  "entries": [
    {
      "id": 42,
      "atMs": 1757030400000,
      "actor": { "kind": "account", "userId": 2, "email": "kitchen@example.com" },
      "address": "192.168.1.20",
      "userAgent": "Mozilla/5.0 (iPad)",
      "event": {
        "kind": "listened",
        "startedAtMs": 1757029800000,
        "connectedMs": 600000,
        "playedMs": 540000,
        "playedBack": true,
        "earliestMs": 1757020000000
      }
    }
  ]
}
```

`actor.kind` is `account` (with the email as it was then, so the log still names a removed account),
`guest` (nobody signed in, or an open recorder), or `host` (a recovery command). `address` is the
connection's, so behind a reverse proxy it is the proxy's. `event.kind` is one of:

| Group | Kinds and their details |
| --- | --- |
| `access` | `signed_in` (`method`: `password`, `code` or `recovery_code`), `sign_in_failed` (`email`: what was typed when it looked like an email, else `null`), `second_factor_failed`, `sign_in_blocked` (`email`), `signed_out` |
| `accounts` | `accounts_set_up`, `stayed_open`, `password_changed`, `two_factor_enabled`, `two_factor_disabled`, `recovery_codes_replaced`, `account_created` (`email`, `role`), `account_removed` (`email`), `role_changed` (`email`, `from`, `to`), `password_set` (`email`), `two_factor_removed` (`email`), `accounts_disabled` |
| `listening` | `listened` (as above, one entry per stream when it closes, however it closes; `playedMs` is what the page reported playing), `exported` (`fromMs`, `toMs`) |
| `recorder` | `capture_started`, `capture_stopped`, `device_selected` (`deviceId`, `null` for the default), `settings_changed` (`changes`: `key`, `from`, `to` for each setting a save changed), `settings_reset`, `bookmark_added` (`label`, `timestampMs`), `bookmark_removed` (`label`), `metrics_token_created` (`replaced`), `metrics_token_revoked` |

An entry is written only once the action has happened, so a refused request leaves nothing. No entry ever
holds a password, a code, a token or a cookie; the event types have nowhere to put one. Writing the log
never fails the action it records: if the database refuses the entry, the action still succeeds and the
service logs a warning. Nothing in the API edits or deletes an entry; they leave only by the
`activityRetentionDays` window.

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
    "deviceSampleRate": 48000,
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

`capture.state` is one of `idle`, `starting`, `recording`, or `error`. `capture.sampleRate` is the rate
being recorded, after the recording rate setting; `capture.deviceSampleRate` is the rate the microphone
itself runs at, which is what the recording rates on offer go up to. They differ when a lower rate is set.
Both are `0` before the first capture. `listeners` counts every open stream
socket, whether it is following the live feed, playing back history, or paused.

`capture.error` is why capture failed when `state` is `error`. While `state` is `recording` it is set when
recordings are not reaching the disk (the recordings folder cannot be written, the disk is full, or the
database is refusing new segments) and starts `Recording to disk is failing, live audio continues`. The
live feed is unaffected, and the field clears by itself once recording works again.

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
  "autoStartDelaySeconds": 0,
  "frameMs": 100,
  "checkForUpdates": true,
  "soundSensitivity": "medium",
  "activityRetentionDays": 90
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
| `autoStartDelaySeconds` | integer | 0 to 600 | On next service start. Seconds auto start waits before opening the device; the HTTP server does not wait |
| `frameMs` | integer | 20 to 500 | On next capture start |
| `checkForUpdates` | boolean | | On the next scheduled check. Whether the service asks GitHub every six hours for a newer release; see `GET /api/updates` |
| `soundSensitivity` | string | `low`, `medium`, `high` | Immediately, for the next sounds request. How far above each room's own background a moment must rise to count as a sound; see `GET /api/timeline/sounds`. Any other word is refused |
| `activityRetentionDays` | integer | 1 to 3650 | On next janitor pass. How long the activity log keeps an entry, on its own window: keeping recordings forever does not keep the log forever; see `GET /api/activity` |

### `GET /api/settings/defaults`

The values a reset restores, in the same shape as `GET /api/settings`. Exposed so a client can say
precisely what a reset will change rather than keeping a second copy of the defaults that drifts.

### `POST /api/settings/reset`

Restores the defaults and returns the new settings. Takes no body. Provided for scripting: the web UI
stages the defaults into its draft and commits them through `PATCH` instead, so that a reset is reviewed
and saved like any other change.

The selected `inputDeviceId` is **preserved**, because the microphone is chosen elsewhere and silently
moving the recorder onto another one is not what resetting the tuning asks for. As with `PATCH`, capture
restarts if `frameMs` actually changed.

Resetting `retentionHours` downwards makes the janitor delete everything outside the smaller window within
a minute, and that audio is not recoverable, so a client should confirm before calling this.

### `POST /api/settings/test-recordings-dir`

Try a recordings directory without saving it, so an unusable path is caught while it can still be
corrected. Changes nothing on disk, including for a path that does not exist yet.

```json
{ "path": "/mnt/audio/on-air" }
```

`path` may be `null` or empty to test the default location.

```json
{
  "ok": true,
  "resolvedPath": "/mnt/audio/on-air",
  "exists": false,
  "willCreate": true,
  "readable": true,
  "writable": true,
  "message": "Does not exist yet. It will be created inside '/mnt/audio' when you save."
}
```

A path that will not work is a normal answer, not an API error: the response is still `200` with `ok`
set to false and `message` explaining why, so a client never has to parse an error envelope to find out
what is wrong. Writability is judged by writing a probe file and deleting it again, because a directory
can exist and be readable while still refusing writes.

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

### `GET /api/timeline/sounds`

The moments something was heard in a window, found from the stored levels with the current
`soundSensitivity`. Listener access.

| Query parameter | Required | Notes |
| --- | --- | --- |
| `fromMs` | yes | Window start, epoch milliseconds |
| `toMs` | yes | Window end, greater than `fromMs`, at most 32 days after it |

```json
{
  "fromMs": 1757030400000,
  "toMs": 1757034000000,
  "sensitivity": "medium",
  "sounds": [
    { "startMs": 1757031000000, "endMs": 1757031007200, "seekMs": 1757030999000, "peak": 36 }
  ]
}
```

- A sound is a stretch where the level rises clearly above that room's own background: each five minute
  block gets a noise floor from its levels, and `soundSensitivity` sets how far above it counts. Bursts
  under 200 ms are ignored, bursts less than 2 s apart merge, sounds under 300 ms are dropped, and a sound
  never spans a gap in the recording.
- A sound that starts before the window or ends after it is included whole, and the answer is the same
  whatever the window, because the recording either side is read too.
- `seekMs` is where to start playback to hear it from its beginning: a second early, but never inside a
  gap. `peak` is its loudest level, `0..255`, on the same scale as `peaks`.
- Only indexed segments are searched, like scrubbing, so the few seconds being recorded now are not.

### `GET /api/timeline/sounds/next`

The sound to jump to from where playback is. Searches all of history, a day at a time, not only a window.
Listener access.

| Query parameter | Required | Default | Notes |
| --- | --- | --- | --- |
| `fromMs` | yes | | Where playback is now, epoch milliseconds |
| `direction` | no | `forward` | `forward` or `backward` |

```json
{ "sound": { "startMs": 1757031000000, "endMs": 1757031007200, "seekMs": 1757030999000, "peak": 36 } }
```

`sound` is `null` when there is none that way. `forward` skips a sound whose `seekMs` is less than 250 ms
ahead, which is the one just jumped to. `backward` returns the sound playback is in when more than 2 s
into it, and the one before otherwise, the way a music player's back button treats a track.

## Export

### `GET /api/export/plan`

What an export would produce, without producing it. Same query as the download, so a client can show the
size and format, and surface a refusal, while the range can still be adjusted.

| Query parameter | Required | Notes |
| --- | --- | --- |
| `fromMs` | yes | Start of the range, epoch milliseconds |
| `toMs` | yes | End of the range, must be greater than `fromMs` |

```json
{
  "fromMs": 1757030400000,
  "toMs": 1757030700000,
  "durationMs": 300000,
  "sampleRate": 48000,
  "channels": 1,
  "totalBytes": 28800044,
  "mixedRates": false
}
```

### `GET /api/export`

Streams a canonical 16 bit PCM WAV file with `Content-Length` and a
`Content-Disposition: attachment` filename naming the range. Same query parameters as the plan.

Three things are worth knowing about what comes out:

- **Gaps become silence** rather than being skipped, so the file's duration matches the requested range
  and thirty seconds into the file is thirty seconds after `fromMs`. Audio whose file is missing or cut
  short counts as a gap too.
- **A range spanning several recording rates exports at the lowest of them**, downsampling the rest.
  Upsampling instead would invent detail the audio never had and make the file bigger for nothing. The
  plan reports this as `mixedRates`.
- **Exports are capped at 2 GB.** RIFF sizes are unsigned 32 bit, so 4 GB is a hard format limit; the cap
  sits well below it. A larger span is refused by the plan with the size it would have been.

## Bookmarks

Named moments on the timeline. A bookmark addresses a point in time rather than a segment, so a moment
can be marked before the segment covering it has closed.

### `GET /api/bookmarks`

Every bookmark, oldest first, which is the order the timeline draws them in.

```json
{
  "bookmarks": [
    {
      "id": 7,
      "timestampMs": 1757030400000,
      "label": "Doorbell rang",
      "note": null,
      "createdAtMs": 1757030412000
    }
  ]
}
```

### `POST /api/bookmarks`

```json
{ "timestampMs": 1757030400000, "label": "Doorbell rang", "note": "someone at the front" }
```

`label` is required and is trimmed. An empty or whitespace only label is a `400`, as is a label over 120
characters or a note over 2000, because silently discarding the end of what somebody typed is worse than
saying it was too long. Returns the created bookmark.

### `PATCH /api/bookmarks/{id}`

Accepts any subset of `timestampMs`, `label` and `note`, and returns the updated bookmark. A present
`note` of `null` clears it, an absent one leaves it alone. An empty patch is a `400` rather than a silent
no-op.

### `DELETE /api/bookmarks/{id}`

Returns the remaining bookmarks, so one request both deletes and refreshes the timeline.

Bookmarks are pruned by the retention janitor along with the audio they point at: once the timeline no
longer reaches that far back, the bookmark is a link to nothing.

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

## Metrics

### `GET /api/metrics`

The recorder's state in the [Prometheus text format](https://prometheus.io/docs/instrumenting/exposition_formats/),
served as `text/plain; version=0.0.4; charset=utf-8`. Needs a listener, like any read, or the scrape
token. Cheap to call: the only query is the same aggregate row `/api/storage` reads.

| Metric | Type | Meaning |
| --- | --- | --- |
| `oar_build_info{version,channel}` | gauge | Always 1; the labels name the running version and its release channel |
| `oar_start_time_seconds` | gauge | When the service started, Unix seconds |
| `oar_capture_state{state}` | gauge | One series each for `idle`, `starting`, `recording` and `error`; the current one is 1 |
| `oar_capture_sample_rate_hertz` | gauge | The rate being recorded; only while capture runs |
| `oar_capture_device_sample_rate_hertz` | gauge | The rate the microphone runs at; only while capture runs |
| `oar_capture_dropped_frames_total` | counter | Frames the recorder had no room for, since the service started |
| `oar_recorder_frames_written_total` | counter | Frames written to segment files since capture last started |
| `oar_recorder_frames_not_written_total` | counter | Frames broadcast live that never reached disk since capture last started |
| `oar_recorder_disk_healthy` | gauge | 1 while writing to disk works, 0 while it fails; only while recording |
| `oar_input_level_rms`, `oar_input_level_peak` | gauge | Recent input level, 0 to 1; only while recording |
| `oar_listeners{activity}` | gauge | Open audio streams by `live`, `playback` and `paused` |
| `oar_recordings_bytes` | gauge | Bytes of closed segments on disk |
| `oar_recordings_segments` | gauge | Closed segments on disk |
| `oar_recordings_oldest_timestamp_seconds` | gauge | Start of the oldest recording kept; absent before anything is recorded |
| `oar_recordings_newest_timestamp_seconds` | gauge | End of the newest closed segment; absent before anything is recorded |
| `oar_retention_seconds` | gauge | The retention window; absent when recordings are kept forever |

A reading that does not exist right now is left out rather than reported as 0, so an alert never mistakes
"nothing recorded yet" for a recording from 1970. The names are a contract: renaming one is a breaking
change and is called out in the changelog.

```text
# HELP oar_capture_state Where the recorder is, one series per state with the current one at 1.
# TYPE oar_capture_state gauge
oar_capture_state{state="idle"} 0
oar_capture_state{state="starting"} 0
oar_capture_state{state="recording"} 1
oar_capture_state{state="error"} 0
```

### `GET /api/metrics/token` (admin)

Whether a scrape token exists. The token itself is never returned here.

```json
{ "createdAtMs": 1757030400000 }
```

`createdAtMs` is `null` when there is none.

### `POST /api/metrics/token` (admin)

Makes a scrape token, replacing any old one, which stops working at once. This answer is the only time the
token is ever sent; only its SHA-256 is stored.

```json
{ "token": "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08", "createdAtMs": 1757030400000 }
```

### `DELETE /api/metrics/token` (admin)

Revokes the token. Answers `{ "createdAtMs": null }`.

## WebSocket `GET /api/ws/stream`

One socket carries both live audio and DVR playback. The session is in exactly one of three modes, and
every control message below is a transition between them. `Paused` remembers where it came from, so
resuming returns you to live or to history rather than always to live:

```mermaid
stateDiagram-v2
    [*] --> Live: connect
    Live --> Playback: seek
    Playback --> Live: go-live, or caught up
    Live --> Paused: pause
    Playback --> Paused: pause
    Paused --> Live: resume, if it was live
    Paused --> Playback: resume, if it was in history
```

"Caught up" is the cursor reaching the live edge on its own, at which point the server sends
`switched-to-live` and resubscribes the session to the hub.

`seek` and `go-live` also work from `Paused`, and a `seek` while already in `Playback` just moves the
cursor. They are left off the diagram because they do the obvious thing and drawing them obscured the part
that matters, which is that `Paused` remembers where it came from.


One socket carries both the live broadcast and DVR playback. Binary messages are audio frames in the format
described in [AUDIO_PIPELINE.md](AUDIO_PIPELINE.md). Text messages are JSON control messages.

The handshake is refused with `403` from a page served by another site, and with `401` when accounts are
on and there is no valid session. An open socket re-checks its session every 15 seconds and the server
closes it once the session has ended, the account was removed, or accounts were switched on after it
connected.

On the same 15 second beat the server sends a WebSocket ping frame. Browsers and WebSocket libraries answer
these automatically, so a client needs to do nothing. A socket the server has heard nothing from for 45
seconds (no message, no ping, no pong) is dropped without a close frame, and so is one that has not
accepted a write for 20 seconds. Both mean the device has gone without closing the connection, and without
them it would stay open, and listed, until TCP gave up. A client that pauses and sits quiet stays connected,
because its pongs keep it heard.

### Server to client control messages

| `type` | Payload | Meaning |
| --- | --- | --- |
| `stream-info` | `sampleRate`, `channels`, `frameMs`, `mode`, `serverTimeMs`, `liveEdgeMs`, `earliestMs`, `capturing` | Sent on connect, and again if capture stops while a listener is attached |
| `mode` | `mode`, `positionMs` | The session changed between `live`, `playback`, and `paused` |
| `switched-to-live` | `timestampMs` | Playback caught up with the live edge |
| `gap` | `fromMs`, `toMs` | No recording exists in this range, or its files are missing or cut short; playback skipped it |
| `end-of-recording` | `timestampMs` | Playback reached the newest data while capture is stopped |
| `level` | `rms`, `peak` | Input meter, emitted about ten times per second in live mode |
| `speed` | `value` | The playback speed actually in force, after clamping, and whenever the server resets it |
| `error` | `code`, `message` | Request could not be honoured, the socket stays open |
| `listeners` | `listeners` | Everybody connected right now. Admins only, see below |
| `listeners-hidden` | | This socket may no longer see the list; drop the copy you have |

### The listener list

A socket whose user may see who else is connected (an admin, or anybody on an open recorder) receives a
`listeners` message straight after `stream-info`, then a fresh one whenever somebody connects, disconnects,
or moves between `live`, `playback` and `paused`, and when a listener in `playback` seeks. It is always
the whole list, oldest connection first, never a difference, so a client that missed one loses nothing. A
listener account never receives it.

```json
{
  "type": "listeners",
  "listeners": [
    {
      "id": 7,
      "email": "kitchen@example.com",
      "role": "listener",
      "address": "192.168.1.24",
      "userAgent": "Mozilla/5.0 (iPhone; CPU iPhone OS 17_6 like Mac OS X) ...",
      "connectedAtMs": 1757033100000,
      "activity": "playback",
      "fromMs": 1757029500000,
      "player": "playing"
    }
  ]
}
```

- `id` is stable for the life of the connection and never reused while the service runs.
- `email` and `role` are `null` for a guest on an open recorder.
- `address` is the peer address the service sees, which behind a reverse proxy is the proxy.
- `userAgent` is the browser's header as sent, trimmed and cut to 512 characters, or `null`.
- `activity` is `live`, `playback` or `paused`. `fromMs` is where playback started or last jumped to,
  not the moving playhead, and is `null` unless `activity` is `playback`.
- `player` is what the client last said with a `player` message: `idle` (play not pressed since the page
  opened), `playing`, or `paused`. `activity` is what the server streams, and the two differ because
  browsers only start audio after a click and the UI's pause button stops the speakers, not the stream. A
  client that never sends `player` is listed as `playing`. The UI sends it on every connect and whenever
  play or pause is pressed. It changes nothing about what the socket sends.

Whether a socket may see the list is checked again with the session every 15 seconds. A socket that loses
it, because its account was made a listener or accounts were switched on, gets `listeners-hidden`; one
that gains it gets a `listeners` message at once.

### Client to server control messages

| `type` | Payload | Meaning |
| --- | --- | --- |
| `live` | | Jump to the live edge and follow it |
| `seek` | `timestampMs` | Start playback from this timestamp |
| `pause` | | Stop sending audio, keep the position |
| `speed` | `value` | Play history at `0.25`, `0.5`, `1`, `1.5`, `2` or `4` times real time. Anything else snaps to the nearest. Ignored while live |
| `resume` | | Continue from the paused position |
| `ping` | `clientTimeMs` | Keep alive, answered with `pong` carrying both clocks |
| `player` | `state` | `idle`, `playing` or `paused`: whether the person is hearing the stream. Informational only, see the listener list |

Speed is pacing, not processing: at double speed the server simply hands over frames twice as often, and
the client plays each one twice as fast. The cursor, the segment index and the binary format stay entirely
speed agnostic. Rejoining the live feed always forces the speed back to `1`, because the present cannot be
outrun, and the server announces that with its own `speed` message.

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
