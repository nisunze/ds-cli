//! Owner acceptance (DECOUPLING-012, fixed list): "Profile means moving a
//! pole, rather than panning: equivalent UI/CLI inputs must produce the same
//! authored position, revision, history and recomputation; SQLite readback
//! after restart must retain the edit."
//!
//! Everything here is the shipped path: the real `ds core serve` process with
//! the production authorizer over a fixture device credential, the network
//! cut, the real `ds core profile-session` CLI, and the selected qualified
//! Network executable (`DS_NETWORK_ENGINE_BIN`) as Core's managed engine.
//!
//! Each request document is exactly the one the Desktop adapter
//! (`ds-web/src/lib/desktop/installed-profile*.ts`) sends through the generic
//! Core door, which stages it as `--request` and reads `--out` back. When
//! `DS_PROFILE_PROOF_RECORD` names a new file, every (request, receipt) pair
//! is written there so the Desktop adapter can be replayed against these
//! exact Core receipts and must emit byte-equal documents.

mod fixtures;

use base64::{Engine, engine::general_purpose::STANDARD};
use fixtures::*;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

const PROJECT: &str = "profile-acceptance";
const SCHEMA: &str = "ds.core.profile-session/v1";

fn package() -> Vec<u8> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../../ds-network/fixtures/pls-public/humble-pole/humble-pole.dsgrid");
    std::fs::read(&path).unwrap_or_else(|e| panic!("fixture package {}: {e}", path.display()))
}

/// The Desktop's scene options for the default application settings
/// (`profileSceneOptions`), and the shared Profile display style fixture.
fn options() -> Value {
    json!({"vertical_exaggeration":10,"terrain_corridor_half_width_m":15,
        "terrain_gap_tolerance_m":100,"clearance_ground_offset_m":6})
}
fn style() -> Value {
    serde_json::from_str(include_str!(
        "../../../kernel/ds-command-kernel/tests/fixtures/profile-display-style.json"
    ))
    .expect("profile display style fixture")
}

struct Driver<'a> {
    server: &'a LiveServer,
    room: PathBuf,
    steps: Vec<Value>,
    counter: usize,
}
impl Driver<'_> {
    /// One `ds core profile-session --request --out` run; the full receipt.
    fn run(&mut self, name: &str, operation: Value) -> Value {
        let request = json!({"schema": SCHEMA, "operation": operation});
        self.counter += 1;
        let input = self.room.join(format!("request-{}.json", self.counter));
        let out = self.room.join(format!("receipt-{}.json", self.counter));
        std::fs::write(&input, serde_json::to_vec(&request).unwrap()).unwrap();
        let run = self.server.ds(&[
            "core",
            "profile-session",
            "--project",
            PROJECT,
            "--request",
            &input.display().to_string(),
            "--out",
            &out.display().to_string(),
            "--output",
            "json",
        ]);
        assert_eq!(
            run.envelope["status"], "ok",
            "{name}: {} {} | server: {}",
            run.stdout, run.stderr, self.server.said()
        );
        assert_eq!(run.envelope["data"]["schema"], "ds.core.profile-artifact/v1");
        let bytes = std::fs::read(&out).expect("the private result file");
        assert_eq!(
            run.envelope["data"]["sha256"],
            format!("{:x}", Sha256::digest(&bytes)),
            "{name}: receipt digest"
        );
        let receipt: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(receipt["schema"], "ds.core.profile-receipt/v1");
        assert_eq!(receipt["project"], PROJECT);
        self.steps
            .push(json!({"name": name, "request": request, "receipt": receipt}));
        receipt
    }
}

fn session(receipt: &Value) -> (Value, Value) {
    (receipt["session_id"].clone(), receipt["model_id"].clone())
}
fn owned(receipt: &Value, action: &str, extra: Value) -> Value {
    let (session_id, model_id) = session(receipt);
    let mut operation = extra;
    operation["action"] = json!(action);
    operation["session_id"] = session_id;
    operation["model_id"] = model_id;
    operation["expected_history"] = receipt["history"].clone();
    operation
}
fn view(receipt: &Value) -> Value {
    owned(
        receipt,
        "view",
        json!({"options": options(), "patch": {}, "style_document": style(), "cached": false, "observe": false}),
    )
}
/// The structure's authored station and position in a native scene.
fn placed(scene: &Value, structure: &str) -> Option<Value> {
    scene["bands"].as_array()?.iter().find_map(|band| {
        band["structures"].as_array()?.iter().find_map(|row| {
            (row["entity_id"] == structure).then(|| {
                json!({"station_m": row["ground"]["station_m"], "chain_station_m": row["ground"]["chain_station_m"],
                    "ground_elevation_m": row["ground"]["elevation_m"], "map_point": row["map_point"],
                    "alignment_id": band["alignment_id"]})
            })
        })
    })
}
fn scene(receipt: &Value) -> Value {
    serde_json::from_str(receipt["payload"]["scene_json"].as_str().expect("native scene json"))
        .expect("native scene")
}

