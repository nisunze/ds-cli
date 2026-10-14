//! Captures the original linked consumer closure before process wiring changes.
use ds_network::network::lv_processing::{
    decode_native_lv_request, encode_native_lv_result, process_native_lv,
};
use std::{fs, io::Write};
fn main() {
    let args: Vec<_> = std::env::args_os().collect();
    assert_eq!(args.len(), 3);
    let input = fs::read(&args[1]).unwrap();
    let request = decode_native_lv_request(&input).unwrap();
    let output = encode_native_lv_result(&process_native_lv(request).unwrap()).unwrap();
    let mut file = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&args[2])
        .unwrap();
    file.write_all(&output).unwrap();
    file.sync_all().unwrap();
}
