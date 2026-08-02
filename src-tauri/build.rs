use std::fs;
use std::path::Path;

fn main() {
    let frontend_dist = Path::new(env!("CARGO_MANIFEST_DIR")).join("../dist");
    if !frontend_dist.exists() {
        println!("cargo:warning=frontend dist is missing; run pnpm build before packaging");
        fs::create_dir_all(&frontend_dist).expect("failed to create frontend dist directory");
    }
    tauri_build::build();
}
