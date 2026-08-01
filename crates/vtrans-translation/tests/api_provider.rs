//! Integration tests for `ApiTranslationProvider` against a local HTTP server.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use vtrans_core::types::{Language, TranslationRequest};
use vtrans_core::TranslationProvider;
use vtrans_translation::{ApiTranslationProvider, RetryPolicy};

/// Build a translation request for tests.
fn request() -> TranslationRequest {
    TranslationRequest::new("hello", Language::English, Language::Japanese)
}

/// Provider with zero retries and a short timeout, pointing at `endpoint`.
fn provider(endpoint: &str, timeout: Duration, retries: u32) -> ApiTranslationProvider {
    ApiTranslationProvider::new(endpoint, "test-model", "test-key", timeout, retries)
}

/// Read one HTTP request's headers (enough for the small test bodies).
async fn read_request(socket: &mut TcpStream) -> Option<Vec<u8>> {
    let mut buffer = [0_u8; 8192];
    let read = socket.read(&mut buffer).await.ok()?;
    if read == 0 {
        return None;
    }
    Some(buffer[..read].to_vec())
}

/// Write a minimal HTTP response.
async fn write_response(socket: &mut TcpStream, status: u16, body: &str) {
    let reason = match status {
        200 => "OK",
        401 => "Unauthorized",
        429 => "Too Many Requests",
        _ => "Internal Server Error",
    };
    let response = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = socket.write_all(response.as_bytes()).await;
}

/// Spawn a server that pops one queued response per request.
async fn spawn_server(
    responses: Vec<(u16, String)>,
) -> (String, Arc<Mutex<VecDeque<(u16, String)>>>, JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let queue = Arc::new(Mutex::new(VecDeque::from(responses)));
    let queue_for_server = Arc::clone(&queue);

    let handle = tokio::spawn(async move {
        loop {
            let Ok((mut socket, _)) = listener.accept().await else {
                break;
            };
            if read_request(&mut socket).await.is_none() {
                continue;
            }
            let (status, body) = {
                let mut queue = queue_for_server.lock().unwrap();
                queue.pop_front().unwrap_or((500, "{}".to_string()))
            };
            write_response(&mut socket, status, &body).await;
        }
    });

    (format!("http://{address}"), queue, handle)
}

/// Spawn a server that accepts requests but never responds.
async fn spawn_hanging_server() -> (String, JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();

    let handle = tokio::spawn(async move {
        loop {
            let Ok((mut socket, _)) = listener.accept().await else {
                break;
            };
            tokio::spawn(async move {
                let mut buffer = [0_u8; 8192];
                let _ = socket.read(&mut buffer).await;
                std::future::pending::<()>().await;
            });
        }
    });

    (format!("http://{address}"), handle)
}

/// Build the provider used by retry tests: zero backoff, bounded retries.
fn retry_provider(endpoint: &str, retries: u32) -> ApiTranslationProvider {
    provider(endpoint, Duration::from_secs(5), retries)
        .with_retry_policy(RetryPolicy::new(retries).with_limits(Duration::ZERO, Duration::ZERO))
}

#[tokio::test]
async fn success_response_returns_translation() {
    let body = r#"{"choices":[{"message":{"content":"こんにちは"}}]}"#.to_string();
    let (endpoint, queue, server) = spawn_server(vec![(200, body)]).await;
    let result = provider(&endpoint, Duration::from_secs(5), 0)
        .translate(&request(), CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(result.translated_text, "こんにちは");
    assert_eq!(result.provider_id, "api");
    assert!(queue.lock().unwrap().is_empty());
    server.abort();
}

#[tokio::test]
async fn http_401_returns_unauthorized() {
    let (endpoint, _, server) = spawn_server(vec![(401, "{}".to_string())]).await;
    let error = provider(&endpoint, Duration::from_secs(5), 0)
        .translate(&request(), CancellationToken::new())
        .await
        .unwrap_err();
    assert!(matches!(error, vtrans_core::TranslationError::Unauthorized));
    server.abort();
}

#[tokio::test]
async fn http_429_returns_rate_limited() {
    let (endpoint, _, server) = spawn_server(vec![(429, "{}".to_string())]).await;
    let error = provider(&endpoint, Duration::from_secs(5), 0)
        .translate(&request(), CancellationToken::new())
        .await
        .unwrap_err();
    assert!(matches!(error, vtrans_core::TranslationError::RateLimited));
    server.abort();
}

#[tokio::test]
async fn retries_until_success() {
    let body = r#"{"choices":[{"text":"Bonjour"}]}"#.to_string();
    let responses = vec![
        (500, "{}".to_string()),
        (500, "{}".to_string()),
        (200, body),
    ];
    let (endpoint, queue, server) = spawn_server(responses).await;
    let result = retry_provider(&endpoint, 2)
        .translate(&request(), CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(result.translated_text, "Bonjour");
    assert!(queue.lock().unwrap().is_empty());
    server.abort();
}

#[tokio::test]
async fn gives_up_after_max_retries() {
    let responses = vec![
        (500, "{}".to_string()),
        (500, "{}".to_string()),
        (500, "{}".to_string()),
        (500, "{}".to_string()),
    ];
    let (endpoint, queue, server) = spawn_server(responses).await;
    let error = retry_provider(&endpoint, 2)
        .translate(&request(), CancellationToken::new())
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        vtrans_core::TranslationError::ApiRequest(_)
    ));
    assert_eq!(queue.lock().unwrap().len(), 1);
    server.abort();
}

#[tokio::test]
async fn timeout_returns_timeout() {
    let (endpoint, server) = spawn_hanging_server().await;
    let error = provider(&endpoint, Duration::from_millis(200), 0)
        .translate(&request(), CancellationToken::new())
        .await
        .unwrap_err();
    assert!(matches!(error, vtrans_core::TranslationError::Timeout(_)));
    server.abort();
}

#[tokio::test]
async fn cancellation_returns_cancelled() {
    let (endpoint, server) = spawn_hanging_server().await;
    let cancel = CancellationToken::new();
    let cancel_clone = cancel.clone();
    let request = request();
    let provider = provider(&endpoint, Duration::from_secs(30), 0);

    let handle = tokio::spawn(async move { provider.translate(&request, cancel_clone).await });
    tokio::time::sleep(Duration::from_millis(100)).await;
    cancel.cancel();
    let error = handle.await.unwrap().unwrap_err();
    assert!(matches!(error, vtrans_core::TranslationError::Cancelled));
    server.abort();
}
