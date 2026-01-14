#[cfg(windows)]
fn main() {
    use std::path::PathBuf;
    use std::process::Command;

    // Generate the .rc file using winres
    let mut res = winres::WindowsResource::new();
    res.set_icon("resources/fm.ico");
    res.compile().unwrap();

    // Cargo places the generated .rc file in OUT_DIR
    let out_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let rc_path = out_dir.join("resource.rc");
    let res_path = out_dir.join("resource.res");

    // Call windres manually
    let status = Command::new("windres")
        .arg(rc_path.to_str().unwrap())
        .arg("-O")
        .arg("coff")
        .arg("-o")
        .arg(res_path.to_str().unwrap())
        .status()
        .expect("Failed to run windres");

    if !status.success() {
        panic!("windres failed with exit code {:?}", status.code());
    }

    // Tell Cargo to link the .res file
    println!("cargo:rustc-link-arg-bins={}", res_path.to_str().unwrap());
}

#[cfg(not(windows))]
fn main() {}
