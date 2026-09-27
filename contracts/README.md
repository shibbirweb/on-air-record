# Wire contracts

Golden fixtures for everything that crosses the wire between the Rust service and the browser. Both test
suites read these files, so a field renamed on one side fails a test on the other instead of leaving both
green.

| File | Written by | Holds |
| --- | --- | --- |
| `server-messages.json` | backend | Every `ServerMessage` on the stream socket, by `type` |
| `responses.json` | backend | Every REST response body the UI reads, and the error envelope |
| `enums.json` | backend | Every value of each string enum on the wire |
| `audio-frames.json` | backend | Binary audio frames as hex, with their header fields and samples |
| `client-messages.json` | frontend | Every `ClientMessage` the browser sends on the stream socket |
| `requests.json` | frontend | Every REST request body and query string `api` in `client.ts` sends |

## How they are checked

The backend generates the first four in `backend/src/contract_tests.rs`, serialising fixed examples exactly
as the server does, and fails when a checked in file differs. Every `Option` is shown both set and `null`
somewhere, because that is the only way a TypeScript type missing `| null` gets caught.

The frontend tests in `frontend/src/api/__tests__/contract*.test.ts` read the same files and prove that
each TypeScript type in `src/api/types.ts` has exactly the fixture's keys, in both directions and all the
way down, with matching value kinds, and that `decodeAudioFrame` reads every frame as the backend wrote it.

The last two go the other way. They hold what the frontend's own code produces (its tests call the real
`api` methods and compare), and the backend parses each one into the real request type and checks every
value arrived.

## When a test fails

After an intended change to a wire type on the backend:

```sh
cd backend && UPDATE_CONTRACTS=1 cargo test contract
git diff ../contracts
cd ../frontend && npm test      # fails until src/api/types.ts matches
```

Review the diff before committing it: it is the change every browser will see.

After an intended change to what the frontend sends, edit `client-messages.json` or `requests.json` by
hand to match, then run `cargo test contract`, which tells you whether the server still accepts it.

Timestamps in frames stay within 2^53 - 1, the largest integer a browser `Number` holds exactly.
