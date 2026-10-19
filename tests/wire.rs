//! Wire-level tests: drive the built sidecar binary over stdio like the
//! gray host does (numeric host→sidecar ids, notifications carry no id).
//! Each test gets a fresh GRAY_HOME so goal docs never leak between runs.

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::time::Duration;

struct Harness {
    child: Child,
    stdin: ChildStdin,
    lines: std::sync::mpsc::Receiver<String>,
    _reader: std::thread::JoinHandle<()>,
    _home: tempfile::TempDir,
}

impl Harness {
    fn spawn() -> Self {
        let home = tempfile::tempdir().expect("temp home");
        let mut child = Command::new(env!("CARGO_BIN_EXE_gray-goal"))
            .arg("sidecar")
            .env("GRAY_HOME", home.path())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn gray-goal sidecar");
        let stdin = child.stdin.take().expect("stdin");
        let stdout: ChildStdout = child.stdout.take().expect("stdout");
        let (tx, rx) = std::sync::mpsc::channel();
        let reader = std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if tx.send(line).is_err() {
                    break;
                }
            }
        });
        Self {
            child,
            stdin,
            lines: rx,
            _reader: reader,
            _home: home,
        }
    }

    fn send(&mut self, v: &serde_json::Value) {
        writeln!(self.stdin, "{v}").expect("write");
        self.stdin.flush().expect("flush");
    }

    fn request(&mut self, id: u64, method: &str, params: serde_json::Value) -> serde_json::Value {
        self.send(&serde_json::json!({"id": id, "method": method, "params": params}));
        let line = self
            .lines
            .recv_timeout(Duration::from_secs(10))
            .expect("reply within 10s");
        let v: serde_json::Value = serde_json::from_str(&line).expect("valid json reply");
        assert_eq!(v["id"], id);
        v
    }

    fn command(&mut self, id: u64, argv: &[&str]) -> serde_json::Value {
        self.request(
            id,
            "command/run",
            serde_json::json!({"name": "/goal", "argv": argv, "session": {"id": "t", "cwd": "/tmp"}}),
        )
    }

    fn tool(&mut self, id: u64, args: serde_json::Value) -> serde_json::Value {
        self.request(
            id,
            "tool/call",
            serde_json::json!({"name": "goal", "args": args, "session": {"id": "t", "cwd": "/tmp"}}),
        )
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[test]
fn manifest_shape() {
    let mut h = Harness::spawn();
    let m = h.request(1, "plugin/manifest", serde_json::json!({}));
    let r = &m["result"];
    assert_eq!(r["name"], "goal");
    assert_eq!(r["protocol"], "1.1");
    assert_eq!(r["sidecar_args"], serde_json::json!(["sidecar"]));
    assert_eq!(r["commands"], serde_json::json!(["/goal"]));
    assert_eq!(r["tools"][0]["name"], "goal");
    assert!(r["hooks"]
        .as_array()
        .unwrap()
        .iter()
        .any(|h| h == "prompt/context"));
}

#[test]
fn lifecycle_create_plan_nudge() {
    let mut h = Harness::spawn();
    // bare /goal → status, no goal yet
    let r = h.command(1, &[]);
    assert!(r["result"]["text"]
        .as_str()
        .unwrap()
        .contains("no active goal"));
    // /goal <desc> → created; reply carries the planning prompt
    let r = h.command(2, &["add", "csv", "export"]);
    assert!(r["result"]["text"].as_str().unwrap().contains("created"));
    assert!(r["result"]["prompt"]
        .as_str()
        .unwrap()
        .contains("set_items"));
    // agent drafts the checklist via the tool
    let r = h.tool(
        3,
        serde_json::json!({"action": "set_items", "items": ["a", "b"]}),
    );
    assert_ne!(r["result"]["is_error"], true);
    // claiming done with open items is refused + lists them
    let r = h.tool(4, serde_json::json!({"action": "complete"}));
    assert_eq!(r["result"]["is_error"], true);
    assert!(r["result"]["content"]
        .as_str()
        .unwrap()
        .contains("2 item(s) still open"));
    // the refused claim arms the agent/before_start nudge — fires once
    let n = h.request(
        5,
        "agent/before_start",
        serde_json::json!({"session": {"id": "t", "cwd": "/tmp"}}),
    );
    assert!(n["result"]["text"].as_str().unwrap().contains("still open"));
    let n = h.request(
        6,
        "agent/before_start",
        serde_json::json!({"session": {"id": "t", "cwd": "/tmp"}}),
    );
    assert!(n["result"]["text"].is_null());
    // prompt/context injects the next unchecked item
    let c = h.request(
        7,
        "prompt/context",
        serde_json::json!({"cwd": "/tmp", "session": {"id": "t", "cwd": "/tmp"}}),
    );
    let text = c["result"]["text"].as_str().unwrap();
    assert!(text.contains("0/2 items done"));
    assert!(text.contains("Next: #1 a"));
    // check one item, then finish → complete succeeds
    h.tool(8, serde_json::json!({"action": "check_item", "n": 1}));
    h.tool(9, serde_json::json!({"action": "check_item", "n": 2}));
    let r = h.tool(10, serde_json::json!({"action": "complete"}));
    assert_ne!(r["result"]["is_error"], true);
    assert!(r["result"]["content"]
        .as_str()
        .unwrap()
        .contains("marked complete"));
    // graceful shutdown
    h.send(&serde_json::json!({"method": "event/notify", "params": {"type": "turn_end"}}));
    h.send(&serde_json::json!({"method": "plugin/shutdown"}));
    let start = std::time::Instant::now();
    while start.elapsed() < Duration::from_secs(5) {
        if h.child.try_wait().expect("wait").is_some() {
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!("sidecar exits on plugin/shutdown");
}

#[test]
fn done_and_drop_by_user() {
    let mut h = Harness::spawn();
    h.command(1, &["new", "ship", "it"]);
    // /goal done 1 on an empty checklist errors
    let r = h.command(2, &["done", "9"]);
    assert!(r["result"]["text"]
        .as_str()
        .unwrap()
        .contains("out of range"));
    // /goal done marks the whole goal even with open items (user authority)
    h.tool(
        3,
        serde_json::json!({"action": "set_items", "items": ["a", "b"]}),
    );
    let r = h.command(4, &["done"]);
    assert!(r["result"]["text"].as_str().unwrap().contains("still open"));
    // dropped goals leave the active pool
    let r = h.command(5, &["drop"]);
    assert!(r["result"]["text"].as_str().unwrap().contains("dropped"));
    let c = h.request(
        6,
        "prompt/context",
        serde_json::json!({"cwd": "/tmp", "session": {"id": "t", "cwd": "/tmp"}}),
    );
    assert!(!c["result"]["text"]
        .as_str()
        .unwrap()
        .contains("Active goal"));
}
