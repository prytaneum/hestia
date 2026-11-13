use std::time::Duration;

use hestia::proto::plugin::PageContext;
use hestia::proto::{server::ServerList, Empty};
use hestia::ui::{self, Action, Effect, Tone, UiNode};
use hestia::proto::hook::{BeforeContentInstall, BeforeLaunch, BeforeServerStart, LaunchPatch};
use hestia::proto::provider::java::Majors;
use hestia::{json, Error, Intercept, Intercepted, Plugin, Provide, Provided, Value};

struct Probe;

impl Plugin for Probe {
    fn init() -> Result<(), Error> {
        match hestia::data::load::<String>("mode")?.as_deref() {
            Some("list-servers") => hestia::call::<ServerList>(&Empty {}).map(drop),
            Some("exhaust-memory") => {
                let hoard = vec![1u8; 512 << 20];
                std::hint::black_box(&hoard);
                Ok(())
            }
            Some(mode) if mode.starts_with("get ") => {
                hestia::http::get(&mode["get ".len()..]).map(drop)
            }
            _ => Ok(()),
        }
    }

    fn on_event(topic: &str, payload: Value) {
        let mut seen = hestia::data::load::<Vec<Value>>("events")
            .ok()
            .flatten()
            .unwrap_or_default();
        seen.push(json!({ "topic": topic, "payload": payload }));
        let _ = hestia::data::save("events", &seen);
    }

    fn on_settings_changed(key: &str) {
        let value: Option<Value> = hestia::settings::get(key).ok().flatten();
        let _ = hestia::data::save("settings-changed", &json!({ "key": key, "value": value }));
    }

    fn render(page: &str, context: PageContext) -> Result<UiNode, Error> {
        match page {
            "overview" => Ok(ui::stack([
                ui::heading("Overview", 1),
                ui::text(context.entry.unwrap_or_else(|| "no entry".into())),
                ui::button("Go", "go"),
            ])),
            _ => Err(Error::NotFound(format!("unknown page `{page}`"))),
        }
    }

    fn action(
        page: &str,
        action: Action,
        values: Value,
        _context: PageContext,
    ) -> Result<Vec<Effect>, Error> {
        match page {
            "overview" => Ok(vec![
                Effect::Toast {
                    tone: Tone::Success,
                    text: format!("{} {}", action.id, values),
                },
                Effect::Refresh,
            ]),
            _ => Err(Error::NotFound(format!("unknown page `{page}`"))),
        }
    }

    fn provide(call: Provide) -> Provided {
        call.on::<Majors>(|_, _| {
            match hestia::data::load::<String>("mode")?.as_deref() {
                Some(mode) if mode.starts_with("get ") => {
                    hestia::http::get(&mode["get ".len()..]).map(|_| Vec::new())
                }
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
                true => Err("not on this list".into()),
                false => Ok(Empty {}),
            }
        })
        .finish()
    }

    fn run(command: &str, args: Value) -> Result<Value, Error> {
        match command {
            "echo" => Ok(args),
            "progress" => {
                hestia::progress::report(Some(0.5), "halfway");
                hestia::progress::report(None, "finishing");
                Ok(json!({ "cancelled": hestia::progress::cancelled() }))
            }
            "wait" => {
                while !hestia::progress::cancelled() {
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(Error::Failed("stopped".into()))
            }
            "stubborn" => loop {
                std::thread::sleep(Duration::from_millis(10));
            },
            "exec" => {
                let (program, argv) = program_call(&args);
                let env = env_pairs(&args);
                let env: Vec<(&str, &str)> = env
                    .iter()
                    .map(|(name, value)| (name.as_str(), value.as_str()))
                    .collect();
                let timeout = Duration::from_millis(args["timeoutMs"].as_u64().unwrap_or(10_000));
                let output =
                    hestia::process::exec_with(&program, &refs(&argv), &env, timeout, None)?;
                Ok(json!({
                    "status": output.status,
                    "stdout": String::from_utf8_lossy(&output.stdout),
                }))
            }
            "spawn" => {
                let (program, argv) = program_call(&args);
                let env = env_pairs(&args);
                let env: Vec<(&str, &str)> = env
                    .iter()
                    .map(|(name, value)| (name.as_str(), value.as_str()))
                    .collect();
                Ok(json!(hestia::process::spawn(&program, &refs(&argv), &env)?))
            }
            "stop" => {
                hestia::process::stop(args["id"].as_str().unwrap_or_default())?;
                Ok(json!(null))
            }
            "running" => Ok(json!(hestia::process::list()?)),
            "state" => {
                let status = hestia::process::status(args["id"].as_str().unwrap_or_default())?;
                Ok(json!(format!("{:?}", status.state)))
            }
            _ => Err(Error::NotFound(format!("unknown command `{command}`"))),
        }
    }
}

fn program_call(args: &Value) -> (String, Vec<String>) {
    let program = args["program"].as_str().unwrap_or_default().to_string();
    let argv = args["args"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|arg| arg.as_str().map(str::to_string))
        .collect();
    (program, argv)
}

fn env_pairs(args: &Value) -> Vec<(String, String)> {
    args["env"]
        .as_object()
        .into_iter()
        .flatten()
        .map(|(name, value)| (name.clone(), value.as_str().unwrap_or_default().into()))
        .collect()
}

fn refs(argv: &[String]) -> Vec<&str> {
    argv.iter().map(String::as_str).collect()
}

fn answer(key: &str) -> Result<LaunchPatch, String> {
    let plan: Value = hestia::data::load(key).ok().flatten().unwrap_or_default();
    if let Some(reason) = plan["veto"].as_str() {
        return Err(reason.to_string());
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
