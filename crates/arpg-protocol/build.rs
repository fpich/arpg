fn main() {
    prost_build::compile_protos(&["../../proto/arpg.proto"], &["../../proto/"])
        .expect("proto compile failed");
    println!("cargo:rerun-if-changed=../../proto/arpg.proto");
}
