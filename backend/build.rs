//! Build script for the embedded web UI.
//!
//! Two jobs, both of them about `frontend/dist`, which `routes::embedded_ui` compiles into the binary.
//!
//! 1. **Make sure the directory exists.** The embed macro fails to compile against a missing folder, so a
//!    fresh clone would not build until someone had run the frontend build. An empty directory embeds
//!    nothing, which is exactly the "no UI in this binary" case the router already handles.
//! 2. **Rebuild when the UI changes.** A proc macro cannot tell Cargo what it read, so without the
//!    directive below a `cargo build --release` after a UI rebuild would happily reuse the previous
//!    binary and ship the previous UI.

use std::path::PathBuf;

fn main() {
    let dist =
        PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("cargo sets the manifest dir"))
            .join("..")
            .join("frontend")
            .join("dist");

    if let Err(error) = std::fs::create_dir_all(&dist) {
        // Not fatal on its own: the compile error from the embed macro is the one worth reading, and it
        // names the path. This only explains why the directory was not created for you.
        println!("cargo:warning=could not create {}: {error}", dist.display());
    }

    println!("cargo:rerun-if-changed={}", dist.display());
}
