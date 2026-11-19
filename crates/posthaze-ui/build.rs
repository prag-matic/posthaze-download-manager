fn main() {
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        // Windows MSVC defaults to a 1 MiB main-thread stack, which debug GPUI
        // element trees can exceed; raise it to 8 MiB to match Unix defaults.
        println!("cargo:rustc-link-arg=/STACK:8388608");
    }
}
