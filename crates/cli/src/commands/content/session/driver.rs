//! The async half of the session: it answers screen requests over the caller's
//! client and injects the replies back as app events. Search, detail, and
//! version lookups are plain request/response; the install job forwards
//! progress and runs last, because the client `Session` has a single
//! event-callback slot.

use client::proto::content::{
    ContentAddSpec, ContentFailure, ContentKind, ContentProject, ContentVersion, InstalledContent,
    SearchQuery, SearchResult, VersionQuery,
};
use client::proto::error::EntryTarget;
use client::proto::minecraft::ProvisionProgress;
use client::Client;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

pub(super) enum Request {
    Search {
        seq: u64,
        query: SearchQuery,
    },
    Detail {
        source: String,
        project: String,
        kind: ContentKind,
    },
    Versions {
        query: VersionQuery,
    },
    Install(InstallJob),
}

/// A staged batch to apply to an entry: the removals to clear first, then the
/// additions to install.
pub(super) struct InstallJob {
    pub entry: EntryTarget,
    pub spec: ContentAddSpec,
    pub removals: Vec<Removal>,
}

/// One removal to perform: the key `content.remove` takes (the project id), the
/// save worlds narrowing it (empty clears every copy), and the index entries it
/// clears, carried along for the report.
pub(super) struct Removal {
    pub key: String,
    pub worlds: Vec<String>,
    pub record: Option<InstalledContent>,
}

pub(super) enum AppEvent {
    Search {
        seq: u64,
        offset: u32,
        result: SearchResult,
    },
    Detail(Box<ContentProject>),
    Versions {
        project: String,
        versions: Vec<ContentVersion>,
    },
    Progress(ProvisionProgress),
    Done {
        items: Vec<InstalledContent>,
        removed: Vec<InstalledContent>,
        failures: Vec<ContentFailure>,
    },
    Failed {
        message: String,
    },
}

/// Answer screen requests over `client`. Ends when the screen (the only request
/// sender) is dropped.
pub(super) async fn drive(
    client: &Client,
    mut requests: UnboundedReceiver<Request>,
    events: UnboundedSender<AppEvent>,
) {
    while let Some(request) = requests.recv().await {
        let event = match request {
            Request::Search { seq, query } => match client.content().search(&query).await {
                Ok(result) => AppEvent::Search {
                    seq,
                    offset: query.offset,
                    result,
                },
                Err(e) => AppEvent::Failed {
                    message: e.to_string(),
                },
            },
            Request::Detail {
                source,
                project,
                kind,
            } => {
                match client
                    .content()
                    .project(&source, &project, Some(kind))
                    .await
                {
                    Ok(detail) => AppEvent::Detail(Box::new(detail)),
                    Err(_) => continue,
                }
            }
            Request::Versions { query } => {
                let project = query.project.clone();
                match client.content().versions(&query).await {
                    Ok(versions) => AppEvent::Versions { project, versions },
                    Err(e) => AppEvent::Failed {
                        message: e.to_string(),
                    },
                }
            }
            Request::Install(job) => run_install(client, &events, job).await,
        };
        if events.send(event).is_err() {
            break;
        }
    }
}

async fn run_install(
    client: &Client,
    events: &UnboundedSender<AppEvent>,
    job: InstallJob,
) -> AppEvent {
    let InstallJob {
        entry,
        spec,
        removals,
    } = job;
    let content = client.content();
    let kind = spec.kind;

    let mut removed = Vec::new();
    let mut failures = Vec::new();
    for removal in removals {
        let key = removal.key.clone();
        match content
            .remove(&entry, kind, &[removal.key], &removal.worlds)
            .await
        {
            Ok(()) => removed.extend(removal.record),
            Err(e) => failures.push(ContentFailure {
                item: key,
                title: removal.record.map(|r| r.title).unwrap_or_default(),
                error: client::error_info(&e),
            }),
        }
    }

    if spec.items.is_empty() {
        return AppEvent::Done {
            items: Vec::new(),
            removed,
            failures,
        };
    }

    let progress = events.clone();
    match content
        .add(&entry, spec, move |p| {
            let _ = progress.send(AppEvent::Progress(p.clone()));
        })
        .await
    {
        Ok((items, add_failures)) => {
            failures.extend(add_failures);
            AppEvent::Done {
                items,
                removed,
                failures,
            }
        }
        Err(e) => AppEvent::Failed {
            message: e.to_string(),
        },
    }
}
