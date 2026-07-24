use std::collections::HashMap;
use std::collections::VecDeque;
use std::convert::Infallible;
use std::sync::Arc;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;
use std::time::Duration;

use async_stream::stream;
use bytes::Bytes;
use http::HeaderValue;
use http::header::CACHE_CONTROL;
use http::header::CONTENT_TYPE;
use http_body::Frame;
use http_body_util::StreamBody;
use serde_json::Value;
use serde_json::json;
use tokio::sync::Mutex;
use tokio::sync::broadcast;
use topcoat::context::Cx;
use topcoat::router::Body;
use topcoat::router::Response;

use crate::bridge::AppServerBridge;
use crate::components::PlanPresentation;
use crate::components::activity_row;
use crate::components::approval_fragment;
use crate::components::is_activity;
use crate::components::is_visible_activity;
use crate::components::item_fragment;
use crate::components::item_fragment_with_plan_presentation;
use crate::components::work_digest;

const REPLAY_EVENT_CAPACITY: usize = 4096;
const REPLAY_BYTE_CAPACITY: usize = 4 * 1024 * 1024;
const CLIENT_EVENT_CAPACITY: usize = 1024;

#[derive(Clone)]
struct SequencedEvent {
    seq: u64,
    encoded: Bytes,
}

#[derive(Default)]
struct ReplayState {
    events: VecDeque<SequencedEvent>,
    bytes: usize,
}

/// Shared, replayable projection of app-server notifications for browser clients.
pub(crate) struct LiveEventHub {
    next_seq: AtomicU64,
    replay: Mutex<ReplayState>,
    events: broadcast::Sender<SequencedEvent>,
}

impl LiveEventHub {
    pub(crate) async fn start(bridge: Arc<AppServerBridge>) -> Arc<Self> {
        let (events, _) = broadcast::channel(CLIENT_EVENT_CAPACITY);
        let hub = Arc::new(Self {
            next_seq: AtomicU64::new(1),
            replay: Mutex::new(ReplayState::default()),
            events,
        });
        hub.attach(bridge).await;
        hub
    }

