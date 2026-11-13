fn main() {
    // Android's static C++ runtime comes in two libraries, and the build of the key lock
    // names the first only
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("android") {
        println!("cargo:rustc-link-lib=c++abi");
    }
}
