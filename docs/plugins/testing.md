# Testing a plugin

The SDK ships a mock host, `hestia::mock`, so plugin code that calls the daemon, makes HTTP requests, reads settings or
runs programs is tested with a plain `cargo test`: no daemon, no `wasm32-wasip2` build. The mock replaces only the
transport under `hestia::call`, `hestia::run_job`, `hestia::http`, `hestia::settings`, `process`, `download`, `data`
and `ipc`. It exists on every target except `wasm32`, so write tests in a `#[cfg(test)]` module beside the code.

```rust
fn refresh() -> Result<usize, hestia::Error> {
    let key: String = hestia::settings::get("api-key")?
        .ok_or_else(|| hestia::Error::NotConfigured("api-key".into()))?;
    let hits: Vec<String> = hestia::http::Request::get("https://api.example.com/v1/hits")
        .header("authorization", format!("Bearer {key}"))
        .send_json()?;
    hestia::run_job::<JavaInstall>(&JavaInstallParams { major: 21, ..Default::default() })?;
    Ok(hits.len())
}

#[cfg(test)]
mod tests {
    use hestia::http::{Method, Response};

    use super::*;

    #[test]
    fn refresh_reads_the_hits_and_installs_java() {
        let mock = hestia::mock::install();
        mock.setting("api-key", &"k");
        mock.respond(
            Method::Get,
            "https://api.example.com/v1/hits",
            Response { status: 200, body: br#"["a","b"]"#.to_vec(), ..Default::default() },
        );
        mock.done::<JavaInstall>(&JavaInstallDone { runtime: Default::default(), already_installed: false });

        assert_eq!(refresh().unwrap(), 2);
        assert_eq!(mock.calls()[0].params["major"], 21);
        assert_eq!(mock.requests()[0].headers.get("authorization"), Some("Bearer k"));
    }
}
```

Source: the doc example in `crates/sdk/src/mock.rs` and `crates/sdk/tests/mock.rs`.

`hestia::mock::install()` returns a `Mock` bound to the current thread; dropping it removes it, so parallel tests never
see each other's answers.

| Script | Inspect |
|---|---|
| `reply::<C>(&result)`, `done::<J>(&done)`, `fail::<C>(error)`: daemon calls and jobs | `calls()` |
| `respond(method, url, response)`, `refuse(method, url, error)`: HTTP | `requests()` |
| `setting(key, &value)` | `setting_value(key)` |
| `data(key, &value)` | `data_value(key)` |
| `exec(program, output)`, `fail_exec`, `process_logs(id, lines)` | `runs()` |
| `file(url, size)`, `fail_download` | `downloads()` |
| `endpoint(name)`, `incoming(endpoint, bytes)` | `sent(endpoint)` |
| `messages(locale, entries)` | |

A call, job, request, program run or download you did not script fails with `Error::Failed` naming it, so a missing
answer is never mistaken for an empty one. An unscripted setting or data key reads as unset. `process::spawn` always
succeeds and returns `mock-1`, `mock-2`, ... Nothing touches the disk, `hestia::log` and `progress::report` do nothing,
and `progress::cancelled` is always false. Calling an SDK function with no mock installed panics.

`hestia plugin test [id...]` runs the tests of the repository's plugins, all or the ones named, and takes `--sdk <dir>`.

The mock does not check permissions, scopes, manifest settings or hosts, and does not run your `Plugin` entry points;
call the functions they delegate to. Check the manifest and grant with `hestia plugin check`, and the whole plugin with
`hestia plugin dev`.
