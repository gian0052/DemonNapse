// d3d11.dll e dxgi.dll servono solo alla cattura dello schermo dell'Ambient: caricate in ritardo,
// Windows le mappa alla prima chiamata, cioè mai se nessun effetto attivo ha un layer Ambient.
fn main() {
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        for dll in ["d3d11.dll", "dxgi.dll"] {
            println!("cargo:rustc-link-arg-bins=/DELAYLOAD:{dll}");
        }
        println!("cargo:rustc-link-arg-bins=delayimp.lib");
    }
}
