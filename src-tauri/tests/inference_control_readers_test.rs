//! Bounded public control admission for specify-inference-commercial-terms task 2.4.2.2.
//! One test in its own integration binary isolates the process-global control semaphore.
//! Paused Tokio time and body-poll notifications avoid sleeps and scheduler-timing guesses.
#[path = "support/proposal.rs"]
mod proposal;

use axum::{
    body::{to_bytes, Body, Bytes},
    extract::Request,
    http::{header, StatusCode},
    response::Response,
    Router,
};
use chiral_network::{billing_store::BillingStore, contracts, protocol::{self, Quote, Signed, QUOTE_DOMAIN}};
use futures_util::{stream, StreamExt};
use proposal::{quote_work_evidence, restore_quote_work, PreparedProvider};
use serde_json::{json, Value};
use std::{net::TcpListener, time::Duration};
use tokio::{
    sync::mpsc,
    task::{AbortHandle, JoinSet},
    time::{advance, timeout, Instant},
};
use tower::Service;

const READERS: usize = 64;
const BODY_BYTES: usize = 96 * 1024;
const DEADLINE: Duration = Duration::from_secs(5);
const IMMEDIATE: Duration = Duration::from_millis(100);
const PROPOSE: &str = "/v1/contracts/propose";

fn request(path: &str, body: Body) -> Request {
    Request::builder().method("POST").uri(path)
        .header(header::CONTENT_TYPE, "application/json").body(body).unwrap()
}

fn incomplete(started: Option<mpsc::UnboundedSender<()>>) -> Body {
    Body::from_stream(stream::once(async move {
        if let Some(started) = started { started.send(()).unwrap(); }
        Ok::<_, std::io::Error>(Bytes::from_static(b"{"))
    }).chain(stream::pending()))
}

async fn immediate(app: &mut Router, body: Body) -> Response {
    timeout(IMMEDIATE, app.call(request(PROPOSE, body))).await
        .expect("control admission must reject or finish without waiting for an occupied reader")
        .unwrap()
}

async fn error(response: Response, status: StatusCode, code: &str) {
    assert_eq!(response.status(), status);
    let bytes = to_bytes(response.into_body(), 4096).await.unwrap();
    let value: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(value["error"]["code"], code);
}

async fn fill(app: &Router, readers: &mut JoinSet<Response>) -> AbortHandle {
    let (started, mut observed) = mpsc::unbounded_channel();
    let mut last = None;
    for index in readers.len()..READERS {
        let mut app = app.clone();
        // Different public control routes must consume the same process-wide budget.
        let path = if index % 2 == 0 { PROPOSE } else { "/v1/auth/challenge" };
        let request = request(path, incomplete(Some(started.clone())));
        last = Some(readers.spawn(async move { app.call(request).await.unwrap() }));
        timeout(IMMEDIATE, observed.recv()).await
            .expect("each of the 64 admitted readers must actually poll its body")
            .unwrap();
    }
    last.expect("at least one slot must be vacant before refill")
}

async fn saturated(app: &mut Router) {
    let (started, mut observed) = mpsc::unbounded_channel();
    let before = Instant::now();
    error(immediate(app, incomplete(Some(started))).await,
        StatusCode::TOO_MANY_REQUESTS, "rate_limit").await;
    assert_eq!(Instant::now(), before, "saturated admission queued the request");
    assert!(observed.try_recv().is_err(), "a rejected request's body was polled");
}

async fn issued(response: Response, fixture: &PreparedProvider) {
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = to_bytes(response.into_body(), BODY_BYTES).await.unwrap();
    let quote: Signed<Quote> = serde_json::from_slice(&bytes).unwrap();
    protocol::verify(QUOTE_DOMAIN, &quote, &fixture.state.wallet).unwrap();
    assert_eq!(quote.payload.offer_id, fixture.offer_id);
    assert_eq!(quote.payload.model, fixture.offer.payload.model);
    assert_eq!(quote.payload.funding_wei, "100");
    let independent = BillingStore::open(&fixture.ledger_path, &fixture.state.config.network.identity).unwrap();
    let saved = independent.quote(&quote.payload.id).unwrap();
    assert_eq!(saved.payload, quote.payload);
    assert_eq!(saved.signature, quote.signature);
    assert!(independent.account(&quote.payload.id).is_err());
}

