use std::time::Duration;

use hestia::proto::plugin::PageContext;
use hestia::proto::{server::ServerList, Empty};
use hestia::ui::{self, Action, Effect, Tone, UiNode};
use hestia::{json, Error, Plugin, Value};

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

hestia::plugin!(Probe);
