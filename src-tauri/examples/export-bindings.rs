use std::{fs, path::PathBuf};

fn main() {
    let destination = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../src/lib/ipc-types.ts");
    fs::create_dir_all(destination.parent().expect("binding path has a parent"))
        .unwrap_or_else(|error| panic!("failed to prepare {}: {error}", destination.display()));
    fs::write(&destination, lyn_lib::contract::typescript_bindings())
        .unwrap_or_else(|error| panic!("failed to write {}: {error}", destination.display()));
    println!("generated {}", destination.display());
}
