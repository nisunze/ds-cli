//! Export reviewed native naming examples for the cross-language receipt consumer tests.
fn main() {
    let input: serde_json::Value = serde_json::from_slice(&std::fs::read(std::env::args().nth(1).expect("input inventory")).unwrap()).unwrap();
    let mut output = serde_json::Map::new();
    for stem in input["stems"].as_array().unwrap() {
        let stem = stem.as_str().unwrap();
        let mut names = serde_json::Map::new();
        for format in input["formats"].as_array().unwrap() {
            let format = format.as_str().unwrap();
            if let Ok(name) = ds_command_kernel::report_formats::report_filename(stem, format) {
                names.insert(format.into(), serde_json::json!(name));
            }
        }
        output.insert(stem.into(), serde_json::Value::Object(names));
    }
    println!("{}", serde_json::to_string_pretty(&output).unwrap());
}
