//! The model proxy. A worker attaches (`POST /attach`) and gets a key of its own and the model to
//! ask for, then sends its chat completions here (`POST /v1/chat/completions`, `Authorization:
//! Bearer <key>`) instead of to the provider. The proxy checks the key, forwards the request with
//! the server's model and provider pin and the provider key from this server's environment
//! ([`upstream`]), and logs each request's tokens and cost per worker and overall (`usage.log` in
//! the data dir, see [`usage`]). With [`Proxy::record`] it also hands each exchange (request,
//! reply, claim and timing, see [`record`]) to the server, which stores it in `model_exchange`
//! for replay.
//!
//! Workers never hold the provider key, so nothing a worker runs, or a model talks it into
//! printing, can leak it. A worker key opens only this proxy's completion endpoint, only until
//! the server stops. Attaching is open to whoever can reach the port, like the database itself:
//! keys scope and account for usage; they don't keep strangers out.

mod keys;
mod record;
#[cfg(test)]
mod tests;
mod upstream;
mod usage;

use std::convert::Infallible;
use std::net::SocketAddr;
use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError};
use std::time::{Duration, Instant, SystemTime};

use bytes::Bytes;
use http_body_util::{BodyExt, Full, LengthLimitError, Limited};
use hyper::body::Incoming;
use hyper::header::{AUTHORIZATION, CONTENT_TYPE};
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Method, Request, Response, StatusCode};
use hyper_util::rt::TokioIo;

use keys::Workers;
use record::Conversations;
pub use record::{Claim, Exchange};
use tokio::sync::mpsc;
pub use upstream::Upstream;
use usage::{Log, Tokens};

const ATTACH: &str = "/attach";
const COMPLETIONS: &str = "/v1/chat/completions";
/// An attach body is a few labels.
const MAX_ATTACH: usize = 4 * 1024;
/// A prompt with its context and the previous attempt's compiler errors.
const MAX_REQUEST: usize = 8 * 1024 * 1024;

type Reply = Response<Full<Bytes>>;

pub struct Proxy {
    listener: std::net::TcpListener,
    state: Arc<State>,
}

struct State {
    upstream: Upstream,
    ledger: Mutex<Ledger>,
    /// Where exchanges go to be recorded, once [`Proxy::record`] asked for them.
    recorder: OnceLock<mpsc::UnboundedSender<Exchange>>,
}

struct Ledger {
    workers: Workers,
    log: Log,
    conversations: Conversations,
}

