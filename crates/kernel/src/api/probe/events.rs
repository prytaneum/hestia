use std::sync::{Arc, Mutex, PoisonError};

use wasmtime::component::{Instance, InstancePre};
use wasmtime::Store;

use super::bindings::exports::hestia::probe::{before_echo, echoed, entry_opened};
use crate::api::Chain;
use crate::registry::{Hook, Point};
use crate::runtime::extension::{Budget, Extension, Outcome};
use crate::runtime::State;
use crate::Plugins;

pub const ENTRY_OPENED: &str = "hestia:probe/entry-opened@0.1.0";
pub const ECHOED: &str = "hestia:probe/echoed@0.1.0";
pub const BEFORE_ECHO: &str = "hestia:probe/before-echo@0.1.0";

struct EntryOpened {
    entry: [String; 1],
}

impl Extension for EntryOpened {
    fn budget(&self) -> Budget {
        Budget::Event
    }

    fn call<'a>(
        &'a self,
        store: &'a mut Store<State>,
        instance: &'a Instance,
        pre: &'a InstancePre<State>,
    ) -> Outcome<'a> {
        Box::pin(async move {
            let topic = entry_opened::GuestIndices::new(pre)?.load(&mut *store, instance)?;
            topic.call_heard(&mut *store, &self.entry[0]).await
        })
    }
}

impl Point for EntryOpened {
    fn interface(&self) -> &'static str {
        ENTRY_OPENED
    }

    fn entries(&self) -> &[String] {
        &self.entry
    }
}

struct Echoed {
    text: String,
}

impl Extension for Echoed {
    fn budget(&self) -> Budget {
        Budget::Event
    }

    fn call<'a>(
        &'a self,
        store: &'a mut Store<State>,
        instance: &'a Instance,
        pre: &'a InstancePre<State>,
    ) -> Outcome<'a> {
        Box::pin(async move {
            let topic = echoed::GuestIndices::new(pre)?.load(&mut *store, instance)?;
            topic.call_heard(&mut *store, &self.text).await
        })
    }
}

impl Point for Echoed {
    fn interface(&self) -> &'static str {
        ECHOED
    }
}

/// The text as the plugins before have left it, and the reason of a veto.
struct Folding {
    text: String,
    veto: Option<String>,
}

struct BeforeEcho(Mutex<Folding>);

impl BeforeEcho {
    fn folding(&self) -> std::sync::MutexGuard<'_, Folding> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl Extension for BeforeEcho {
    fn budget(&self) -> Budget {
        Budget::Hook
    }

    fn call<'a>(
        &'a self,
        store: &'a mut Store<State>,
        instance: &'a Instance,
        pre: &'a InstancePre<State>,
    ) -> Outcome<'a> {
        Box::pin(async move {
            let hook = before_echo::GuestIndices::new(pre)?.load(&mut *store, instance)?;
            let text = self.folding().text.clone();
            Ok(hook
                .call_check(&mut *store, &text)
                .await?
                .map(|verdict| match verdict {
                    before_echo::Verdict::Proceed(text) => self.folding().text = text,
                    before_echo::Verdict::Veto(reason) => self.folding().veto = Some(reason),
                }))
        })
    }
}

impl Point for BeforeEcho {
    fn interface(&self) -> &'static str {
        BEFORE_ECHO
    }
}

impl Hook for BeforeEcho {
    fn vetoed(&self) -> bool {
        self.folding().veto.is_some()
    }
}

/// What `before_echo` came to: the text the plugins left, and who vetoed it.
#[derive(Debug, PartialEq, Eq)]
pub struct Hooked {
    pub text: String,
    pub vetoed: Option<Vetoed>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Vetoed {
    pub plugin: String,
    pub reason: String,
}

impl Plugins {
    /// Tells the plugins that hear `entry-opened` that `entry`, a stable id,
    /// was opened.
    pub fn entry_opened(self: &Arc<Self>, entry: &str) {
        self.publish(Arc::new(EntryOpened {
            entry: [entry.to_owned()],
        }));
    }

    /// Tells the plugins that hear `echoed` the text that was echoed.
    pub fn echoed(self: &Arc<Self>, text: &str) {
        self.publish(Arc::new(Echoed {
            text: text.to_owned(),
        }));
    }

    /// Runs `before-echo` over the plugins that export it, for a call made
    /// through `chain`.
    pub async fn before_echo(&self, text: &str, chain: &Chain) -> Hooked {
        let hook = BeforeEcho(Mutex::new(Folding {
            text: text.to_owned(),
            veto: None,
        }));
        let plugin = self.fold(&hook, chain).await;
        let Folding { text, veto } = hook.0.into_inner().unwrap_or_else(PoisonError::into_inner);
        Hooked {
            text,
            vetoed: plugin
                .zip(veto)
                .map(|(plugin, reason)| Vetoed { plugin, reason }),
        }
    }
}
