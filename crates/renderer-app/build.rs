use std::{env, path::PathBuf, process::Command};

fn ensure_frontend_dist() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("manifest dir"));
    let ui_dir = manifest_dir.join("../../renderer-ui");
    let dist_index = ui_dir.join("dist/index.html");

    if dist_index.exists() {
        return;
    }

    // Install dependencies if they have not been downloaded yet.
    if !ui_dir.join("node_modules").exists() {
        let status = Command::new("npm")
            .arg("install")
            .current_dir(&ui_dir)
            .status()
            .expect("failed to run npm install");

        if !status.success() {
            panic!("npm install failed with status: {status}");
        }
    }

    let status = Command::new("npm")
        .arg("run")
        .arg("build")
        .current_dir(&ui_dir)
        .status()
        .expect("failed to run npm run build");

    if !status.success() {
        panic!("npm run build failed with status: {status}");
    }
}

fn main() {
    ensure_frontend_dist();
    tauri_build::build()
}
