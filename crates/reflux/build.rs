fn main() {
    // The icon resource, for MSVC builds (the release's); `scripts/cross-check.sh` checks with
    // the GNU target and no resource compiler.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") && std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        println!("cargo:rerun-if-changed=../../packaging/windows/reflux.rc");
        println!("cargo:rerun-if-changed=../../packaging/windows/reflux.ico");
        embed_resource::compile("../../packaging/windows/reflux.rc", embed_resource::NONE).manifest_required().unwrap();
    }
}
