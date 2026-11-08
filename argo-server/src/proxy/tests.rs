//! The proxy end to end against a fake provider: attach, forward with the provider key, refuse
//! unknown keys, send every request as the server's model and provider, and log usage per worker
//! and overall without ever logging a key.

use std::convert::Infallible;
use std::net::SocketAddr;

use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::body::Incoming;
use hyper::header::AUTHORIZATION;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use serde_json::{Value, json};

use super::{Proxy, Upstream};

const REAL_KEY: &str = "sk-or-real-provider-key";
const MODEL: &str = "nvidia/nemotron-test";

/// A provider that answers only `REAL_KEY`, and only for `MODEL` pinned to `fireworks`, with
/// fixed usage and cost.
async fn provider() -> SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let (stream, _) = listener.accept().await.unwrap();
            let service = service_fn(|req: Request<Incoming>| async move {
                let auth = req.headers().get(AUTHORIZATION).cloned();
                let ok = auth.is_some_and(|v| v == format!("Bearer {REAL_KEY}").as_str());
                let body = req.into_body().collect().await.unwrap().to_bytes();
                let sent: Value = serde_json::from_slice(&body).unwrap();
                let pinned = json!({"order": ["fireworks"], "allow_fallbacks": false});
                let (status, body) = if !ok {
                    (StatusCode::UNAUTHORIZED, "{}".into())
                } else if sent["model"] != MODEL || sent["provider"] != pinned {
                    (StatusCode::BAD_REQUEST, sent.to_string())
                } else {
                    let usage = json!({"prompt_tokens": 100, "completion_tokens": 20,
                        "total_tokens": 120, "cost": 0.0001});
                    let reply = json!({"choices": [{"message": {"content": "fn f() {}"}}],
                        "usage": usage, "provider": "Fireworks"});
                    (StatusCode::OK, reply.to_string())
                };
                let mut reply = Response::new(Full::new(Bytes::from(body)));
                *reply.status_mut() = status;
                Ok::<_, Infallible>(reply)
            });
            tokio::spawn(http1::Builder::new().serve_connection(TokioIo::new(stream), service));
        }
    });
    addr
}

async fn proxy(provider: SocketAddr, key: &str, log: &std::path::Path) -> String {
    let base = format!("http://{provider}/");
    let upstream = Upstream::new(&base, key, MODEL, Some("fireworks")).unwrap();
    let proxy = Proxy::bind("127.0.0.1:0".parse().unwrap(), upstream, log).unwrap();
    let url = format!("http://{}", proxy.addr());
    tokio::spawn(proxy.serve());
    url
}

async fn post(url: &str, key: Option<&str>, body: Value) -> (u16, Value) {
    let mut req = reqwest::Client::new().post(url).body(body.to_string());
    if let Some(key) = key {
        req = req.bearer_auth(key);
    }
    let reply = req.send().await.unwrap();
    let status = reply.status().as_u16();
    (
        status,
        serde_json::from_slice(&reply.bytes().await.unwrap()).unwrap(),
    )
}

fn completion() -> Value {
    json!({"model": "nemotron-x", "messages": [{"role": "user", "content": "hi"}]})
}

#[tokio::test]
async fn workers_attach_and_complete_through_the_provider_key() {
    let dir = crate::scratch::dir("proxy");
    let log = dir.join("usage.log");
    let url = proxy(provider().await, REAL_KEY, &log).await;
    let completions = format!("{url}/v1/chat/completions");

    assert_eq!(post(&completions, None, completion()).await.0, 401);
    assert_eq!(
        post(&completions, Some("argo-made-up"), completion())
            .await
            .0,
        401
    );
    // The provider key itself is no worker key.
    assert_eq!(
        post(&completions, Some(REAL_KEY), completion()).await.0,
        401
    );

    let label = json!({"host": "box\nforged line", "job": 0, "identity": "c200ab"});
    let (status, attached) = post(&format!("{url}/attach"), None, label).await;
    assert_eq!(status, 200);
    assert_eq!(attached["worker"], "w1");
    assert_eq!(
        attached["model"], MODEL,
        "workers learn the model when they attach"
    );
    let key = attached["key"].as_str().unwrap().to_string();
    let (_, other) = post(&format!("{url}/attach"), None, json!({})).await;
    let other = other["key"].as_str().unwrap().to_string();

    let (status, reply) = post(&completions, Some(&key), completion()).await;
    assert_eq!(status, 200, "{reply}");
    assert_eq!(reply["choices"][0]["message"]["content"], "fn f() {}");
    post(&completions, Some(&key), completion()).await;
    assert_eq!(post(&completions, Some(&other), completion()).await.0, 200);

    let text = std::fs::read_to_string(&log).unwrap();
    assert!(
        text.contains("attach w1 host=boxforgedline job=0 identity=c200ab"),
        "{text}"
    );
    assert!(text.contains("attach w2 unlabelled"), "{text}");
    assert!(
        text.contains("forwarding to http://127.0.0.1:")
            && text.contains(" as nvidia/nemotron-test via provider fireworks only"),
        "{text}"
    );
    assert!(
        text.contains("w1 status=200 ")
            && text.contains(" via=Fireworks tokens=100+20=120 $0.000100 | w1: 2 requests (0 failed), tokens 200+40=240 $0.000200"),
        "{text}"
    );
    assert!(
        text.contains("| all: 3 requests (0 failed), tokens 300+60=360 $0.000300"),
        "{text}"
    );
    assert_eq!(text.lines().count(), 6, "{text}");
    assert!(!text.contains(&key) && !text.contains(REAL_KEY), "{text}");
}

