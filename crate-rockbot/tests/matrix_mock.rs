/// Mock-backed integration tests for MatrixPlatform.
///
/// Verifies that M_UNKNOWN_TOKEN errors from the Matrix sync endpoint
/// are detected and mapped to RockBotError::AuthFailed, triggering
/// the force_relogin mechanism for the next connect cycle.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use rockbot::config::MatrixServerConfig;
use rockbot::error::RockBotError;
use rockbot::platform::{MatrixPlatform, MessageHandler, MessagingClient};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

fn noop_handler() -> MessageHandler {
    Box::new(|_msg, _sender| Box::pin(async {}))
}

fn unique_state_dir(label: &str) -> String {
    let n = TEST_COUNTER.fetch_add(1, Ordering::SeqCst);
    format!("./tmp/matrix-test-{label}-{n}")
}

async fn mount_versions_mock(mock_server: &MockServer) {
    Mock::given(method("GET"))
        .and(path("/_matrix/client/versions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "versions": ["v1.11"],
            "unstable_features": {}
        })))
        .mount(mock_server)
        .await;
}

fn login_response() -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(serde_json::json!({
        "user_id": "@testbot:example.org",
        "access_token": "fake-access-token",
        "device_id": "TESTDEVICE"
    }))
}

fn unknown_token_sync_response() -> ResponseTemplate {
    ResponseTemplate::new(401).set_body_json(serde_json::json!({
        "errcode": "M_UNKNOWN_TOKEN",
        "error": "Access token has expired",
        "soft_logout": false
    }))
}

#[tokio::test]
async fn test_unknown_token_maps_to_auth_failed() {
    let mock_server = MockServer::start().await;

    mount_versions_mock(&mock_server).await;

    Mock::given(method("POST"))
        .and(path("/_matrix/client/v3/login"))
        .respond_with(login_response())
        .mount(&mock_server)
        .await;

    Mock::given(method("GET"))
        .and(path("/_matrix/client/v3/sync"))
        .respond_with(unknown_token_sync_response())
        .mount(&mock_server)
        .await;

    let state_dir = unique_state_dir("unknown-token");
    std::fs::remove_dir_all(&state_dir).ok();

    let config = MatrixServerConfig {
        homeserver: mock_server.uri(),
        user_id: "@testbot:example.org".to_string(),
        password: "testpass".to_string(),
        device_id: Some("TESTDEVICE".to_string()),
        state_dir: state_dir.clone(),
    };

    let platform = MatrixPlatform::new(&config);

    let result = tokio::time::timeout(
        Duration::from_secs(30),
        platform.connect_and_run(noop_handler()),
    )
    .await;

    std::fs::remove_dir_all(&state_dir).ok();

    assert!(result.is_ok(), "connect_and_run timed out after 30s");
    let inner = result.unwrap();
    assert!(inner.is_err(), "expected error from connect_and_run");
    match inner.unwrap_err() {
        RockBotError::AuthFailed(msg) => {
            assert!(
                msg.contains("M_UNKNOWN_TOKEN")
                    || msg.contains("UnknownToken")
                    || msg.contains("sync"),
                "error message should reference the token error, got: {msg}"
            );
        }
        other => panic!("expected AuthFailed, got: {other:?}"),
    }
}

#[tokio::test]
async fn test_force_relogin_after_unknown_token() {
    let mock_server = MockServer::start().await;

    mount_versions_mock(&mock_server).await;

    // Login mock: expect 2 calls (initial + forced re-login after M_UNKNOWN_TOKEN)
    Mock::given(method("POST"))
        .and(path("/_matrix/client/v3/login"))
        .respond_with(login_response())
        .expect(2)
        .mount(&mock_server)
        .await;

    Mock::given(method("GET"))
        .and(path("/_matrix/client/v3/sync"))
        .respond_with(unknown_token_sync_response())
        .mount(&mock_server)
        .await;

    let state_dir = unique_state_dir("force-relogin");
    std::fs::remove_dir_all(&state_dir).ok();

    let config = MatrixServerConfig {
        homeserver: mock_server.uri(),
        user_id: "@testbot:example.org".to_string(),
        password: "testpass".to_string(),
        device_id: Some("TESTDEVICE".to_string()),
        state_dir: state_dir.clone(),
    };

    let platform = MatrixPlatform::new(&config);

    // First connect: login + sync returns M_UNKNOWN_TOKEN -> AuthFailed
    let result1 = tokio::time::timeout(
        Duration::from_secs(30),
        platform.connect_and_run(noop_handler()),
    )
    .await;
    assert!(result1.is_ok(), "first connect_and_run timed out");
    let inner1 = result1.unwrap();
    assert!(matches!(inner1.unwrap_err(), RockBotError::AuthFailed(_)));

    // Second connect: force_relogin should be true -> login again (not session restore)
    let result2 = tokio::time::timeout(
        Duration::from_secs(30),
        platform.connect_and_run(noop_handler()),
    )
    .await;

    std::fs::remove_dir_all(&state_dir).ok();

    assert!(result2.is_ok(), "second connect_and_run timed out");
    let inner2 = result2.unwrap();
    assert!(
        matches!(inner2.unwrap_err(), RockBotError::AuthFailed(_)),
        "second connect should also get AuthFailed from M_UNKNOWN_TOKEN"
    );
    // WireMock verifies login was called exactly 2 times on drop
}
