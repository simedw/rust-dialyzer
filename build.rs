use std::{env, path::PathBuf};

fn main() {
    let manifest = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let (domain, source) = match env::var("CARGO_PKG_NAME").unwrap().as_str() {
        "rust-dialyzer-poc" => ("28", manifest),
        "rust-dialyzer-otp29" => ("29", manifest.parent().unwrap().to_owned()),
        package => panic!("no OTP semantic domain assigned to {package}"),
    };
    println!("cargo::rustc-check-cfg=cfg(otp_domain, values(\"28\", \"29\"))");
    println!("cargo::rustc-check-cfg=cfg(dialyzer_tsan)");
    println!("cargo::rustc-cfg=otp_domain=\"{domain}\"");
    println!("cargo::rustc-env=RUST_DIALYZER_SOURCE_ROOT={}", source.display());
    println!("cargo::rerun-if-changed={}", source.join("build.rs").display());
}
