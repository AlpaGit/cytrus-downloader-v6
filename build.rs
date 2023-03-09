use flatc_rust;

use std::path::Path;
use flatc_rust::Flatc;

fn main() {
    // println!("cargo:rerun-if-changed=flatbuffers/manifest.fbs");
    let flatc = Flatc::from_path("./flatc");

    // First check with have good `flatc`
    flatc.check().unwrap();
    flatc.run(flatc_rust::Args {
        inputs: &[Path::new("flatbuffers/manifest.fbs")],
        out_dir: Path::new("src/flatbuffers/"),
        ..Default::default()
    }).expect("flatc");
}