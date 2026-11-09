//! The setup's icon (a plain letter G mark, generated) and its manifest (DPI awareness).
#[path = "src/icon_gen.rs"]
#[allow(dead_code)]
mod icon_gen;

fn main() {
    println!("cargo:rerun-if-changed=src/icon_gen.rs");
    println!("cargo:rerun-if-changed=setup.rc");
    println!("cargo:rerun-if-changed=setup.manifest.xml");
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap());
    std::fs::write(out.join("gravecord.ico"), icon_gen::ico()).unwrap();
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        embed_resource::compile(
            "setup.rc",
            embed_resource::ParamsIncludeDirs([out.as_path()]),
        )
        .manifest_required()
        .unwrap();
    }
}
