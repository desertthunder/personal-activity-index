use std::{env, fs, path::PathBuf};

fn main() {
    println!("cargo:rerun-if-changed=src/app.rs");
    println!("cargo:rerun-if-changed=src/main.rs");

    let out_dir = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR not set by Cargo build script environment"));
    let man_path = out_dir.join("pai.1");

    let man = build_manpage();
    fs::write(&man_path, man).expect("failed to write manpage");
    println!("cargo:rustc-env=PAI_MAN_PAGE={}", man_path.display());
}

#[path = "src/app.rs"]
mod app;

fn build_manpage() -> Vec<u8> {
    use clap::CommandFactory;

    let cmd = app::Cli::command();
    let man = clap_mangen::Man::new(cmd);
    let mut buffer = Vec::new();
    man.render(&mut buffer).expect("failed to render man page");
    buffer
}
