# Testing a plugin

The SDK ships a mock host, `hestia::mock`, so the code of a plugin that calls the daemon, makes http requests or reads
its settings runs in a plain `cargo test` on your own machine: no daemon, no `wasm32-wasip2` build, no component to load.

The mock replaces only the transport underneath `hestia::call`, `hestia::run_job`, `hestia::http` and `hestia::settings`.
Your code calls the same functions it ships with. It exists on every target except `wasm32`, so there is nothing to
enable: write the tests in a `#[cfg(test)]` module beside the code and run `cargo test`.

## A test

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

Source: the doc example in `crates/sdk/src/mock.rs` and `crates/sdk/tests/mock.rs`, which run in CI.

## What it scripts

`hestia::mock::install()` returns a `Mock` bound to the current thread. Each test owns its mock, and dropping it removes
it, so tests running in parallel never see each other's answers.

| Call | Does |
|---|---|
| `mock.reply::<C>(&result)` | answers every `hestia::call::<C>` with `result` |
| `mock.done::<J>(&done)` | settles every `hestia::run_job::<J>` with `done` |
| `mock.fail::<C>(error)` | fails a call or job with `error` |
| `mock.respond(method, url, response)` | answers that method and exact URL |
| `mock.refuse(method, url, error)` | fails that request, such as `Error::Offline(None)` or `Error::Denied(..)` |
| `mock.setting(key, &value)` | sets a setting, as the user would |
| `mock.setting_value(key)` | the setting's value now, including what the plugin wrote with `settings::set` |
| `mock.exec(program, output)` | finishes every `process::exec` of that program with `output`; `mock.fail_exec` fails it |
| `mock.process_logs(id, lines)` | the captured lines `process::logs` returns for a spawned process |
| `mock.file(url, size)` | serves that exact URL to `download::download`; `mock.fail_download` fails it |
| `mock.data(key, &value)` | saves a value under a data key, as an earlier call would have |
| `mock.data_value(key)` | what is saved under the key now |
| `mock.runs()` | every program run or spawned, with its args, env, cwd and Java major |
| `mock.downloads()` | every download asked for, with its URL, destination and digest |
| `mock.calls()` | every call and job made, with its channel, params and whether it was a job |
| `mock.requests()` | every request sent, with method, URL, headers and body |

An answer stays until you script it again. A call, job, request, program run or download you did not script fails with
`Error::Failed` naming the channel, the method and URL, the program or the URL, so a missing answer is never mistaken
for an empty one. A setting or data key you did not script reads as unset. `process::spawn` always succeeds and returns
an id (`mock-1`, `mock-2`, ...) that `status`, `stop` and `list` then track. Nothing touches the disk: data values live
in the mock, and a download writes no file at its destination. Calling an SDK function with no mock installed panics.

## What it does not do

The mock does not check permissions, scopes, manifest settings or hosts: it proves your code's logic, not what the user
granted. It does not run the plugin's entry points either; call the functions your `Plugin` methods delegate to. Check
permissions and the manifest with `hestia plugin check`, and the whole plugin with `hestia plugin dev`.
