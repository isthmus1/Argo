//! The proxy access path against a fake `argo-server` proxy: attach, a refused key (the server
//! restarted) attaches again without spending an attempt, and the new key works.

use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

use super::{Access, Message, Model, ModelError, Nemotron, Reply, classify};
use crate::proxy::Proxy;

/// Mints `k1`, `k2`, ...; refuses `k1` (as a restarted server would) and answers any other key.
async fn fake_proxy() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move {
        let mut minted = 0;
        loop {
            let (mut stream, _) = listener.accept().await.unwrap();
            let request = read_request(&mut stream).await;
            let (status, body) = if request.starts_with("POST /attach ") {
                minted += 1;
                assert!(request.contains(r#""job":3"#), "{request}");
                (
                    "200 OK",
                    format!(r#"{{"key":"k{minted}","worker":"w{minted}","model":"nvidia/m"}}"#),
                )
            } else if request.contains("authorization: Bearer k1\r\n") {
                (
                    "401 Unauthorized",
                    r#"{"error":{"message":"unknown worker key"}}"#.into(),
                )
            } else {
                assert!(
                    request.starts_with("POST /v1/chat/completions "),
                    "{request}"
                );
                let reply = r#"{"choices":[{"message":{"content":"fn ok() {}"}}]}"#;
                ("200 OK", reply.into())
            };
            let reply = format!(
                "HTTP/1.1 {status}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            );
            stream.write_all(reply.as_bytes()).await.unwrap();
        }
    });
    url
}

/// Headers (lowercased names, as reqwest sends them) and the body, by `content-length`.
async fn read_request(stream: &mut tokio::net::TcpStream) -> String {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        let n = stream.read(&mut chunk).await.unwrap();
        buf.extend_from_slice(&chunk[..n]);
        let text = String::from_utf8_lossy(&buf).to_string();
        if let Some(end) = text.find("\r\n\r\n") {
            let length = text[..end]
                .lines()
                .find_map(|l| l.strip_prefix("content-length: "))
                .map_or(0, |n| n.trim().parse().unwrap());
            if buf.len() >= end + 4 + length || n == 0 {
                return text;
            }
        }
    }
}

#[tokio::test]
async fn a_refused_worker_key_attaches_again_without_spending_an_attempt() {
    let url = fake_proxy().await;
    let proxy = Proxy::attach(&url, 3, "c200ab").await.unwrap();
    assert_eq!((proxy.name(), proxy.key()), ("w1".into(), "k1".into()));
    assert_eq!(proxy.model(), "nvidia/m");
    let model = Nemotron::new(Duration::from_secs(10), Access::Proxy(proxy));
    let ask = [Message::User("prompt".into())];
    match model.chat("m", &ask, &[]).await {
        Err(ModelError::Unavailable(why)) => assert!(why.contains("attached again as w2"), "{why}"),
        other => panic!("{other:?}"),
    }
    let reply = Reply {
        content: Some("fn ok() {}".into()),
        calls: vec![],
    };
    assert_eq!(model.chat("m", &ask, &[]).await, Ok(reply));
}

#[tokio::test]
async fn an_absent_proxy_says_how_to_go_without_it() {
    let closed = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", closed.local_addr().unwrap());
    drop(closed);
    let why = Proxy::attach(&url, 0, "c200ab").await.err().unwrap();
    assert!(
        why.contains("cannot attach to the model proxy") && why.contains("--direct"),
        "{why}"
    );
}

#[test]
fn rejected_requests_count_and_endpoint_failures_wait() {
    assert!(
        matches!(classify(400, "context too long"), ModelError::Reply(w) if w.contains("context too long"))
    );
    assert!(matches!(classify(413, ""), ModelError::Reply(_)));
    assert!(matches!(classify(429, ""), ModelError::Unavailable(_)));
    assert!(matches!(classify(503, ""), ModelError::Unavailable(_)));
    assert!(matches!(classify(401, ""), ModelError::Unavailable(_)));
    // A provider pinned without fallbacks that can't call tools: no wait will fix it.
    let no_tools =
        r#"{"error":{"message":"No endpoints found that support tool use.","code":404}}"#;
    assert!(matches!(classify(404, no_tools), ModelError::Reply(_)));
    assert!(matches!(
        classify(404, "not found"),
        ModelError::Unavailable(_)
    ));
}

/// A proxy that answers everything and hands over each completion request it got.
async fn capturing_proxy() -> (String, tokio::sync::mpsc::UnboundedReceiver<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    tokio::spawn(async move {
        loop {
            let (mut stream, _) = listener.accept().await.unwrap();
            let request = read_request(&mut stream).await;
            let body = if request.starts_with("POST /attach ") {
                r#"{"key":"k","worker":"w1","model":"nvidia/m"}"#
            } else {
                tx.send(request).unwrap();
                r#"{"choices":[{"message":{"content":"ok"}}]}"#
            };
            let reply = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            );
            stream.write_all(reply.as_bytes()).await.unwrap();
        }
    });
    (url, rx)
}

#[tokio::test]
async fn proxied_requests_name_the_claim_they_are_for() {
    let (url, mut seen) = capturing_proxy().await;
    let proxy = Proxy::attach(&url, 0, "c200ab").await.unwrap();
    let model = Nemotron::new(Duration::from_secs(10), Access::Proxy(proxy));
    let ask = [Message::User("prompt".into())];
    model.chat("m", &ask, &[]).await.unwrap();
    let unclaimed = seen.recv().await.unwrap();
    assert!(!unclaimed.contains("x-argo-chunk"), "{unclaimed}");
    super::CLAIM
        .scope((42, 7), model.chat("m", &ask, &[]))
        .await
        .unwrap();
    let claimed = seen.recv().await.unwrap();
    assert!(
        claimed.contains("x-argo-chunk: 42\r\n") && claimed.contains("x-argo-generation: 7\r\n"),
        "{claimed}"
    );
}
