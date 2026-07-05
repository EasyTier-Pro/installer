fn main() {
    if std::env::var("PROFILE").as_deref() == Ok("release") {
        thunk::thunk();
    } else {
        println!("cargo:warning=thunk-rs skipped for non-release build");
    }
}
