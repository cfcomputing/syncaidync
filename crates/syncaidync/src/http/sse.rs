//! Server-sent event hub.

use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;

use futures_util::stream;
use http_body_util::{BodyExt, StreamBody};
use hyper::body::{Bytes, Frame};
use hyper::header::{CACHE_CONTROL, CONTENT_TYPE};
use hyper::{Response, StatusCode};
use serde::Serialize;
use tokio::sync::broadcast;
use tokio::time::{interval, MissedTickBehavior};

use super::error::ApiError;
use super::Body;

const CAP: usize = 64;

#[derive(Clone)]
pub struct Hub {
    tx: broadcast::Sender<Arc<str>>,
}

impl Hub {
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(CAP);
        Self { tx }
    }

    pub fn publish(&self, event: &str, data: &impl Serialize) {
        let Ok(payload) = serde_json::to_string(data) else {
            return;
        };
        let msg = format!("event: {event}\ndata: {payload}\n\n");
        let _ = self.tx.send(Arc::from(msg));
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Arc<str>> {
        self.tx.subscribe()
    }
}

impl Default for Hub {
    fn default() -> Self {
        Self::new()
    }
}

/// Close a lagging stream so EventSource reconnects for current state.
pub fn stream_response(
    rx: broadcast::Receiver<Arc<str>>,
    initial_state: impl Serialize,
) -> Result<Response<Body>, ApiError> {
    let payload = serde_json::to_string(&initial_state).map_err(|_| ApiError::internal())?;
    let initial = format!("event: state\ndata: {payload}\n\n");

    let stream = stream::unfold(
        SseState {
            rx,
            initial: Some(initial),
            ping: {
                let mut i = interval(Duration::from_secs(20));
                i.set_missed_tick_behavior(MissedTickBehavior::Delay);
                i
            },
            skip_first_ping: true,
        },
        |mut st| async move {
            if let Some(init) = st.initial.take() {
                return Some((Ok::<_, Infallible>(Frame::data(Bytes::from(init))), st));
            }
            if st.skip_first_ping {
                st.ping.tick().await;
                st.skip_first_ping = false;
            }
            tokio::select! {
                msg = st.rx.recv() => {
                    match msg {
                        Ok(text) => {
                            Some((
                                Ok(Frame::data(Bytes::from(text.as_ref().to_owned()))),
                                st,
                            ))
                        }
                        Err(_) => None,
                    }
                }
                _ = st.ping.tick() => {
                    Some((Ok(Frame::data(Bytes::from(": ping\n\n"))), st))
                }
            }
        },
    );

    let body = BodyExt::boxed(StreamBody::new(stream));
    let mut res = Response::new(body);
    *res.status_mut() = StatusCode::OK;
    res.headers_mut().insert(
        CONTENT_TYPE,
        hyper::header::HeaderValue::from_static("text/event-stream"),
    );
    res.headers_mut().insert(
        CACHE_CONTROL,
        hyper::header::HeaderValue::from_static("no-store"),
    );
    crate::http::security_headers(&mut res, false, None);
    Ok(res)
}

struct SseState {
    rx: broadcast::Receiver<Arc<str>>,
    initial: Option<String>,
    ping: tokio::time::Interval,
    skip_first_ping: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn lag_ends_the_stream_for_reconnect() {
        let hub = Hub::new();
        let mut body = stream_response(hub.subscribe(), serde_json::json!({}))
            .unwrap()
            .into_body();
        let initial = body.frame().await.unwrap().unwrap().into_data().unwrap();
        assert!(initial.starts_with(b"event: state"));

        for i in 0..=CAP {
            hub.publish("session", &i);
        }

        assert!(body.frame().await.is_none());
    }
}
