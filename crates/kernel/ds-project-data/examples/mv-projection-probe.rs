//! Read-only native consumer probe for the three public MV context entry points.
use ds_project_data::mv_projection;
use serde_json::json;
use std::path::PathBuf;
fn main() {
    let mut args = std::env::args_os().skip(1);
    let mode = args.next().unwrap();
    let answer = if mode == "unused" {
        json!({"unused":true})
    } else {
        let path = PathBuf::from(args.next().unwrap());
        match mode.to_str().unwrap() {
            "local" => {
                let local = mv_projection::load_local(&path).unwrap();
                json!({"model_id":local.model_id,"revision_id":local.revision_id,"sha256":local.sha256,"projection":local.projection,"route_extents":local.route_extents})
            }
            "project" => {
                let bytes = std::fs::read(path).unwrap();
                mv_projection::project(&bytes, "fixture-mv-projection").unwrap()
            }
            "routes" => {
                let bytes = std::fs::read(path).unwrap();
                json!(mv_projection::route_extents(
                    &bytes,
                    "fixture-mv-projection",
                    "fixture-model"
                )
                .unwrap())
            }
            "batch" => {
                let bytes = std::fs::read(path).unwrap();
                let session = mv_projection::ProjectionSession::capture().unwrap();
                json!([
                    session.project(&bytes, "fixture-mv-projection").unwrap(),
                    session
                        .project(&bytes, "fixture-second-projection")
                        .unwrap()
                ])
            }
            _ => panic!("unknown probe"),
        }
    };
    assert!(args.next().is_none());
    println!("{}", serde_json::to_string(&answer).unwrap());
}