    pub(crate) async fn attach(self: &Arc<Self>, bridge: Arc<AppServerBridge>) {
        let task_hub = Arc::clone(self);
        tokio::spawn(async move {
            let mut source = bridge.subscribe();
            let mut renderer = EventRenderer::default();
            for request in bridge.outstanding_server_requests().await {
                if let Some(event) = renderer.render(request).await {
                    task_hub.publish(event).await;
                }
            }
            loop {
                match source.recv().await {
                    Ok(message) => {
                        if let Some(event) = renderer.render(message).await {
                            task_hub.publish(event).await;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => {
                        task_hub
                            .publish(json!({ "op": "resync", "reason": "sourceLagged" }))
                            .await;
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
        });
    }

    pub(crate) async fn runtime_changed(&self, account: &str) {
        self.publish(json!({ "op": "runtimeChanged", "account": account }))
            .await;
    }

    pub(crate) fn watermark(&self) -> u64 {
        self.next_seq.load(Ordering::Acquire).saturating_sub(1)
    }

    async fn publish(&self, mut event: Value) {
        let seq = self.next_seq.fetch_add(1, Ordering::AcqRel);
        if let Some(object) = event.as_object_mut() {
            object.insert("seq".to_string(), Value::from(seq));
        }
        let encoded = Bytes::from(format!("id: {seq}\ndata: {event}\n\n"));
        let sequenced = SequencedEvent { seq, encoded };
        let mut replay = self.replay.lock().await;
        replay.bytes += sequenced.encoded.len();
        replay.events.push_back(sequenced.clone());
        while replay.events.len() > REPLAY_EVENT_CAPACITY || replay.bytes > REPLAY_BYTE_CAPACITY {
            if let Some(evicted) = replay.events.pop_front() {
                replay.bytes = replay.bytes.saturating_sub(evicted.encoded.len());
            }
        }
        drop(replay);
        let _ = self.events.send(sequenced);
    }

    async fn replay_after(&self, last_event_id: Option<u64>) -> ReplayBatch {
        let replay = self.replay.lock().await;
        let requested = last_event_id.unwrap_or_else(|| self.watermark());
        let oldest = replay.events.front().map(|event| event.seq);
        let gap = oldest.is_some_and(|oldest| requested.saturating_add(1) < oldest);
        let events = replay
            .events
            .iter()
            .filter(|event| event.seq > requested)
            .cloned()
            .collect();
        ReplayBatch { events, gap }
    }
}

struct ReplayBatch {
    events: Vec<SequencedEvent>,
    gap: bool,
}

pub(crate) async fn response(hub: Arc<LiveEventHub>, last_event_id: Option<u64>) -> Response {
    // Subscribe first. Anything published while the replay snapshot is captured can therefore
    // appear twice but cannot be lost; sequence de-duplication below removes the duplicate.
    let mut live = hub.events.subscribe();
    let replay = hub.replay_after(last_event_id).await;
    let output = stream! {
        let mut delivered = last_event_id.unwrap_or_default();
        if replay.gap {
            let seq = hub.watermark();
            yield Ok::<_, Infallible>(Frame::data(sse_json(seq, json!({
                "seq": seq,
                "op": "resync",
                "reason": "replayExpired"
            }))));
            delivered = seq;
        } else {
            for event in replay.events {
                delivered = delivered.max(event.seq);
                yield Ok(Frame::data(event.encoded));
            }
        }
        let mut keepalive = tokio::time::interval(Duration::from_secs(15));
        loop {
            tokio::select! {
                _ = keepalive.tick() => {
                    yield Ok(Frame::data(Bytes::from_static(b": keepalive\n\n")));
                }
                event = live.recv() => {
                    match event {
                        Ok(event) if event.seq > delivered => {
                            delivered = event.seq;
                            yield Ok(Frame::data(event.encoded));
                        }
                        Ok(_) => {}
                        Err(broadcast::error::RecvError::Lagged(_)) => {
                            let seq = hub.watermark();
                            delivered = seq;
                            yield Ok(Frame::data(sse_json(seq, json!({
                                "seq": seq,
                                "op": "resync",
                                "reason": "clientLagged"
                            }))));
                        }
                        Err(broadcast::error::RecvError::Closed) => break,
                    }
                }
            }
        }
    };
    let mut response = Response::new(Body::new(StreamBody::new(output)));
    response.headers_mut().insert(
        CONTENT_TYPE,
        HeaderValue::from_static("text/event-stream; charset=utf-8"),
    );
    response
        .headers_mut()
        .insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
        .headers_mut()
        .insert("x-accel-buffering", HeaderValue::from_static("no"));
    response
}

fn sse_json(seq: u64, event: Value) -> Bytes {
    Bytes::from(format!("id: {seq}\ndata: {event}\n\n"))
}

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
struct ItemKey {
    thread_id: String,
    turn_id: String,
    item_id: String,
}

#[derive(Default)]
struct EventRenderer {
    items: HashMap<ItemKey, Value>,
}

impl EventRenderer {
    async fn render(&mut self, message: Value) -> Option<Value> {
        let method = message.get("method")?.as_str()?;
        let params = message.get("params").unwrap_or(&Value::Null);
        let scope = Scope::from_params(params);
        match method {
            "item/started" => {
                let item = params.get("item")?.clone();
                let key = scope.item_key(item.get("id")?.as_str()?);
                self.items.insert(key.clone(), item.clone());
                self.render_item(&key, &item, PlanPresentation::Streaming)
                    .await
            }
            "item/completed" => {
                let item = params.get("item")?.clone();
                let key = scope.item_key(item.get("id")?.as_str()?);
                self.items.insert(key.clone(), item.clone());
                let presentation = if item.get("type").and_then(Value::as_str) == Some("plan") {
                    PlanPresentation::Completed
                } else {
                    PlanPresentation::Historical
                };
                self.render_item(&key, &item, presentation).await
            }
            "item/agentMessage/delta" => {
                self.append_text(scope, params, "agentMessage", "text")
                    .await
            }
            "item/plan/delta" => self.append_text(scope, params, "plan", "text").await,
            "item/commandExecution/outputDelta" => {
                self.append_text(scope, params, "commandExecution", "aggregatedOutput")
                    .await
            }
            "item/fileChange/outputDelta" => {
                self.append_text(scope, params, "fileChange", "output")
                    .await
            }
            "item/reasoning/textDelta" => {
                self.append_indexed(scope, params, "content", "contentIndex")
                    .await
            }
            "item/reasoning/summaryTextDelta" => {
                self.append_indexed(scope, params, "summary", "summaryIndex")
                    .await
            }
            "turn/started" => Some(scoped_event(
                &scope,
                json!({
                    "op": "turn",
                    "turnId": params.pointer("/turn/id").and_then(Value::as_str).unwrap_or_default(),
                    "status": "inProgress"
                }),
            )),
            "turn/completed" => Some(scoped_event(
                &scope,
                json!({
                    "op": "turn",
                    "turnId": params.pointer("/turn/id").and_then(Value::as_str).unwrap_or_default(),
                    "status": params.pointer("/turn/status").and_then(Value::as_str).unwrap_or("completed")
                }),
            )),
            "thread/status/changed" => Some(scoped_event(
                &scope,
                json!({
                    "op": "threadStatus",
                    "status": params.get("status").cloned().unwrap_or(Value::Null)
                }),
            )),
            "thread/settings/updated" => Some(scoped_event(
                &scope,
                json!({
                    "op": "threadSettings",
                    "activePermissionProfile": params.pointer("/threadSettings/activePermissionProfile").cloned(),
                    "approvalPolicy": params.pointer("/threadSettings/approvalPolicy").cloned(),
                    "sandboxPolicy": params.pointer("/threadSettings/sandboxPolicy").cloned()
                }),
            )),
            "thread/goal/updated" => Some(scoped_event(
                &scope,
                json!({
                    "op": "goal",
                    "goal": params.get("goal").cloned().unwrap_or(Value::Null)
                }),
            )),
            "thread/goal/cleared" => Some(scoped_event(
                &scope,
                json!({ "op": "goal", "goal": Value::Null }),
            )),
            "skills/changed" => Some(json!({ "op": "skillsChanged" })),
            "warning" | "guardianWarning" | "error" | "configWarning" => Some(scoped_event(
                &scope,
                json!({
                    "op": "notice",
                    "level": if method == "error" { "error" } else { "warning" },
                    "message": params.get("message").or_else(|| params.get("error")).and_then(Value::as_str).unwrap_or("Codex reported a problem")
                }),
            )),
            "mcpServer/oauthLogin/completed" => Some(json!({
                "op": "mcpAuth",
                "name": params.get("name").and_then(Value::as_str).unwrap_or_default(),
                "success": params.get("success").and_then(Value::as_bool).unwrap_or(false),
                "error": params.get("error").and_then(Value::as_str).unwrap_or_default()
            })),
            "web/requestResolved" => Some(json!({
                "op": "remove",
                "domId": format!("approval-{}", id_key(params.get("id")?))
            })),
            _ if message.get("id").is_some() => {
                let id = id_key(message.get("id")?);
                let cx = Cx::default();
                let view = approval_fragment(&cx, &message).await.ok()?;
                Some(scoped_event(
                    &scope,
                    json!({
                        "op": "upsert",
                        "domId": format!("approval-{id}"),
                        "html": view.render(&Cx::default())
                    }),
                ))
            }
            _ => None,
        }
    }

    async fn append_text(
        &mut self,
        scope: Scope,
        params: &Value,
        item_type: &str,
        field: &str,
    ) -> Option<Value> {
        let key = scope.item_key(params.get("itemId")?.as_str()?);
        let delta = params.get("delta")?.as_str()?;
        let item = self
            .items
            .entry(key.clone())
            .or_insert_with(|| json!({ "id": key.item_id, "type": item_type, field: "" }));
        let object = item.as_object_mut()?;
        let text = object
            .get(field)
            .and_then(Value::as_str)
            .unwrap_or_default();
        object.insert(
            field.to_string(),
            Value::String(truncated_append(text, delta)),
        );
        let item = item.clone();
        self.render_item(&key, &item, PlanPresentation::Streaming)
            .await
    }

    async fn append_indexed(
        &mut self,
        scope: Scope,
        params: &Value,
        field: &str,
        index_field: &str,
    ) -> Option<Value> {
        let key = scope.item_key(params.get("itemId")?.as_str()?);
        let delta = params.get("delta")?.as_str()?;
        let index = params.get(index_field)?.as_u64()? as usize;
        let item = self.items.entry(key.clone()).or_insert_with(
            || json!({ "id": key.item_id, "type": "reasoning", "summary": [], "content": [] }),
        );
        let values = item
            .as_object_mut()?
            .entry(field)
            .or_insert_with(|| json!([]));
        let values = values.as_array_mut()?;
        values.resize(index + 1, Value::String(String::new()));
        let text = values[index].as_str().unwrap_or_default();
        values[index] = Value::String(truncated_append(text, delta));
        let item = item.clone();
        self.render_item(&key, &item, PlanPresentation::Historical)
            .await
    }

    async fn render_item(
        &self,
        key: &ItemKey,
        item: &Value,
        plan_presentation: PlanPresentation,
    ) -> Option<Value> {
        let cx = Cx::default();
        if is_activity(item) {
            if !is_visible_activity(item) {
                return Some(json!({
                    "op": "remove",
                    "threadId": key.thread_id,
                    "turnId": key.turn_id,
                    "domId": format!("item-{}", key.item_id)
                }));
            }
            let row = activity_row(&cx, item).await.ok()?;
            let digest = work_digest(
                &cx,
                &key.turn_id,
                if matches!(
                    item.get("status").and_then(Value::as_str),
                    Some("inProgress" | "running")
                ) {
                    "inProgress"
                } else {
                    "completed"
                },
                std::slice::from_ref(item),
            )
            .await
            .ok()?;
            return Some(json!({
                "op": "activityUpsert",
                "threadId": key.thread_id,
                "turnId": key.turn_id,
                "itemId": key.item_id,
                "domId": format!("item-{}", key.item_id),
                "digestId": format!("work-{}", key.turn_id),
                "html": row.render(&Cx::default()),
                "digestHtml": digest.render(&Cx::default())
            }));
        }
        let view = if item.get("type").and_then(Value::as_str) == Some("plan") {
            item_fragment_with_plan_presentation(&cx, item, plan_presentation)
                .await
                .ok()?
        } else {
            item_fragment(&cx, item).await.ok()?
        };
        Some(json!({
            "op": "upsert",
            "threadId": key.thread_id,
            "turnId": key.turn_id,
            "itemId": key.item_id,
            "domId": format!("item-{}", key.item_id),
            "html": view.render(&Cx::default())
        }))
    }
}

#[derive(Clone, Default)]
struct Scope {
    thread_id: String,
    turn_id: String,
}

impl Scope {
    fn from_params(params: &Value) -> Self {
        Self {
            thread_id: params
                .get("threadId")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            turn_id: params
                .get("turnId")
                .and_then(Value::as_str)
                .or_else(|| params.pointer("/turn/id").and_then(Value::as_str))
                .unwrap_or_default()
                .to_string(),
        }
    }

    fn item_key(&self, item_id: &str) -> ItemKey {
        ItemKey {
            thread_id: self.thread_id.clone(),
            turn_id: self.turn_id.clone(),
            item_id: item_id.to_string(),
        }
    }
}

fn scoped_event(scope: &Scope, mut event: Value) -> Value {
    if let Some(object) = event.as_object_mut() {
        object.insert(
            "threadId".to_string(),
            Value::String(scope.thread_id.clone()),
        );
        object.insert("turnId".to_string(), Value::String(scope.turn_id.clone()));
    }
    event
}

fn truncated_append(existing: &str, delta: &str) -> String {
    const MAX_BYTES: usize = 256 * 1024;
    let mut combined = String::with_capacity(existing.len().saturating_add(delta.len()));
    combined.push_str(existing);
    combined.push_str(delta);
    if combined.len() <= MAX_BYTES {
        return combined;
    }
    let mut start = combined.len() - MAX_BYTES;
    while !combined.is_char_boundary(start) {
        start += 1;
    }
    format!("… output truncated …\n{}", &combined[start..])
}

fn id_key(id: &Value) -> String {
    id.as_str()
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| id.to_string())
}

#[cfg(test)]
#[path = "stream_tests.rs"]
mod tests;
