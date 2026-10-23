// include_dir! embeds templates/ when src/scaffold.rs compiles, but cargo doesn't know the binary
// depends on those files. This makes a template edit rebuild it.
fn main() {
    println!("cargo:rerun-if-changed=templates");
}
