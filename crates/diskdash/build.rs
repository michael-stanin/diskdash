// Release builds embed frontend/dist. Rebuild when the UI build changes,
// including newly added hashed asset files.
fn main() {
    println!("cargo:rerun-if-changed=../../frontend/dist");
}
