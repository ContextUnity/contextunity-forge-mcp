fn main() {
    generate_profiles();
    if std::env::var_os("CARGO_FEATURE_LANG_PROTO").is_none() {
        return;
    }
    let proto_src = std::path::Path::new("vendor/tree-sitter-proto/src");
    let mut c_config = cc::Build::new();
    c_config.include(proto_src);
    c_config
        .flag_if_supported("-Wno-unused-parameter")
        .flag_if_supported("-Wno-unused-but-set-variable")
        .flag_if_supported("-Wno-trigraphs");
    c_config.file(proto_src.join("parser.c"));
    c_config.compile("tree-sitter-proto");
    println!("cargo:rerun-if-changed=vendor/tree-sitter-proto/src/parser.c");
}

fn generate_profiles() {
    use sha2::Digest;
    let directory = std::path::Path::new("src/engine/languages");
    println!("cargo:rerun-if-changed={}", directory.display());
    let mut profiles = Vec::new();
    for entry in std::fs::read_dir(directory).expect("read language profile directory") {
        let entry = entry.expect("read language profile entry");
        let path = entry.path();
        if !entry.file_type().expect("read profile file type").is_file()
            || path.extension().and_then(|p| p.to_str()) != Some("rs")
        {
            continue;
        }
        let name = path
            .file_stem()
            .and_then(|p| p.to_str())
            .expect("UTF-8 profile filename");
        if name == "mod" {
            continue;
        }
        assert!(
            name.bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
                && name.as_bytes()[0].is_ascii_lowercase(),
            "invalid language profile filename: {name}"
        );
        profiles.push((
            name.to_owned(),
            path.canonicalize()
                .expect("canonical language profile path"),
        ));
    }
    profiles.sort_by(|a, b| a.0.cmp(&b.0));
    let enabled: Vec<_> = profiles
        .iter()
        .filter(|(name, _)| {
            let variable = format!("CARGO_FEATURE_LANG_{}", name.to_ascii_uppercase());
            println!("cargo:rerun-if-env-changed={variable}");
            std::env::var_os(variable).is_some()
        })
        .collect();
    let mut fingerprint_files = vec![
        std::path::PathBuf::from("Cargo.lock"),
        std::path::PathBuf::from("Cargo.toml"),
        std::path::PathBuf::from("build.rs"),
        std::path::PathBuf::from("src/core/models.rs"),
        std::path::PathBuf::from("src/core/models/compact_graph.rs"),
        std::path::PathBuf::from("src/core/semantic.rs"),
        std::path::PathBuf::from("src/core/typed_facts.rs"),
        std::path::PathBuf::from("src/core/schema.rs"),
        std::path::PathBuf::from("src/core/commitments.rs"),
        std::path::PathBuf::from("src/db/writer.rs"),
    ];
    let mut pending = vec![std::path::PathBuf::from("src/engine")];
    while let Some(path) = pending.pop() {
        for entry in std::fs::read_dir(path).expect("read profile fingerprint directory") {
            let entry = entry.expect("read profile fingerprint entry");
            let path = entry.path();
            if entry
                .file_type()
                .expect("read profile fingerprint type")
                .is_dir()
            {
                pending.push(path);
            } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
                fingerprint_files.push(path);
            }
        }
    }
    fingerprint_files.sort();
    let mut digest = sha2::Sha256::new();
    for (name, _) in &enabled {
        digest.update(format!("lang-{name}"));
        digest.update([0]);
    }
    for path in fingerprint_files {
        println!("cargo:rerun-if-changed={}", path.display());
        let contents = std::fs::read(&path).expect("read profile fingerprint input");
        digest.update(
            path.to_str()
                .expect("UTF-8 profile fingerprint path")
                .as_bytes(),
        );
        digest.update([0]);
        digest.update((contents.len() as u64).to_le_bytes());
        digest.update(contents);
    }
    println!(
        "cargo:rustc-env=FORGE_LANGUAGE_PROFILE_DIGEST={:x}",
        digest.finalize()
    );
    let mut source = String::new();
    source.push_str("pub const AVAILABLE_LANGUAGE_FEATURES: &[&str] = &[\n");
    for (name, _) in &profiles {
        source.push_str(&format!("\"lang-{name}\",\n"));
    }
    source.push_str("];\n");
    source.push_str("pub const ENABLED_LANGUAGE_FEATURES: &[&str] = &[\n");
    for (name, _) in &enabled {
        source.push_str(&format!("\"lang-{name}\",\n"));
    }
    source.push_str("];\n");
    for (name, path) in &enabled {
        source.push_str(&format!(
            "#[path = {:?}] pub mod {name};\n",
            path.to_str().expect("UTF-8 profile path")
        ));
    }
    source.push_str("static PROFILE_GROUPS: &[&[&dyn LanguageProfile]] = &[\n");
    for (name, _) in &enabled {
        source.push_str(&format!("{name}::PROFILES,\n"));
    }
    source.push_str("];\n");
    let output =
        std::path::PathBuf::from(std::env::var_os("OUT_DIR").expect("Cargo output directory"));
    std::fs::write(output.join("language_profiles.rs"), source).expect("write language registry");
}
