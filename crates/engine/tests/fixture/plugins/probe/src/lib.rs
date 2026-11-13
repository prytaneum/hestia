use std::time::Duration;

use hestia::pages::{Paged, Pages, Values};
use hestia::proto::{server::ServerList, Empty};
use hestia::ui::{self, Effect, Tone};
use hestia::proto::hook::{BeforeContentInstall, BeforeLaunch, BeforeServerStart, LaunchPatch};
use hestia::proto::files::{ServerFileParams, ServerFilesRead};
use hestia::proto::instance::InstanceList;
use hestia::proto::provider::java::Majors;
use hestia::{
    json, Commands, Error, Events, Intercept, Intercepted, Plugin, Provide, Provided, Ran, Value,
};
use serde::{Deserialize, Serialize};

struct Probe;

#[derive(Serialize, Deserialize)]
#[serde(tag = "id", content = "payload", rename_all = "kebab-case")]
enum Act {
    Go,
}

#[derive(Deserialize)]
struct FailArgs {
    kind: String,
}

#[derive(Deserialize)]
struct Program {
    program: String,
    #[serde(default)]
    args: Vec<String>,
    #[serde(default)]
    env: std::collections::BTreeMap<String, String>,
    #[serde(default, rename = "timeoutMs")]
    timeout_ms: Option<u64>,
}

#[derive(Deserialize)]
struct Id {
    id: String,
}

#[derive(Deserialize)]
struct Bump {
    name: String,
    #[serde(default, rename = "pauseMs")]
    pause_ms: u64,
    #[serde(default)]
    trap: bool,
    #[serde(default)]
    hang: bool,
}

#[derive(Deserialize)]
struct Setting {
    key: String,
    value: Value,
}

#[derive(Deserialize)]
struct Endpoint {
    endpoint: String,
}

impl Plugin for Probe {
    fn init() -> Result<(), Error> {
        match hestia::data::load::<String>("mode")?.as_deref() {
            Some("list-servers") => hestia::call::<ServerList>(&Empty {}).map(drop),
            Some("exhaust-memory") => {
                let hoard = vec![1u8; 512 << 20];
                std::hint::black_box(&hoard);
                Ok(())
            }
            Some("list-instances") => hestia::call::<InstanceList>(&Empty {}).map(drop),
            Some(mode) if mode.starts_with("read ") => hestia::call::<ServerFilesRead>(
                &ServerFileParams {
                    server: "smp".into(),
                    path: mode["read ".len()..].into(),
                },
            )
            .map(drop),
            Some(mode) if mode.starts_with("get ") => {
                hestia::http::get(&mode["get ".len()..]).map(drop)
            }
            Some(mode) if mode.starts_with("fail ") => Err(typed(&mode["fail ".len()..])),
            _ => Ok(()),
        }
    }

    fn event(call: Events) {
        call.glob("*", |topic, payload| {
            let mut seen = hestia::data::load::<Vec<Value>>("events")
                .ok()
                .flatten()
                .unwrap_or_default();
            if topic == "ipc.readable" {
                let id = payload["connection"].as_str().unwrap_or_default();
                let bytes = hestia::ipc::Connection::resume(id)
                    .receive(4096, Duration::ZERO)
                    .unwrap_or_default();
                let _ = hestia::data::save("ipc-received", &String::from_utf8_lossy(&bytes));
            }
            seen.push(json!({ "topic": topic, "payload": payload }));
            let _ = hestia::data::save("events", &seen);
        })
        .finish()
    }

    fn on_settings_changed(key: &str) {
        let value: Option<Value> = hestia::settings::get(key).ok().flatten();
        let _ = hestia::data::save("settings-changed", &json!({ "key": key, "value": value }));
    }

    fn page(call: Pages) -> Paged {
        call.on(
            "overview",
            |context| {
                Ok(ui::stack([
                    ui::heading("Overview", 1),
                    ui::text(context.entry.unwrap_or_else(|| "no entry".into())),
                    ui::button("Go", Act::Go),
                ]))
            },
            |act: Act, values: Values, _context| match act {
                Act::Go => Ok(vec![
                    Effect::Toast {
                        tone: Tone::Success,
                        text: format!("go {}", values.parse::<Value>()?),
                    },
                    Effect::Refresh,
                ]),
            },
        )
        .finish()
    }

    fn provide(call: Provide) -> Provided {
        call.on::<Majors>(|_, _| {
            match hestia::data::load::<String>("mode")?.as_deref() {
                Some(mode) if mode.starts_with("get ") => {
                    hestia::http::get(&mode["get ".len()..]).map(|_| Vec::new())
                }
                Some(mode) if mode.starts_with("fail ") => Err(typed(&mode["fail ".len()..])),
                _ => Ok(Vec::new()),
            }
        })
        .finish()
    }

