use serde_json::Value;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

fn installer() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_easytier-pro-installer"))
}

fn temp_install_dir(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock")
        .as_nanos();
    std::env::temp_dir().join(format!(
        "easytier-pro-installer-e2e-{name}-{}-{nanos}",
        std::process::id()
    ))
}

fn run_with_stdin(args: &[&str], input: &str) -> std::process::Output {
    let mut child = Command::new(installer())
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn installer");

    child
        .stdin
        .as_mut()
        .expect("stdin")
        .write_all(input.as_bytes())
        .expect("write stdin");

    child.wait_with_output().expect("wait installer")
}

fn first_json_line(stdout: &[u8]) -> Value {
    let stdout = String::from_utf8_lossy(stdout);
    let line = stdout.lines().next().expect("json line");
    serde_json::from_str(line).expect("valid json line")
}

#[test]
fn help_lists_public_commands_and_hides_desktop_protocol() {
    let output = Command::new(installer())
        .arg("--help")
        .output()
        .expect("run help");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("install"));
    assert!(stdout.contains("update"));
    assert!(stdout.contains("uninstall"));
    assert!(stdout.contains("status"));
    assert!(!stdout.contains("desktop"));
}

#[test]
fn desktop_status_reports_empty_install_dir_as_not_ready() {
    let install_dir = temp_install_dir("status");
    let request = serde_json::json!({ "install_dir": install_dir.to_string_lossy() }).to_string();

    let output = run_with_stdin(&["desktop", "status", "--json"], &request);

    let _ = std::fs::remove_dir_all(&install_dir);
    assert!(output.status.success());
    let event = first_json_line(&output.stdout);
    assert_eq!(event["event"], "finished");
    assert_eq!(event["data"]["binaries_present"], false);
    assert_eq!(event["data"]["ready"], false);
}

#[test]
fn desktop_install_rejects_missing_required_fields() {
    let output = run_with_stdin(&["desktop", "install", "--json"], "{}");

    assert!(!output.status.success());
    let event = first_json_line(&output.stdout);
    assert_eq!(event["event"], "error");
    assert_eq!(event["data"]["code"], "invalid_request");
}

#[test]
fn desktop_install_requires_json_flag() {
    let output = run_with_stdin(&["desktop", "install"], "{}");

    assert!(!output.status.success());
    let event = first_json_line(&output.stdout);
    assert_eq!(event["event"], "error");
    assert_eq!(event["data"]["code"], "invalid_request");
}
