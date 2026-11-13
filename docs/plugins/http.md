# HTTP, URLs and hashes

A plugin with `network:connect` reaches the hosts its manifest lists in `network`, over `wasi:http`. `hestia::http` wraps
that in a request builder, a response with headers and status classes, a URL builder and the mapping from a failing
status to the typed `hestia::Error`. `hestia::hash` covers the digests. The pure parts (status mapping, `Retry-After`,
header lookup, URL encoding, hashes) are unit tested in `crates/sdk/src/http/` and `crates/sdk/src/hash.rs`.

Only `https://` URLs to named hosts are allowed and the host does not follow redirects: a `3xx` comes back as it is, and
the plugin requests the `Location` itself (which must be granted too). The body is limited by the host.

## Requests

```rust
use hestia::http::{Request, Url};

let url = Url::new("https://api.example.com/v2")?
    .segment("search")
    .query("query", "fabric api")
    .query("limit", 20);

let hits: Vec<Hit> = Request::get(url)
    .header("authorization", format!("Bearer {key}"))
    .send_json()?;
```

Source: the doc examples in `crates/sdk/src/http/request.rs` and `mod.rs`.

| Call | Does |
|---|---|
| `Request::get/head/post/put/patch/delete(url)` | starts a request; `url` is a `&str`, a `String` or a `Url` |
| `.header(name, value)` | sets a header, replacing any of the same name |
| `.body(content_type, bytes)` | sets a raw body |
| `.json(&value)?` | sets a JSON body (`Error::Json` if it does not encode) |
| `.form(pairs)` | sets an `application/x-www-form-urlencoded` body |
| `.send()?` | sends it and returns the `Response`, whatever the status |
| `.send_json::<T>()?` | `send`, then `error_for_status`, then decode the body as JSON |

`hestia::http::get(url)` and `post(url, content_type, body)` are the short forms.

A call that cannot be made is an error: `Error::Denied` when the host is not granted, `Error::Offline` when it cannot
be reached or timed out, `Error::Failed` otherwise. An answer, even a `500`, is a `Response`.

## Responses

`Response` has `status`, `headers` and `body`; `text()` (`Error::Invalid` if not UTF-8) and `json::<T>()` (`Error::Json`
if it does not decode); the status classes `is_informational`, `is_success`, `is_redirection`, `is_client_error` and
`is_server_error`; and `retry_after()`.

`Headers::get(name)` is the first value, `get_all(name)` every value, and names compare case-insensitively.

### From a status to an error

`response.error_for_status()?` passes a `2xx` through and maps anything else:

| Status | Error |
|---|---|
| `2xx` | the response |
| `404` | `Error::NotFound("HTTP 404…")` |
| `429` | `Error::RateLimited { retry_after }`, from a `Retry-After` in seconds; the HTTP-date form gives `None` |
| `401`, `403` | `Error::Upstream { status: Some(code), .. }` |
| any other `3xx`, `4xx`, `5xx` | `Error::Upstream { status: Some(code), retry_after, message }` |

The message is `HTTP <status>` and, for a text, JSON or XML body, a short snippet of it (at most 200 characters, white
space collapsed). Headers never appear in it. `401` and `403` stay `Upstream` on purpose: the plugin knows whether that
means a wrong API key (`Error::NotConfigured("api-key")`), a missing sign-in or something else, so map it where it is
known:

```rust
let reply = Request::get(url).send()?;
let reply = match reply.error_for_status() {
    Err(Error::Upstream { status: Some(401 | 403), .. }) => {
        return Err(Error::NotConfigured("api-key".into()))
    }
    other => other?,
};
```

A provider's errors reach the user as described in [providers.md](providers.md#errors).

## URLs

`Url::new(base)` takes `https://host` or `https://host/path` (a query, a fragment or a missing host is
`Error::Invalid`). `.segment(s)` and `.segments(iter)` append path segments; `.query(key, value)` appends a pair, and a key
may repeat. Everything but `A-Z a-z 0-9 - . _ ~` is percent-encoded, so a `/`, `?`, `#`, `&`, `=`, `+`, a space or
non-ASCII text stays inside its component: `"a b&c/é"` becomes `a%20b%26c%2F%C3%A9`. `hestia::http::encode(text)` is that
encoding on its own.

## Hashes

```rust
use hestia::hash::{self, Algorithm};

let digest = hash::sha256(&bytes);
let on_disk = hash::file("/data/agent", Algorithm::Sha256)?;
```

`sha1`, `sha256` and `sha512` take bytes and return lowercase hex; `file(path, algorithm)` reads a file in 64 KiB chunks,
so a large download is never held whole, and `Algorithm::hash_reader` does the same for any `Read`. A missing file is
`Error::Io`.
