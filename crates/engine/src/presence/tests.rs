use std::sync::{Arc, Mutex};

use super::*;

#[derive(Debug, Clone, PartialEq)]
enum Op {
    Connect,
    Set(Option<String>),
    Clear,
}

#[derive(Default)]
struct Script {
    ops: Vec<Op>,
    refuse_connect: bool,
}

struct Fake(Arc<Mutex<Script>>);

impl Discord for Fake {
    fn connect(&mut self) -> Result<()> {
        let mut script = self.0.lock().unwrap();
        script.ops.push(Op::Connect);
        if script.refuse_connect {
            bail!("no discord");
        }
        Ok(())
    }

    fn set(&mut self, card: &Activity) -> Result<()> {
        self.0
            .lock()
            .unwrap()
            .ops
            .push(Op::Set(card.details.clone()));
        Ok(())
    }

    fn clear(&mut self) -> Result<()> {
        self.0.lock().unwrap().ops.push(Op::Clear);
        Ok(())
    }

    fn close(&mut self) {}
}

fn presence() -> (Presence, Arc<Mutex<Script>>) {
    let script = Arc::new(Mutex::new(Script::default()));
    (Presence::new(Box::new(Fake(script.clone()))), script)
}

fn card(details: &str) -> Activity {
    Activity {
        details: Some(details.into()),
        ..Activity::default()
    }
}

fn shown(script: &Arc<Mutex<Script>>) -> Vec<Op> {
    script
        .lock()
        .unwrap()
        .ops
        .iter()
        .filter(|op| **op != Op::Connect)
        .cloned()
        .collect()
}

fn set(details: &str) -> Op {
    Op::Set(Some(details.into()))
}

#[test]
fn the_core_card_is_published_once_while_unchanged() {
    let (presence, script) = presence();
    presence.set_core(Some(card("core")));
    presence.tick(true);
    presence.tick(true);
    assert_eq!(shown(&script), [set("core")]);
}

#[test]
fn disabled_shows_nothing() {
    let (presence, script) = presence();
    presence.set_core(Some(card("core")));
    presence.tick(true);
    presence.tick(false);
    assert_eq!(shown(&script), [set("core"), Op::Clear]);
}

#[test]
fn nothing_wanted_clears_what_was_shown() {
    let (presence, script) = presence();
    presence.set_core(Some(card("core")));
    presence.tick(true);
    presence.set_core(None);
    presence.tick(true);
    assert_eq!(shown(&script), [set("core"), Op::Clear]);
}

#[test]
fn updates_beyond_five_in_twenty_seconds_wait() {
    let (presence, script) = presence();
    let start = Instant::now();
    for n in 0..5 {
        presence.set_core(Some(card(&format!("card {n}"))));
        presence.tick_at(true, start + Duration::from_secs(n));
    }
    presence.set_core(Some(card("sixth")));
    presence.tick_at(true, start + Duration::from_secs(6));
    assert_eq!(shown(&script).len(), 5);
    presence.tick_at(true, start + Duration::from_secs(20));
    assert_eq!(shown(&script).last(), Some(&set("sixth")));
}

#[test]
fn a_missing_discord_is_retried_only_every_few_ticks() {
    let (presence, script) = presence();
    script.lock().unwrap().refuse_connect = true;
    presence.set_core(Some(card("core")));
    for _ in 0..=RETRY_TICKS {
        presence.tick(true);
    }
    let connects = |script: &Arc<Mutex<Script>>| {
        script
            .lock()
            .unwrap()
            .ops
            .iter()
            .filter(|op| **op == Op::Connect)
            .count()
    };
    assert_eq!(connects(&script), 1);
    presence.tick(true);
    assert_eq!(connects(&script), 2);
    assert!(shown(&script).is_empty());
}

#[test]
fn shutdown_clears_and_stops() {
    let (presence, script) = presence();
    presence.set_core(Some(card("core")));
    presence.tick(true);
    presence.shutdown();
    assert!(presence.stopped());
    assert_eq!(shown(&script), [set("core"), Op::Clear]);
}
