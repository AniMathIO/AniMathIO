//! Discovery must not trust bundles planted in a project's working directory.

use std::process::Command;

#[test]
fn working_directory_cannot_supply_plugins() {
    const PROBE_ROOT: &str = "ANIMATHIO_DISCOVERY_TEST_ROOT";
    if let Some(root) = std::env::var_os(PROBE_ROOT) {
        let root = std::path::PathBuf::from(root);
        for dir in animathio_plugin_host::plugin_search_dirs() {
            assert!(
                !dir.path.starts_with(&root),
                "working directory supplied a plugin location: {}",
                dir.path.display()
            );
        }
        return;
    }

    let project = tempfile::tempdir().unwrap();
    let root = project.path().canonicalize().unwrap();
    let cwd = root.join("nested/work");
    std::fs::create_dir_all(&cwd).unwrap();
    std::fs::create_dir_all(root.join("target/plugins")).unwrap();
    std::fs::create_dir_all(cwd.join("plugins")).unwrap();
    // Change only the child process's CWD/environment, keeping parallel tests
    // independent. A former CWD-to-target/plugins walk would find this project.
    let output = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "working_directory_cannot_supply_plugins",
            "--nocapture",
        ])
        .current_dir(cwd)
        .env_remove("ANIMATHIO_PLUGIN_DIR")
        .env(PROBE_ROOT, root)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "discovery probe failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