#[test]
#[ignore = "drives the shipped Core with a qualified Network: set DS_NETWORK_ENGINE_BIN and run with --ignored"]
fn a_pole_move_through_the_profile_session_is_the_same_authored_edit_and_survives_restart() {
    let engine = std::env::var_os("DS_NETWORK_ENGINE_BIN")
        .expect("DS_NETWORK_ENGINE_BIN names the qualified Network executable");
    assert!(Path::new(&engine).is_absolute());
    let bytes = package();
    let sha = format!("{:x}", Sha256::digest(&bytes));
    let machine = device_home::DeviceHome::linked(LANE, "profile-acceptance-device");
    let mut server = LiveServer::start(&machine, 1);
    let room = tempfile::tempdir().expect("private request room");
    let mut steps = Vec::new();

    // Session A: open, enter editing, draw, preview and apply one station move.
    let (opened, edited, moved, recomputed, structure, target, axis);
    {
        let mut driver = Driver { server: &server, room: room.path().to_owned(), steps: Vec::new(), counter: 0 };
        opened = driver.run(
            "open",
            json!({"action":"open","package_base64":STANDARD.encode(&bytes),"package_sha256":sha,"recover":true}),
        );
        assert_eq!(opened["package_sha256"], sha);
        assert_eq!(opened["closed"], false);
        let initial = opened["history"].clone();
        assert_eq!(initial["undo_depth"], 0);
        edited = driver.run(
            "edit_mode",
            owned(&opened, "edit_mode", json!({"enabled": true})),
        );
        assert_eq!(edited["payload"]["enabled"], true);
        assert_eq!(edited["history"], initial, "entering editing authors nothing");
        let drawn = driver.run("view", view(&edited));
        let before = scene(&drawn);
        axis = before["axis_pin"].clone();
        assert!(axis.is_object(), "the drawn scene carries its axis pin");
        structure = "str-e9bc0619ae359761-1".to_owned();
        let start = placed(&before, &structure).expect("the pole is drawn");
        assert_eq!(start["station_m"], 100.0);
        target = 120.0;
        let request = json!({"expected_revision": initial["model_revision"], "structure_id": structure, "station_m": target});
        let preview = driver.run(
            "preview_structure_station_move",
            owned(&drawn, "control", json!({"operation": {"action":"preview_structure_station_move",
                "request": request, "options": options(), "held_axis_pin": axis}})),
        );
        assert_eq!(preview["history"], initial, "a preview authors nothing");
        let proposed = &preview["payload"]["result"]["preview"];
        assert_eq!(proposed["structure_id"], structure.as_str());
        assert_eq!(proposed["target_station_m"], target);
        assert_eq!(proposed["refusal_on_write"], Value::Null);
        moved = driver.run(
            "apply_structure_station_move",
            owned(&preview, "control", json!({"operation": {"action":"apply_structure_station_move", "request": request}})),
        );
        assert_eq!(moved["payload"]["applied"], true);
        assert_eq!(moved["history"]["undo_depth"], 1, "one authored edit");
        assert_eq!(moved["history"]["initial_revision"], initial["initial_revision"]);
        assert_ne!(moved["history"]["model_revision"], initial["model_revision"]);
        assert_eq!(
            moved["payload"]["result"]["outcome"]["final_revision"]["revision_id"],
            moved["history"]["model_revision"]
        );
        recomputed = driver.run("recompute_view", view(&moved));
        assert_eq!(recomputed["history"], moved["history"]);
        let after = scene(&recomputed);
        assert_eq!(after["model_revision"], moved["history"]["model_revision"]);
        let position = placed(&after, &structure).expect("the moved pole is drawn");
        assert_eq!(position["station_m"], target, "authored position is the requested station");
        assert_eq!(position["alignment_id"], start["alignment_id"]);
        let state = driver.run("state", owned(&recomputed, "state", json!({})));
        assert_eq!(state["history"], moved["history"]);
        steps.append(&mut driver.steps);
    }

    // Restart Core over the same protected SQLite store: the acknowledged edit
    // is retained and reopens at its exact head without re-authoring it.
    server.restart(1);
    let mut driver = Driver { server: &server, room: room.path().to_owned(), steps: Vec::new(), counter: 100 };
    let reopened = driver.run(
        "reopen_after_restart",
        json!({"action":"open","package_base64":STANDARD.encode(&bytes),"package_sha256":sha,"recover":true}),
    );
    assert_eq!(reopened["history"], moved["history"], "SQLite readback retains the edit's exact history");
    assert_eq!(reopened["package_sha256"], sha);
    let redrawn = driver.run("recompute_after_restart", view(&reopened));
    let after_restart = scene(&redrawn);
    assert_eq!(after_restart["model_revision"], moved["history"]["model_revision"]);
    assert_eq!(
        placed(&after_restart, &structure),
        placed(&scene(&recomputed), &structure),
        "the authored position is retained after restart"
    );
    let identical_scene = redrawn["payload"]["scene_json"] == recomputed["payload"]["scene_json"];
    steps.append(&mut driver.steps);

    // A stale request (the pre-move history) is refused, never repaired.
    let stale = owned(&opened, "state", json!({}));
    let input = room.path().join("stale.json");
    std::fs::write(&input, serde_json::to_vec(&json!({"schema":SCHEMA,"operation":stale})).unwrap()).unwrap();
    let refused = server.ds(&["core","profile-session","--project",PROJECT,"--request",&input.display().to_string(),"--output","json"]);
    assert_eq!(refused.envelope["status"], "error", "{}", refused.stdout);
    assert_eq!(refused.envelope["error"]["code"], "core_profile_history_conflict");

    if let Some(path) = std::env::var_os("DS_PROFILE_PROOF_RECORD") {
        let record = json!({
            "schema": "ds.proof.core-profile-station-move/v1",
            "package_sha256": sha,
            "structure_id": structure,
            "target_station_m": target,
            "recompute_scene_identical_after_restart": identical_scene,
            "steps": steps,
        });
        let path = PathBuf::from(path);
        assert!(path.is_absolute() && !path.exists(), "the proof record is a new absolute file");
        std::fs::write(&path, serde_json::to_vec_pretty(&record).unwrap()).unwrap();
    }
}
