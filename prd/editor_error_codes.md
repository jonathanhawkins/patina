# Editor HTTP Error Codes

pat-f23vr: the editor HTTP server (`gdeditor::editor_server`) returns the
same JSON envelope for every non-2xx response so clients can parse error
information uniformly. This document is the source of truth for the
machine-readable codes the server emits.

## Envelope shape

Every non-2xx response from `/api/*` and the editor's static routes has
this body shape:

```json
{
  "error": {
    "code": "<machine_code>",
    "message": "<human-readable explanation>"
  }
}
```

The `error.code` field is a stable, snake_case string identifier that
clients may switch on. The `error.message` field is a free-form human
string intended for surfacing in UI and logs; its wording may evolve and
should not be parsed.

## Default code per status

When a handler does not provide an explicit code, the server derives one
from the HTTP status code:

| HTTP status | Default `error.code`     |
|-------------|--------------------------|
| 400         | `bad_request`            |
| 401         | `unauthorized`           |
| 403         | `forbidden`              |
| 404         | `not_found`              |
| 409         | `conflict`               |
| 413         | `payload_too_large`      |
| 415         | `unsupported_media_type` |
| 422         | `unprocessable_entity`   |
| 429         | `rate_limit`             |
| 500         | `internal`               |
| 501         | `not_implemented`        |
| 503         | `unavailable`            |

Any status not listed above falls back to the generic code `error`.

## Explicit codes used by handlers

Some failure modes use a more specific machine code than the default
status-derived code. The set is small and stable:

| Code              | HTTP status | When emitted                                                                                    |
|-------------------|-------------|-------------------------------------------------------------------------------------------------|
| `path_sandbox`    | 403         | Request supplied a path containing `..` or pointing outside the project root (pat-aivim).       |
| `cors_origin`     | 403         | Request's `Origin` header is not in the configured CORS allowlist (pat-bof7u).                  |
| `rate_limit`      | 429         | Per-token request rate exceeded `DEFAULT_RATE_LIMIT_PER_SECOND` (pat-vxejb).                    |
| `unauthorized`    | 401         | Bearer-token gate refused a request that lacked a valid `Authorization: Bearer <token>` header. |
| `not_found`       | 404         | Route or resource missing (catch-all for routes; thumbnail/preview misses).                     |
| `bad_request`     | 400         | Malformed HTTP request, missing or unparseable body, missing required parameter.                |
| `internal`        | 500         | Unrecoverable server-side error (handler bug, I/O failure, serialize failure).                  |

## Properties guaranteed by the envelope

`editor_error_envelope_test` is the acceptance gate for this contract:

1. The response body of every non-2xx response parses as JSON.
2. The parsed JSON has a top-level `error` field whose value is an object.
3. That object has a string `code` and a string `message`.
4. The `code` is non-empty.
5. The Content-Type is `application/json`.

The 200-class responses are out of scope and may have any shape, including
ones with their own domain-specific `"error"` key inside `"data"` (the
probe schemas, for example, use `data.error` for downstream tool status).

## Adding a new code

When a handler needs a more specific code than the status-derived default:

1. Use `send_error_coded(stream, status, "snake_case_code", "human message")`.
2. Add a row to the "Explicit codes used by handlers" table above.
3. Update the acceptance test only if the new code introduces a new
   response category that is not already exercised by an existing probe.

The default helper `send_error(stream, status, message)` remains the
recommended entry point for handlers that do not need a custom code.
