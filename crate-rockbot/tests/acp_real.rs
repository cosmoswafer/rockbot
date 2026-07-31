// ─── Live Integration Probe: ACP Delegate vs real agent ─────────────────────
//
// DFD covered: _dfd/tools/acp-delegate.md
//
// Ignored by default — requires `deno` on PATH (spawns `deno x opencode-ai
// acp` as the ACP agent). Run with:
//
//   cargo test -p rockbot --test acp_real -- --ignored --nocapture
//
// Exercises the production spawn path (env allowlist, cwd, kill_on_drop) that
// the duplex-stream mock in integration_mock_acp.rs cannot cover.

use rockbot::acp::AcpClient;
use rockbot::config::AcpConfig;

#[tokio::test]
#[ignore]
async fn test_acp_real_opencode_end_turn() {
    let cfg = AcpConfig {
        enabled: true,
        command: "deno".into(),
        args: vec!["x".into(), "opencode-ai".into(), "acp".into()],
        cwd: "./tmp".into(),
        session_cwd: "./tmp".into(),
        timeout_secs: 300,
        ..AcpConfig::default()
    };
    let client = AcpClient::new(cfg);

    let result = client
        .prompt("What is 2+2? Reply with just the number.", 300)
        .await
        .expect("prompt against real opencode ACP agent should succeed");

    println!("stop_reason: {:?}", result.stop_reason);
    println!("truncated: {}", result.truncated);
    println!("text: {}", result.text);
    assert!(result.text.contains('4'), "expected '4' in output: {}", result.text);

    // Second prompt reuses the same session.
    let result2 = client
        .prompt("And 3+3? Reply with just the number.", 300)
        .await
        .expect("second prompt should succeed");
    assert!(result2.text.contains('6'), "expected '6' in output: {}", result2.text);

    client.shutdown().await;
}
