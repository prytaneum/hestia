use proto::access::{EntryAccess, JobAccess};
use proto::error::{EntryKind, EntryTarget};
use proto::job::{Job, JobStarted};
use proto::Contract;
use serde::{Deserialize, Serialize};

use super::ID;

#[derive(Serialize, Deserialize, Default, Debug, Clone, PartialEq, Eq)]
#[serde(default, rename_all = "camelCase")]
pub struct EchoParams {
    pub text: String,
}

#[derive(Serialize, Deserialize, Default, Debug, Clone, PartialEq, Eq)]
#[serde(default, rename_all = "camelCase")]
pub struct Echoed {
    pub text: String,
    pub length: u32,
}

#[derive(Serialize, Deserialize, Default, Debug, Clone, PartialEq, Eq)]
#[serde(default, rename_all = "camelCase")]
pub struct EntryRef {
    pub id: String,
}

fn server(id: &str) -> Vec<EntryTarget> {
    vec![EntryTarget::new(EntryKind::Server, id)]
}

/// Acts on the entry it names.
pub struct Open;
impl Contract for Open {
    const CHANNEL: &'static str = "probe.open";
    const OWNER: Option<&'static str> = Some(ID);
    type Params = EntryRef;
    type Result = EntryRef;
}
impl EntryAccess for Open {
    fn touches(params: &EntryRef) -> Vec<EntryTarget> {
        server(&params.id)
    }

    fn creates(_: &EntryRef) -> Vec<EntryTarget> {
        Vec::new()
    }
}

/// Makes the entry it names.
pub struct Create;
impl Contract for Create {
    const CHANNEL: &'static str = "probe.create";
    const OWNER: Option<&'static str> = Some(ID);
    type Params = EntryRef;
    type Result = EntryRef;
}
impl EntryAccess for Create {
    fn touches(_: &EntryRef) -> Vec<EntryTarget> {
        Vec::new()
    }

    fn creates(result: &EntryRef) -> Vec<EntryTarget> {
        server(&result.id)
    }
}

/// A job that makes the entry it names, known when it is done.
pub struct Build;
impl Contract for Build {
    const CHANNEL: &'static str = "probe.build";
    const OWNER: Option<&'static str> = Some(ID);
    type Params = EntryRef;
    type Result = JobStarted;
}
impl Job for Build {
    type Progress = ();
    type Done = EntryRef;
}
impl EntryAccess for Build {
    fn touches(_: &EntryRef) -> Vec<EntryTarget> {
        Vec::new()
    }

    fn creates(_: &JobStarted) -> Vec<EntryTarget> {
        Vec::new()
    }
}
impl JobAccess for Build {
    fn creates_done(done: &EntryRef) -> Vec<EntryTarget> {
        server(&done.id)
    }
}

/// A typed call.
pub struct Echo;
impl Contract for Echo {
    const CHANNEL: &'static str = "probe.echo";
    const OWNER: Option<&'static str> = Some(ID);
    type Params = EchoParams;
    type Result = Echoed;
}

/// A job: answers the shouted text when it is done.
pub struct Shout;
impl Contract for Shout {
    const CHANNEL: &'static str = "probe.shout";
    const OWNER: Option<&'static str> = Some(ID);
    type Params = EchoParams;
    type Result = JobStarted;
}
impl Job for Shout {
    type Progress = ();
    type Done = Echoed;
}
impl EntryAccess for Echo {
    fn touches(_: &EchoParams) -> Vec<EntryTarget> {
        Vec::new()
    }

    fn creates(_: &Echoed) -> Vec<EntryTarget> {
        Vec::new()
    }
}
impl EntryAccess for Shout {
    fn touches(_: &EchoParams) -> Vec<EntryTarget> {
        Vec::new()
    }

    fn creates(_: &JobStarted) -> Vec<EntryTarget> {
        Vec::new()
    }
}
impl JobAccess for Shout {
    fn creates_done(_: &Echoed) -> Vec<EntryTarget> {
        Vec::new()
    }
}
