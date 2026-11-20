//! Native Messaging stdin/stdout round-trip for `posthaze-cli native-host`.
//!
//! Guards against the host hanging Chrome's pipe: a `ping` frame must produce one
//! length-prefixed JSON reply and the process must exit promptly.

use std::io::{Read, Write};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[test]
fn native_host_ping_replies_and_exits_promptly() {
    let dir = tempfile::TempDir::new().unwrap();
    let db_path = dir.path().join("state.db");
    let bin = env!("CARGO_BIN_EXE_posthaze-cli");

    let mut child = Command::new(bin)
        .args(["--db", db_path.to_str().unwrap(), "native-host"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();

    let payload = br#"{"type":"ping"}"#;
    let mut stdin = child.stdin.take().unwrap();
    stdin
        .write_all(&u32::try_from(payload.len()).unwrap().to_le_bytes())
        .unwrap();
    stdin.write_all(payload).unwrap();
    stdin.flush().unwrap();
    drop(stdin);

    let mut stdout = child.stdout.take().unwrap();
    let mut len_buf = [0_u8; 4];
    stdout.read_exact(&mut len_buf).unwrap();
    let mut body = vec![0_u8; u32::from_le_bytes(len_buf) as usize];
    stdout.read_exact(&mut body).unwrap();
    let reply: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(reply["ok"], true, "unexpected reply: {reply}");

    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success());
            break;
        }
        assert!(Instant::now() < deadline, "native host did not exit");
        std::thread::sleep(Duration::from_millis(50));
    }
}