    fn intercept(call: Intercept) -> Intercepted {
        call.on::<BeforeLaunch>(|context| {
            let _ = hestia::data::save("context", &format!("{context:?}"));
            answer("launch")
        })
        .on::<BeforeServerStart>(|context| {
            let _ = hestia::data::save("context", &format!("{context:?}"));
            answer("server")
        })
        .on::<BeforeContentInstall>(|context| {
            let refused: Vec<String> = hestia::data::load("content")
                .ok()
                .flatten()
                .unwrap_or_default();
            match refused.contains(&context.file_name) {
                true => Err(Error::Denied("not on this list".into())),
                false => Ok(Empty {}),
            }
        })
        .finish()
    }

    fn command(call: Commands) -> Ran {
        call.on("echo", |args: Value| Ok(args))
            .on("fail", |args: FailArgs| -> Result<(), Error> {
                Err(typed(&args.kind))
            })
            .on("progress", |_: Empty| {
                hestia::progress::report(Some(0.5), "halfway");
                hestia::progress::report(None, "finishing");
                Ok(json!({ "cancelled": hestia::progress::cancelled() }))
            })
            .on("wait", |_: Empty| -> Result<(), Error> {
                while !hestia::progress::cancelled() {
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(Error::Failed("stopped".into()))
            })
            .on("stubborn", |_: Empty| -> Result<(), Error> {
                loop {
                    std::thread::sleep(Duration::from_millis(10));
                }
            })
            .on("exec", |call: Program| {
                let env: Vec<(&str, &str)> = call
                    .env
                    .iter()
                    .map(|(name, value)| (name.as_str(), value.as_str()))
                    .collect();
                let timeout = Duration::from_millis(call.timeout_ms.unwrap_or(10_000));
                let output = hestia::process::exec_with(
                    &call.program,
                    &refs(&call.args),
                    &env,
                    timeout,
                    None,
                )?;
                Ok(json!({
                    "status": output.status,
                    "stdout": String::from_utf8_lossy(&output.stdout),
                }))
            })
            .on("spawn", |call: Program| {
                let env: Vec<(&str, &str)> = call
                    .env
                    .iter()
                    .map(|(name, value)| (name.as_str(), value.as_str()))
                    .collect();
                hestia::process::spawn(&call.program, &refs(&call.args), &env)
            })
            .on("bump", |call: Bump| {
                hestia::data::update(&call.name, |count: &mut u32| {
                    std::thread::sleep(Duration::from_millis(call.pause_ms));
                    if call.trap {
                        panic!("trapped while holding the lock");
                    }
                    while call.hang {
                        std::thread::sleep(Duration::from_millis(10));
                    }
                    *count += 1;
                    *count
                })
            })
            .on("set-setting", |call: Setting| {
                hestia::settings::set(&call.key, &call.value)
            })
            .on("ipc-open", |call: Endpoint| {
                Ok(hestia::ipc::open(&call.endpoint)?.id().to_string())
            })
            .on("stop", |call: Id| hestia::process::stop(&call.id))
            .on("running", |_: Empty| hestia::process::list())
            .on("state", |call: Id| {
                let status = hestia::process::status(&call.id)?;
                Ok(format!("{:?}", status.state))
            })
            .finish()
    }
}

fn refs(argv: &[String]) -> Vec<&str> {
    argv.iter().map(String::as_str).collect()
}

fn typed(kind: &str) -> Error {
    match kind {
        "offline" => Error::Offline(Some("no route".into())),
        "upstream" => Error::Upstream {
            status: Some(503),
            retry_after: Some(30),
            message: "the service is down".into(),
        },
        "rate-limited" => Error::RateLimited {
            retry_after: Some(7),
        },
        "sign-in" => Error::SignInRequired,
        "not-configured" => Error::NotConfigured("api-key".into()),
        "missing" => Error::MissingRequirement {
            name: "git".into(),
            url: Some("https://git-scm.com".into()),
        },
        "busy" => Error::Busy("the entry is claimed".into()),
        "not-found" => Error::NotFound("no such thing".into()),
        other => Error::Failed(format!("unknown failure `{other}`")),
    }
}

fn answer(key: &str) -> Result<LaunchPatch, Error> {
    let plan: Value = hestia::data::load(key).ok().flatten().unwrap_or_default();
    if let Some(reason) = plan["veto"].as_str() {
        return Err(Error::Failed(reason.to_string()));
    }
    let list = |key: &str| -> Vec<String> {
        plan[key]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|arg| arg.as_str().map(str::to_string))
            .collect()
    };
    Ok(LaunchPatch {
        extra_jvm_args: list("jvm"),
        extra_game_args: list("game"),
        wrapper: None,
    })
}

hestia::plugin!(Probe);
