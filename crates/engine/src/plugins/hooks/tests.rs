use std::sync::Mutex;

use super::*;

#[derive(Clone, Copy)]
enum Answer {
    Patch(&'static str),
    Veto,
    Timeout,
    Trap,
    Reentry,
}

struct Fake {
    id: &'static str,
    hooks: &'static [&'static str],
    disabled: bool,
    answer: Answer,
    calls: Arc<Mutex<Vec<&'static str>>>,
}

impl Participant for Fake {
    fn id(&self) -> &str {
        self.id
    }

    fn declares(&self, hook: &str) -> bool {
        self.hooks.contains(&hook)
    }

    fn is_disabled(&self) -> bool {
        self.disabled
    }
}

struct Stub;

impl Hook for Stub {
    const NAME: &'static str = "before-launch";
    type Plugin = Fake;
    type Context = ();
    type Patch = Vec<&'static str>;

    fn fold(mut folded: Self::Patch, next: Self::Patch) -> Self::Patch {
        folded.extend(next);
        folded
    }

    fn invoke<'a>(
        plugin: &'a Fake,
        _: &'a (),
        _: &'a CallChain,
        timeout: Duration,
    ) -> BoxFuture<'a, Result<Verdict<Self::Patch>, CallError>> {
        plugin.calls.lock().unwrap().push(plugin.id);
        Box::pin(async move {
            match plugin.answer {
                Answer::Patch(arg) => Ok(Verdict::Patch(vec![arg])),
                Answer::Veto => Ok(Verdict::Veto("nope".into())),
                Answer::Timeout => Err(CallError::Timeout(timeout)),
                Answer::Trap => Err(CallError::Trap("boom".into())),
                Answer::Reentry => Err(CallError::Chain(crate::plugins::ChainError::Reentry(
                    plugin.id.into(),
                ))),
            }
        })
    }
}

struct Plugins(Vec<Arc<Fake>>);

impl Roster<Fake> for Plugins {
    fn loaded(&self) -> Vec<Arc<Fake>> {
        self.0.clone()
    }
}

struct Rig {
    calls: Arc<Mutex<Vec<&'static str>>>,
    plugins: Vec<Arc<Fake>>,
}

impl Rig {
    fn new() -> Self {
        Rig {
            calls: Arc::default(),
            plugins: Vec::new(),
        }
    }

    fn with(self, id: &'static str, answer: Answer) -> Self {
        self.plugin(id, &["before-launch"], false, answer)
    }

    fn plugin(
        mut self,
        id: &'static str,
        hooks: &'static [&'static str],
        disabled: bool,
        answer: Answer,
    ) -> Self {
        self.plugins.push(Arc::new(Fake {
            id,
            hooks,
            disabled,
            answer,
            calls: self.calls.clone(),
        }));
        self
    }

    async fn run(&self) -> Result<HookOutcome<Vec<&'static str>>, ErrorInfo> {
        let roster = Arc::new(Plugins(self.plugins.clone()));
        let hooks = Hooks::default();
        hooks.attach(&roster);
        hooks.run::<Stub>(&(), &CallChain::default()).await
    }

    fn called(&self) -> Vec<&'static str> {
        self.calls.lock().unwrap().clone()
    }
}

fn failed(plugin: &str) -> WarningInfo {
    WarningInfo::PluginHookFailed {
        plugin: plugin.into(),
        hook: "before-launch".into(),
    }
}

#[tokio::test]
async fn plugins_run_by_id_and_patches_fold_in_that_order() {
    let rig = Rig::new()
        .with("charlie", Answer::Patch("c"))
        .with("alpha", Answer::Patch("a"))
        .with("bravo", Answer::Patch("b"));

    let outcome = rig.run().await.unwrap();

    assert_eq!(rig.called(), ["alpha", "bravo", "charlie"]);
    assert_eq!(outcome.patch, ["a", "b", "c"]);
    assert!(outcome.warnings.is_empty());
}

#[tokio::test]
async fn a_timeout_warns_and_the_rest_still_run() {
    let rig = Rig::new()
        .with("alpha", Answer::Timeout)
        .with("bravo", Answer::Patch("b"));

    let outcome = rig.run().await.unwrap();

    assert_eq!(outcome.patch, ["b"]);
    assert_eq!(outcome.warnings, [failed("alpha")]);
}

#[tokio::test]
async fn a_trap_or_a_reentry_refusal_warns_too() {
    let rig = Rig::new()
        .with("alpha", Answer::Trap)
        .with("bravo", Answer::Reentry)
        .with("charlie", Answer::Patch("c"));

    let outcome = rig.run().await.unwrap();

    assert_eq!(outcome.patch, ["c"]);
    assert_eq!(outcome.warnings, [failed("alpha"), failed("bravo")]);
}

#[tokio::test]
async fn a_veto_stops_at_that_plugin() {
    let rig = Rig::new()
        .with("alpha", Answer::Patch("a"))
        .with("bravo", Answer::Veto)
        .with("charlie", Answer::Patch("c"));

    let error = rig.run().await.unwrap_err();

    assert_eq!(
        error,
        ErrorInfo::PluginVetoed {
            plugin: "bravo".into(),
            hook: "before-launch".into(),
            reason: "nope".into(),
        }
    );
    assert_eq!(rig.called(), ["alpha", "bravo"]);
}

#[tokio::test]
async fn a_plugin_without_the_hook_or_disabled_is_not_called() {
    let rig = Rig::new()
        .plugin("alpha", &["before-content-install"], false, Answer::Veto)
        .plugin("bravo", &["before-launch"], true, Answer::Veto)
        .with("charlie", Answer::Patch("c"));

    let outcome = rig.run().await.unwrap();

    assert_eq!(rig.called(), ["charlie"]);
    assert_eq!(outcome.patch, ["c"]);
}

#[tokio::test]
async fn no_roster_means_no_hooks() {
    let hooks = Hooks::<Fake>::default();

    let outcome = hooks.run::<Stub>(&(), &CallChain::default()).await.unwrap();

    assert_eq!(outcome, HookOutcome::default());
}
