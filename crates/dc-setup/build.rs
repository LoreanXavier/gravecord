fn main() {
    println!("cargo:rerun-if-changed=../../app/assets/branding/gravecord/app/gravecord.ico");
    println!("cargo:rerun-if-changed=setup.rc");
    println!("cargo:rerun-if-changed=setup.manifest.xml");
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap());
    std::fs::copy(
        "../../app/assets/branding/gravecord/app/gravecord.ico",
        out.join("gravecord.ico"),
    )
    .unwrap();
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        embed_resource::compile(
            "setup.rc",
            embed_resource::ParamsIncludeDirs([out.as_path()]),
        )
        .manifest_required()
        .unwrap();
    }
}