#[tokio::test(start_paused = true)]
async fn control_readers_bound_time_and_concurrency_and_recover_every_permit() {
    let directory = tempfile::tempdir().unwrap();
    // Reserve real loopback addresses without starting unused servers or making network calls.
    let public = TcpListener::bind("127.0.0.1:0").unwrap();
    let admin = TcpListener::bind("127.0.0.1:0").unwrap();
    let fixture = PreparedProvider::new(directory.path(), 20, "1", "2",
        public.local_addr().unwrap(), admin.local_addr().unwrap());
    let mut app = contracts::router(fixture.state.clone());
    // Explicitly restored future-window evidence pins the budget while Tokio time
    // advances independently of the signing wall clock; no HTTP work is claimed.
    let work_minute = protocol::now().unwrap() / 60 + 10;
    restore_quote_work(&fixture.ledger_path, work_minute, 598);
    let work = |count| assert_eq!(quote_work_evidence(&fixture.ledger_path),
        (work_minute.to_string(), work_minute, count));

    let before = Instant::now();
    let response = timeout(DEADLINE + Duration::from_secs(1),
        app.call(request(PROPOSE, incomplete(None)))).await
        .expect("an incomplete control body must have a five-second deadline").unwrap();
    assert_eq!(Instant::now() - before, DEADLINE);
    error(response, StatusCode::REQUEST_TIMEOUT, "request_timeout").await;
    work(598);

    let mut readers = JoinSet::new();
    let cancel = fill(&app, &mut readers).await;
    saturated(&mut app).await;
    work(598); // Neither the 64 blocked readers nor rejected reader reached signing.
    let held_since = Instant::now();
    cancel.abort();
    assert!(timeout(IMMEDIATE, readers.join_next()).await.unwrap().unwrap()
        .unwrap_err().is_cancelled());

    // Keep 63 readers occupied. Every following request needs the one reclaimed permit.
    let broken = Body::from_stream(stream::once(async {
        Err::<Bytes, _>(std::io::Error::new(std::io::ErrorKind::ConnectionReset,
            "reader-internal-detail"))
    }));
    let response = immediate(&mut app, broken).await;
    assert!(response.status().is_client_error());
    assert_ne!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    let bytes = to_bytes(response.into_body(), 4096).await.unwrap();
    assert!(!std::str::from_utf8(&bytes).unwrap().contains("reader-internal-detail"));
    error(immediate(&mut app, Body::from("{")).await,
        StatusCode::BAD_REQUEST, "invalid_request").await;

    let proposal = json!({"offer_id": fixture.offer_id,
        "consumer_wallet": "0x2222222222222222222222222222222222222222", "funding_wei": "100"});
    let mut padded = serde_json::to_vec(&proposal).unwrap();
    padded.resize(BODY_BYTES, b' ');
    let padded = Bytes::from(padded);
    // Both streams contain valid JSON; no Content-Length can stand in for counting bytes.
    let oversized = Body::from_stream(stream::iter([
        Ok::<_, std::io::Error>(padded.clone()), Ok(Bytes::from_static(b" ")),
    ]));
    error(immediate(&mut app, oversized).await,
        StatusCode::PAYLOAD_TOO_LARGE, "request_too_large").await;
    work(598); // Broken, malformed, oversized and expired bodies spend no work.
    let exact = Body::from_stream(stream::once(async move { Ok::<_, std::io::Error>(padded) }));
    issued(immediate(&mut app, exact).await, &fixture).await;
    work(599);

    fill(&app, &mut readers).await;
    saturated(&mut app).await;
    assert_eq!(Instant::now(), held_since, "error recovery waited for existing readers to expire");
    advance(DEADLINE).await;
    while !readers.is_empty() {
        let response = timeout(IMMEDIATE, readers.join_next()).await
            .expect("every timed-out reader must complete").unwrap().unwrap();
        error(response, StatusCode::REQUEST_TIMEOUT, "request_timeout").await;
    }
    work(599);

    // Reoccupy all 64 slots, not just one, to catch partial timeout cleanup leaks.
    fill(&app, &mut readers).await;
    saturated(&mut app).await;
    readers.abort_all();
    while !readers.is_empty() {
        assert!(timeout(IMMEDIATE, readers.join_next()).await.unwrap().unwrap()
            .unwrap_err().is_cancelled());
    }
    work(599); // Cancellation and saturation release readers, not signing budget.
    issued(immediate(&mut app, Body::from(serde_json::to_vec(&proposal).unwrap())).await,
        &fixture).await;
    work(600);
    error(immediate(&mut app, Body::from(serde_json::to_vec(&proposal).unwrap())).await,
        StatusCode::TOO_MANY_REQUESTS, "rate_limit").await;
    work(600);
}
