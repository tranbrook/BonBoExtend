//! Tests for kill switch — emergency stop mechanism.

use bonbo_agent::kill_switch::KillSwitch;
use std::path::PathBuf;
use tempfile::TempDir;

fn make_kill_switch(dir: &TempDir) -> KillSwitch {
    KillSwitch::new(dir.path())
}

#[tokio::test]
async fn test_kill_switch_starts_deactivated() {
    let dir = TempDir::new().unwrap();
    let ks = make_kill_switch(&dir);
    assert!(!ks.is_activated().await);
}

#[tokio::test]
async fn test_kill_switch_activate() {
    let dir = TempDir::new().unwrap();
    let ks = make_kill_switch(&dir);
    ks.activate().await.unwrap();
    assert!(ks.is_activated().await);
    // Check file was created
    let kill_file = dir.path().join("kill_switch.flag");
    assert!(kill_file.exists());
}

#[tokio::test]
async fn test_kill_switch_deactivate() {
    let dir = TempDir::new().unwrap();
    let ks = make_kill_switch(&dir);
    ks.activate().await.unwrap();
    assert!(ks.is_activated().await);

    ks.deactivate().await.unwrap();
    assert!(!ks.is_activated().await);
    // File should be removed
    let kill_file = dir.path().join("kill_switch.flag");
    assert!(!kill_file.exists());
}

#[tokio::test]
async fn test_kill_switch_file_activation() {
    let dir = TempDir::new().unwrap();
    let ks = make_kill_switch(&dir);

    // Create the kill file manually
    let kill_file = dir.path().join("kill_switch.flag");
    std::fs::write(&kill_file, "KILLED").unwrap();

    // Kill switch should detect file
    assert!(ks.is_activated().await);
}

#[tokio::test]
async fn test_kill_switch_path() {
    let dir = TempDir::new().unwrap();
    let ks = make_kill_switch(&dir);
    let expected = dir.path().join("kill_switch.flag");
    assert_eq!(ks.kill_file_path(), expected);
}
