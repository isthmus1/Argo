use std::env;
use std::path::PathBuf;
include!(concat!(env!("OUT_DIR"), "/bindings.rs"));

pub fn compiled_c_return_val(code: &str) -> String {
    create_file(code);
    header_from_c();
    //compile
    delete_file();
    return String::from("Alice");
}

fn create_file(code: &str) {
    std::fs::write("temp.c", code).expect("failed to write temp.c");
}

fn delete_file() {
    match std::fs::remove_file("temp.c") {
        Ok(()) => {}
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => panic!("failed to delete temp.c: {err}"),
    }
}

fn header_from_c() -> String {
    let bindings = bindgen::Builder::default()
        // The input header we would like to generate
        // bindings for.
        .header("temp.c")
        // Tell cargo to invalidate the built crate whenever any of the
        // included header files changed.
        .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()))
        // Finish the builder and generate the bindings.
        .generate()
        // Unwrap the Result and panic on failure.
        .expect("Unable to generate bindings");

    // Write the bindings to the $OUT_DIR/bindings.rs file.
    let out_path = PathBuf::from(env::var("OUT_DIR").unwrap());
    bindings
        .write_to_file(out_path.join("bindings.rs"))
        .expect("Couldn't write bindings!");

    return String::from("Alice");
}