impl State {
    /// A panic elsewhere can't corrupt counters enough to matter; keep serving.
    fn ledger(&self) -> MutexGuard<'_, Ledger> {
        self.ledger.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl Proxy {
    /// Bind `addr` (before anything else starts, so a busy port fails at once) and open the log.
    pub fn bind(addr: SocketAddr, upstream: Upstream, log: &Path) -> Result<Proxy, String> {
        let listener = std::net::TcpListener::bind(addr)
            .and_then(|l| l.set_nonblocking(true).map(|()| l))
            .map_err(|e| format!("model proxy on {addr}: {e}"))?;
        let mut log = Log::open(log)?;
        let to = upstream.describe();
        log.line(&format!(
            "proxy started, forwarding to {to}; totals start at zero"
        ));
        let workers = Workers::default();
        let conversations = Conversations::default();
        let ledger = Mutex::new(Ledger {
            workers,
            log,
            conversations,
        });
        let recorder = OnceLock::new();
        let state = Arc::new(State {
            upstream,
            ledger,
            recorder,
        });
        Ok(Proxy { listener, state })
    }

    /// Every exchange from now on, in the order they were answered. The receiver buffers them
    /// until the server has a database connection to record them with; asked for once.
    pub fn record(&self) -> mpsc::UnboundedReceiver<Exchange> {
        let (tx, rx) = mpsc::unbounded_channel();
        assert!(
            self.state.recorder.set(tx).is_ok(),
            "the proxy is recorded once"
        );
        rx
    }

    /// Where requests go and as what.
    pub fn upstream(&self) -> String {
        self.state.upstream.describe()
    }

    #[cfg(test)]
    pub fn addr(&self) -> SocketAddr {
        self.listener.local_addr().unwrap()
    }

    /// Serve until dropped; never returns `Ok`.
    pub async fn serve(self) -> Result<(), String> {
        let listener = tokio::net::TcpListener::from_std(self.listener)
            .map_err(|e| format!("model proxy: {e}"))?;
        loop {
            let stream = match listener.accept().await {
                Ok((stream, _)) => stream,
                // Out of file descriptors and the like: pass, don't stop serving.
                Err(e) => {
                    eprintln!("argo-server: model proxy: {e}");
                    tokio::time::sleep(Duration::from_millis(100)).await;
                    continue;
                }
            };
            let state = self.state.clone();
            tokio::spawn(async move {
                let service = service_fn(move |req| handle(state.clone(), req));
                // A worker hanging up mid-request is its own affair.
                let io = TokioIo::new(stream);
                let _ = http1::Builder::new().serve_connection(io, service).await;
            });
        }
    }
}

async fn handle(state: Arc<State>, req: Request<Incoming>) -> Result<Reply, Infallible> {
    let post = req.method() == Method::POST;
    let reply = match req.uri().path() {
        ATTACH if post => attach(&state, req).await,
        COMPLETIONS if post => complete(&state, req).await,
        _ => text(
            StatusCode::NOT_FOUND,
            "argo-server's model proxy serves POST /attach and POST /v1/chat/completions",
        ),
    };
    Ok(reply)
}

/// Mint a key for a new worker and log who it says it is.
async fn attach(state: &State, req: Request<Incoming>) -> Reply {
    let body = match read(req, MAX_ATTACH).await {
        Ok(body) => body,
        Err(reply) => return reply,
    };
    let mut ledger = state.ledger();
    let Ledger { workers, log, .. } = &mut *ledger;
    match workers.attach(label(&body)) {
        Ok((key, worker)) => {
            log.line(&format!("attach {} {}", worker.name(), worker.label));
            let model = state.upstream.model();
            let json = serde_json::json!({ "worker": worker.name(), "key": key, "model": model });
            respond(StatusCode::OK, json.to_string().into())
        }
        Err(e) => text(StatusCode::INTERNAL_SERVER_ERROR, &e),
    }
}

/// Check the worker's key, forward its request with the provider key, and count the reply.
async fn complete(state: &State, req: Request<Incoming>) -> Reply {
    let key = req
        .headers()
        .get(AUTHORIZATION)
        .and_then(|v| v.to_str().ok()?.strip_prefix("Bearer "))
        .map(str::to_string);
    let claim = claim_of(req.headers());
    let Some(key) = key.filter(|k| state.ledger().workers.get(k).is_some()) else {
        // The worker's cue to attach again: its key died with a previous server run.
        let why = "unknown worker key; POST /attach for a new one";
        return text(StatusCode::UNAUTHORIZED, why);
    };
    let body = match read(req, MAX_REQUEST).await {
        Ok(body) => body,
        Err(reply) => return reply,
    };
    let body = match state.upstream.rewrite(&body) {
        Ok(body) => body,
        Err(why) => return text(StatusCode::BAD_REQUEST, &why),
    };
    let sent_at = SystemTime::now();
    let started = Instant::now();
    let forwarded = state.upstream.complete(body.clone()).await;
    let ms = started.elapsed().as_millis();
    let mut via = "?".to_string();
    let (reply, status, tokens, response) = match forwarded {
        Err(why) => (
            text(StatusCode::BAD_GATEWAY, &why),
            "unreachable".into(),
            None,
            why,
        ),
        // Not the worker's key, so not a 401 to the worker, which would only attach again.
        Ok((s @ (401 | 403), _)) => {
            let why = format!("the provider refused argo-server's key ({s})");
            (
                text(StatusCode::BAD_GATEWAY, &why),
                s.to_string(),
                None,
                why,
            )
        }
        Ok((s, body)) => {
            let tokens = Tokens::of(&body);
            via = served_by(&body);
            let response = String::from_utf8_lossy(&body).into_owned();
            let status = StatusCode::from_u16(s).unwrap_or(StatusCode::BAD_GATEWAY);
            (respond(status, body), s.to_string(), tokens, response)
        }
    };
    let answered_at = SystemTime::now();
    let ok = status.starts_with('2');
    let mut ledger = state.ledger();
    let Ledger {
        workers,
        log,
        conversations,
    } = &mut *ledger;
    if let Some(worker) = workers.record(&key, ok, tokens) {
        if let Some(recorder) = state.recorder.get() {
            // The rewritten body parsed before, so it parses again.
            let sent: serde_json::Value = serde_json::from_slice(&body).unwrap_or_default();
            let (prefix, request) = conversations.delta(&worker.name(), claim, &sent);
            let _ = recorder.send(Exchange {
                worker: worker.name(),
                claim,
                sent_at,
                answered_at,
                status: status.parse().unwrap_or(0),
                tokens,
                prefix,
                request,
                response,
            });
        }
        let spent = tokens.map_or("?".into(), |t| t.to_string());
        let (name, usage) = (worker.name(), worker.usage);
        log.line(&format!(
            "{name} status={status} {ms}ms via={via} tokens={spent} | {name}: {usage} | all: {}",
            workers.total
        ));
    }
    reply
}

/// The claim a worker's request says it is for (`x-argo-chunk`, `x-argo-generation`).
fn claim_of(headers: &hyper::HeaderMap) -> Claim {
    let number = |name: &str| headers.get(name)?.to_str().ok()?.parse().ok();
    Claim {
        chunk: number("x-argo-chunk"),
        generation: number("x-argo-generation").and_then(|g: u64| u32::try_from(g).ok()),
    }
}

/// The body, at most `max` bytes.
async fn read(req: Request<Incoming>, max: usize) -> Result<Bytes, Reply> {
    match Limited::new(req.into_body(), max).collect().await {
        Ok(body) => Ok(body.to_bytes()),
        Err(e) if e.is::<LengthLimitError>() => Err(text(
            StatusCode::PAYLOAD_TOO_LARGE,
            &format!("request body over {max} bytes"),
        )),
        Err(e) => Err(text(StatusCode::BAD_REQUEST, &format!("request body: {e}"))),
    }
}

/// What the worker says it is (`{"host", "job", "identity"}`, all optional), as one log-safe line.
fn label(body: &[u8]) -> String {
    let json: serde_json::Value = serde_json::from_slice(body).unwrap_or_default();
    let label: Vec<String> = ["host", "job", "identity"]
        .into_iter()
        .filter_map(|field| {
            let value = match json.get(field)? {
                serde_json::Value::String(s) => s.clone(),
                other => other.to_string(),
            };
            Some(format!("{field}={}", clean(&value)))
        })
        .collect();
    if label.is_empty() {
        "unlabelled".into()
    } else {
        label.join(" ")
    }
}

/// Which provider OpenRouter routed the request to (its reply's `provider`), for the log.
fn served_by(body: &[u8]) -> String {
    let json: serde_json::Value = serde_json::from_slice(body).unwrap_or_default();
    json.get("provider")
        .and_then(serde_json::Value::as_str)
        .map_or("?".into(), clean)
}

/// A worker-supplied value kept to one short token: no spaces or newlines to forge log lines.
fn clean(value: &str) -> String {
    let keep = |c: &char| c.is_ascii_alphanumeric() || "._:/@-".contains(*c);
    value.chars().filter(keep).take(80).collect()
}

fn text(status: StatusCode, why: &str) -> Reply {
    let json = serde_json::json!({ "error": { "message": why } });
    respond(status, json.to_string().into())
}

fn respond(status: StatusCode, body: Bytes) -> Reply {
    let mut reply = Response::new(Full::new(body));
    *reply.status_mut() = status;
    let json = hyper::header::HeaderValue::from_static("application/json");
    reply.headers_mut().insert(CONTENT_TYPE, json);
    reply
}
