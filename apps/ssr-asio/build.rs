// SPDX-License-Identifier: MIT
#![forbid(unsafe_code)]
fn main() {
    // The release ships the x64 DLL only (ADR 0025); the ARM64 build still
    // compiles, because the workspace is linted and tested on that target.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows")
        && std::env::var("CARGO_CFG_TARGET_ARCH").as_deref() == Ok("x86_64")
    {
        for name in ["DllGetClassObject", "DllCanUnloadNow"] {
            println!("cargo:rustc-cdylib-link-arg=/EXPORT:{name}");
            println!("cargo:rustc-cdylib-link-arg=/INCLUDE:{name}");
        }
    }
}