#[tokio::test]
async fn a_refused_provider_key_is_a_gateway_error_not_a_dead_worker_key() {
    let dir = crate::scratch::dir("proxy-refused");
    let log = dir.join("usage.log");
    let url = proxy(provider().await, "wrong-provider-key", &log).await;
    let (_, attached) = post(&format!("{url}/attach"), None, json!({"host": "box"})).await;
    let key = attached["key"].as_str().unwrap();
    let (status, reply) = post(
        &format!("{url}/v1/chat/completions"),
        Some(key),
        completion(),
    )
    .await;
    assert_eq!(status, 502, "{reply}");
    let text = std::fs::read_to_string(&log).unwrap();
    assert!(text.contains("w1 status=401 "), "{text}");
    assert!(
        text.contains("tokens=? | w1: 1 requests (1 failed)"),
        "{text}"
    );
}

#[tokio::test]
async fn an_unreachable_provider_is_a_gateway_error() {
    let dir = crate::scratch::dir("proxy-down");
    let closed = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap();
    let url = proxy(closed, REAL_KEY, &dir.join("usage.log")).await;
    let (_, attached) = post(&format!("{url}/attach"), None, json!({})).await;
    let key = attached["key"].as_str().unwrap();
    let completions = format!("{url}/v1/chat/completions");
    assert_eq!(post(&completions, Some(key), completion()).await.0, 502);
    let (status, _) = post(&format!("{url}/v1/models"), Some(key), json!({})).await;
    assert_eq!(status, 404, "only chat completions are forwarded");
    let (status, _) = post(&completions, Some(key), json!(["not", "a", "request"])).await;
    assert_eq!(status, 400);
}

/// Each forwarded request comes out of `record` with its claim, cost and reply; the second turn
/// of a conversation carries only its new message.
#[tokio::test]
async fn exchanges_are_handed_over_for_recording() {
    let dir = crate::scratch::dir("proxy-record");
    let base = format!("http://{}/", provider().await);
    let upstream = Upstream::new(&base, REAL_KEY, MODEL, Some("fireworks")).unwrap();
    let proxy = Proxy::bind(
        "127.0.0.1:0".parse().unwrap(),
        upstream,
        &dir.join("usage.log"),
    )
    .unwrap();
    let mut exchanges = proxy.record();
    let url = format!("http://{}", proxy.addr());
    tokio::spawn(proxy.serve());
    let (_, attached) = post(&format!("{url}/attach"), None, json!({})).await;
    let key = attached["key"].as_str().unwrap().to_string();

    let send = |body: Value| {
        let req = reqwest::Client::new()
            .post(format!("{url}/v1/chat/completions"))
            .bearer_auth(&key)
            .header("x-argo-chunk", "12")
            .header("x-argo-generation", "3")
            .body(body.to_string());
        async move { assert_eq!(req.send().await.unwrap().status().as_u16(), 200) }
    };
    send(completion()).await;
    let mut turn = completion();
    turn["messages"]
        .as_array_mut()
        .unwrap()
        .push(json!({"role": "tool", "content": "no"}));
    send(turn).await;

    let first = exchanges.recv().await.unwrap();
    assert_eq!(
        (first.worker.as_str(), first.status, first.prefix),
        ("w1", 200, 0)
    );
    assert_eq!(
        (first.claim.chunk, first.claim.generation),
        (Some(12), Some(3))
    );
    assert_eq!(first.tokens.unwrap().cost, 0.0001);
    let sent: Value = serde_json::from_str(&first.request).unwrap();
    assert_eq!(sent["model"], MODEL, "recorded as sent upstream");
    assert!(first.response.contains("fn f() {}") && first.answered_at >= first.sent_at);
    let second = exchanges.recv().await.unwrap();
    let delta: Value = serde_json::from_str(&second.request).unwrap();
    assert_eq!(second.prefix, 1);
    assert_eq!(
        delta["messages"],
        json!([{"role": "tool", "content": "no"}])
    );
}
