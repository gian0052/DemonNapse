// d3d11.dll and dxgi.dll are only needed for the Ambient's screen capture: delay-loaded,
// Windows maps them at the first call, i.e. never if no active effect has an Ambient layer.
fn main() {
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        for dll in ["d3d11.dll", "dxgi.dll"] {
            println!("cargo:rustc-link-arg-bins=/DELAYLOAD:{dll}");
        }
        println!("cargo:rustc-link-arg-bins=delayimp.lib");
    }
}
