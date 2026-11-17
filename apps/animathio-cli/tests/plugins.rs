//! `animathio-cli plugins` round trip against a private plugin folder and settings file.
//! Uses the frozen `hello` component fixture, so no plugin build is needed.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::Value;
use tempfile::tempdir;

const ID: &str = "com.example.hello";

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../crates/plugin-host/tests/fixtures/hello-1.0.0.amplugin")
}

struct Env {
    root: tempfile::TempDir,
}

impl Env {
    fn new() -> Self {
        Self {
            root: tempdir().unwrap(),
        }
    }
    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_animathio-cli"))
            .arg("plugins")
            .args(args)
            .env(
                "ANIMATHIO_USER_PLUGIN_DIR",
                self.root.path().join("plugins"),
            )
            .env(
                "ANIMATHIO_PLUGIN_SETTINGS",
                self.root.path().join("settings.json"),
            )
            .output()
            .unwrap()
    }
    fn ok(&self, args: &[&str]) -> String {
        let output = self.run(args);
        assert!(
            output.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap()
    }
    fn list(&self) -> Value {
        serde_json::from_str(&self.ok(&["list", "--json"])).unwrap()
    }
}

#[test]
fn install_enable_disable_remove_round_trip() {
    let env = Env::new();
    let file = fixture();
    let file = file.to_str().unwrap();

    let empty = env.list();
    assert_eq!(empty["community_enabled"], false);
    assert_eq!(empty["plugins"].as_array().unwrap().len(), 0);

    let text = env.ok(&["install", file]);
    assert!(
        text.contains(ID) && text.contains("starts disabled"),
        "{text}"
    );
    let listing = env.list();
    assert_eq!(listing["plugins"][0]["id"], ID);
    assert_eq!(listing["plugins"][0]["enabled"], false);

    // A second install is a collision unless it is an update.
    let again = env.run(&["install", file]);
    assert!(!again.status.success());
    assert!(String::from_utf8_lossy(&again.stderr).contains("already installed"));
    env.ok(&["install", file, "--force"]);

    let text = env.ok(&["enable", ID]);
    assert!(text.contains("community plugins are off"), "{text}");
    assert_eq!(env.list()["plugins"][0]["enabled"], true);
    assert_eq!(env.ok(&["community"]).trim(), "community plugins: off");
    assert_eq!(env.ok(&["community", "on"]).trim(), "community plugins: on");
    assert_eq!(env.list()["community_enabled"], true);

    env.ok(&["disable", ID]);
    assert_eq!(env.list()["plugins"][0]["enabled"], false);

    env.ok(&["remove", ID]);
    assert_eq!(env.list()["plugins"].as_array().unwrap().len(), 0);
    assert!(!env.run(&["remove", ID]).status.success());
    assert!(!env.run(&["enable", ID]).status.success());
    assert_eq!(
        env.ok(&["community", "off"]).trim(),
        "community plugins: off"
    );
}

#[test]
fn install_refuses_bad_hashes_urls_and_packages() {
    let env = Env::new();
    let file = fixture();
    let file = file.to_str().unwrap();
    let wrong = "0".repeat(64);

    let bad = env.run(&["install", file, "--sha256", &wrong]);
    assert!(!bad.status.success());
    assert!(String::from_utf8_lossy(&bad.stderr).contains("sha-256 mismatch"));

    let http = env.run(&[
        "install",
        "http://example.com/p.amplugin",
        "--sha256",
        &wrong,
    ]);
    assert!(String::from_utf8_lossy(&http.stderr).contains("https"));
    let nohash = env.run(&["install", "https://example.com/p.amplugin"]);
    assert!(String::from_utf8_lossy(&nohash.stderr).contains("sha-256"));

    let junk = env.root.path().join("junk.amplugin");
    std::fs::write(&junk, b"not a package").unwrap();
    assert!(
        !env.run(&["install", junk.to_str().unwrap()])
            .status
            .success()
    );

    assert_eq!(env.list()["plugins"].as_array().unwrap().len(), 0);
}

fn render(env: &Env, project: &Path, out: &Path) -> Vec<u8> {
    let output = Command::new(env!("CARGO_BIN_EXE_animathio-cli"))
        .args(["render", project.to_str().unwrap(), "--frame", "0"])
        .args([
            "--width",
            "64",
            "--height",
            "36",
            "-o",
            out.to_str().unwrap(),
        ])
        .env("ANIMATHIO_USER_PLUGIN_DIR", env.root.path().join("plugins"))
        .env(
            "ANIMATHIO_PLUGIN_SETTINGS",
            env.root.path().join("settings.json"),
        )
        .env_remove("ANIMATHIO_PLUGIN_DIR")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    std::fs::read(out).unwrap()
}

/// An installed community plugin only renders once the global switch and its own flag are on.
#[test]
fn a_community_plugin_renders_only_when_enabled() {
    use animathio_core::{
        Clip, ClipId, ClipKind, Command, Project, Time, TimeRange, Track, TrackId,
    };

    let env = Env::new();
    let mut project = Project::default();
    let mut history = animathio_core::History::default();
    history
        .execute(
            &mut project,
            &Command::AddTrack {
                track: Track::new(TrackId(1), "Plugin"),
                index: 0,
            },
        )
        .unwrap();
    let clip = Clip::new(
        ClipId(1),
        ClipKind::Plugin {
            plugin_id: ID.to_owned(),
            params: serde_json::Value::Null,
        },
        TimeRange::new(Time(0), Time(animathio_core::TICKS_PER_SECOND)).unwrap(),
    );
    history
        .execute(
            &mut project,
            &Command::AddClip {
                track: TrackId(1),
                clip,
            },
        )
        .unwrap();
    let project_file = env.root.path().join("project.json");
    animathio_format::write_json(std::fs::File::create(&project_file).unwrap(), &project).unwrap();
    let out = env.root.path().join("frame.png");

    env.ok(&["install", fixture().to_str().unwrap()]);
    let placeholder = render(&env, &project_file, &out);

    // Installed but disabled, and enabled with the switch off: still the placeholder.
    assert_eq!(render(&env, &project_file, &out), placeholder);
    env.ok(&["enable", ID]);
    assert_eq!(render(&env, &project_file, &out), placeholder);

    // Switch on: the plugin loads and its clip renders differently.
    env.ok(&["community", "on"]);
    let drawn = render(&env, &project_file, &out);
    assert_ne!(drawn, placeholder);

    // Disabled again: back to the placeholder.
    env.ok(&["disable", ID]);
    assert_eq!(render(&env, &project_file, &out), placeholder);
}
