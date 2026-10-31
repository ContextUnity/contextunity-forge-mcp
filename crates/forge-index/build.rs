fn main() {
    generate_profiles();
    if std::env::var_os("CARGO_FEATURE_LANG_PROTO").is_none() {
        return;
    }
    let proto_src = std::path::Path::new("../../vendor/tree-sitter-proto/src");
    let mut c_config = cc::Build::new();
    c_config.include(proto_src);
    c_config
        .flag_if_supported("-Wno-unused-parameter")
        .flag_if_supported("-Wno-unused-but-set-variable")
        .flag_if_supported("-Wno-trigraphs");
    c_config.file(proto_src.join("parser.c"));
    c_config.compile("tree-sitter-proto");
    println!("cargo:rerun-if-changed=../../vendor/tree-sitter-proto/src/parser.c");
}

fn generate_profiles() {
    let directory = std::path::Path::new("src/languages");
    let manifest = std::fs::read_to_string("Cargo.toml").expect("read Cargo manifest");
    println!("cargo:rerun-if-changed={}", directory.display());
    let mut profiles = Vec::new();
    for entry in std::fs::read_dir(directory).expect("read language profile directory") {
        let entry = entry.expect("read language profile entry");
        let entry_path = entry.path();
        let file_type = entry.file_type().expect("read profile file type");
        let (name, path) = if file_type.is_file()
            && entry_path.extension().and_then(|p| p.to_str()) == Some("rs")
        {
            (
                entry_path
                    .file_stem()
                    .and_then(|p| p.to_str())
                    .expect("UTF-8 profile filename")
                    .to_owned(),
                entry_path,
            )
        } else if file_type.is_dir() {
            let name = entry_path
                .file_name()
                .and_then(|p| p.to_str())
                .expect("UTF-8 profile directory name")
                .to_owned();
            let module_path = entry_path.join("mod.rs");
            if !module_path.is_file()
                || !manifest_declares_feature(&manifest, &format!("lang-{name}"))
            {
                continue;
            }
            (name, module_path)
        } else {
            continue;
        };
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
            name,
            path.canonicalize()
                .expect("canonical language profile path"),
        ));
    }
    profiles.sort_by(|a, b| a.0.cmp(&b.0));
    for pair in profiles.windows(2) {
        assert_ne!(
            pair[0].0, pair[1].0,
            "language profile has both a flat and directory module: {}",
            pair[0].0
        );
    }
    let enabled: Vec<_> = profiles
        .iter()
        .filter(|(name, _)| {
            let variable = format!("CARGO_FEATURE_LANG_{}", name.to_ascii_uppercase());
            println!("cargo:rerun-if-env-changed={variable}");
            std::env::var_os(variable).is_some()
        })
        .collect();

    let _ = generate_framework_manifests();

    let mut source = String::new();
    source.push_str("/// Language features recognized by this build.\n");
    source.push_str("pub const AVAILABLE_LANGUAGE_FEATURES: &[&str] = &[\n");
    for (name, _) in &profiles {
        source.push_str(&format!("\"lang-{name}\",\n"));
    }
    source.push_str("];\n");
    source.push_str("/// Language features compiled into this build.\n");
    source.push_str("pub const ENABLED_LANGUAGE_FEATURES: &[&str] = &[\n");
    for (name, _) in &enabled {
        source.push_str(&format!("\"lang-{name}\",\n"));
    }
    source.push_str("];\n");
    for (name, path) in &enabled {
        source.push_str(&format!(
            "/// {name} language profile and extraction rules.\n#[path = {:?}] pub mod {name};\n",
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

fn generate_framework_manifests() -> Vec<std::path::PathBuf> {
    let directory = std::path::Path::new("src/languages/manifests");
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
    let output =
        std::path::PathBuf::from(std::env::var_os("OUT_DIR").expect("Cargo output directory"));
    std::fs::write(output.join("framework_manifests.rs"), source)
        .expect("write framework inventory");
    manifests.into_iter().map(|(_, _, path)| path).collect()
}

fn manifest_declares_feature(manifest: &str, feature: &str) -> bool {
    let mut in_features = false;
    for line in manifest.lines() {
        let line = line.split('#').next().unwrap_or_default().trim();
        if line.starts_with('[') {
            in_features = line == "[features]";
            continue;
        }
        if in_features
            && line
                .split_once('=')
                .is_some_and(|(key, _)| key.trim().trim_matches('"') == feature)
        {
            return true;
        }
    }
    false
}
