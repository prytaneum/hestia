//! A daemon standing in for `hestiad`, driven over an in-memory duplex rather
//! than a socket. Answers one scripted reply per channel.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use client::proto::error::ErrorInfo;
use client::Client;
use ipc::protocol::{self, Event, Response};
use ipc::Connection;
use serde_json::{json, Value};

pub enum Reply {
    Ok(Value),
    Fail(ErrorInfo),
    /// Answer, then push these events.
    OkThen(Value, Vec<Event>),
    /// Never answer.
    Silent,
    /// Answer with this frame verbatim, envelope and all.
    Frame(String),
}

#[derive(Default)]
pub struct Script {
    replies: HashMap<String, Vec<Reply>>,
    seen: Arc<Mutex<Vec<String>>>,
}

impl Script {
    pub fn new() -> Script {
        Script::default()
    }

    pub fn on(mut self, channel: &str, reply: Reply) -> Script {
        self.replies
            .entry(channel.to_string())
            .or_default()
            .push(reply);
        self
    }

    /// Every channel called so far, in order.
    pub fn seen(&self) -> Arc<Mutex<Vec<String>>> {
        self.seen.clone()
    }

    /// Wire the script to a client over a duplex, and serve until it is dropped.
    pub fn serve(self) -> Client {
        let (ours, theirs) = tokio::io::duplex(64 * 1024);
        let (mut reader, mut writer) = Connection::from_io(ours).into_split();
        let mut replies = self.replies;
        let seen = self.seen.clone();
        tokio::spawn(async move {
            while let Ok(Some(frame)) = reader.recv().await {
                let Ok(request) = protocol::decode_request(&frame) else {
                    continue;
                };
                seen.lock().unwrap().push(request.channel.clone());
                let queued = replies
                    .get_mut(&request.channel)
                    .filter(|queue| !queue.is_empty())
                    .map(|queue| queue.remove(0));

                let (response, events) = match queued.unwrap_or(Reply::Ok(json!({}))) {
                    Reply::Ok(payload) => (Some(Response::success(payload)), Vec::new()),
                    Reply::OkThen(payload, events) => (Some(Response::success(payload)), events),
                    Reply::Fail(info) => (
                        Some(Response::failure(
                            serde_json::to_value(&info).unwrap_or(Value::Null),
                        )),
                        Vec::new(),
                    ),
                    Reply::Silent => (None, Vec::new()),
                    Reply::Frame(raw) => {
                        let _ = writer.send(&raw).await;
                        continue;
                    }
                };

                if let Some(mut response) = response {
                    response.id = request.id;
                    let frame = protocol::encode_response(&response);
                    if writer.send(&frame).await.is_err() {
                        return;
                    }
                }
                for event in events {
                    if writer.send(&protocol::encode_event(&event)).await.is_err() {
                        return;
                    }
                }
            }
        });
        Client::over(Connection::from_io(theirs))
    }
}

pub fn event(topic: &str, payload: Value) -> Event {
    Event {
        topic: topic.to_string(),
        payload,
    }
}
