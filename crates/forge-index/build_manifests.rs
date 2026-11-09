use std::path::{Path, PathBuf};

pub fn generate_framework_manifests(directory_path: &str) -> Vec<PathBuf> {
    let directory = Path::new(directory_path);
    println!("cargo:rerun-if-changed={}", directory.display());
    let mut manifests = Vec::new();
    for entry in std::fs::read_dir(directory).expect("read framework manifest directory") {
        let entry = entry.expect("read framework manifest entry");
        let path = entry.path();
        let Some(extension @ ("toml" | "yaml" | "yml")) =
            path.extension().and_then(|value| value.to_str())
        else {
            continue;
        };
        let file_type = entry.file_type().expect("read framework manifest type");
        assert!(
            !file_type.is_symlink(),
            "framework manifest must not be a symlink"
        );
        if !file_type.is_file() {
            continue;
        }
        let name = path
            .file_stem()
            .and_then(|value| value.to_str())
            .filter(|value| !value.is_empty())
            .expect("UTF-8 framework manifest identity");
        manifests.push((name.to_owned(), extension.to_owned(), path.clone()));
    }
    manifests.sort_by(|left, right| left.0.cmp(&right.0));
    for pair in manifests.windows(2) {
        assert_ne!(
            pair[0].0, pair[1].0,
            "duplicate framework manifest identity"
        );
    }
    let mut source = String::from("&[\n");
    for (name, extension, path) in &manifests {
        let absolute = path
            .canonicalize()
            .expect("canonical framework manifest path");
        source.push_str(&format!(
            "({name:?}, {extension:?}, include_str!({:?})),\n",
            absolute.to_str().expect("UTF-8 framework manifest path")
        ));
    }
    source.push_str("]\n");
    let output = PathBuf::from(std::env::var_os("OUT_DIR").expect("Cargo output directory"));
    std::fs::write(output.join("framework_manifests.rs"), source)
        .expect("write framework inventory");
    manifests.into_iter().map(|(_, _, path)| path).collect()
}
