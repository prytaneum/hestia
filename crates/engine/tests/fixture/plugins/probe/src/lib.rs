use std::time::Duration;

use hestia::proto::plugin::PageContext;
use hestia::proto::{server::ServerList, Empty};
use hestia::ui::{self, Action, Effect, Tone, UiNode};
use hestia::{
    json, Error, InstallContext, LaunchContext, LaunchPatch, Plugin, ServerContext, Value,
};

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

    fn before_launch(context: LaunchContext) -> Result<LaunchPatch, String> {
        let _ = hestia::data::save("context", &format!("{context:?}"));
        answer("launch")
    }

    fn before_server_start(context: ServerContext) -> Result<LaunchPatch, String> {
        let _ = hestia::data::save("context", &format!("{context:?}"));
        answer("server")
    }

    fn before_content_install(context: InstallContext) -> Result<(), String> {
        let refused: Vec<String> = hestia::data::load("content")
            .ok()
            .flatten()
            .unwrap_or_default();
        match refused.contains(&context.file_name) {
            true => Err("not on this list".into()),
            false => Ok(()),
        }
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
            _ => Err(Error::NotFound(format!("unknown command `{command}`"))),
        }
    }
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
    })
}

hestia::plugin!(Probe);
