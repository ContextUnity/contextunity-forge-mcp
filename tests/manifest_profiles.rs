use contextunity_forge_mcp::engine::{ast, languages};

#[cfg(feature = "lang-typescript")]
#[test]
fn package_json_dependencies_include_runtime_development_and_peer_packages() {
    let profile = languages::require("typescript").unwrap();
    assert_eq!(profile.manifest_filenames(), ["package.json"]);
    let dependencies = profile.extract_manifest_dependencies(
        "package.json",
        r#"{"dependencies":{"react":"^18","@scope/pkg":"1"},"devDependencies":{"vitest":"1"},"peerDependencies":{"typescript":"5"},"optionalDependencies":{"ignored":"1"}}"#,
    );
    assert_eq!(
        dependencies,
        ["react", "@scope/pkg", "vitest", "typescript"]
    );
    assert!(profile.is_stdlib("fs"));
    assert!(profile.is_stdlib("node:fs/promises"));
    assert!(profile.is_stdlib("node:custom-built-in"));
    assert!(!profile.is_stdlib("@scope/fs"));
    assert!(profile.builtin("process"));
    assert!(profile.builtin("Buffer"));
}

#[cfg(feature = "lang-typescript")]
#[test]
fn commonjs_require_imports_bind_identifiers_and_destructured_exports() {
    let facts = ast::extract(
        "sample.cjs",
        "javascript",
        "const fs = require('fs');\nconst { readFile, writeFile: write } = require('node:fs/promises');\nfunction run(require) { const local = require('not-a-module'); }\n",
    )
    .unwrap();
    let imports: Vec<_> = facts
        .references
        .iter()
        .filter(|r| r.kind == "imports")
        .collect();
    assert_eq!(imports.len(), 3, "{imports:?}");
    assert!(imports.iter().any(|r| r.expression == "fs"
        && r.alias.as_deref() == Some("fs")
        && r.module.as_deref() == Some("fs")));
    assert!(imports.iter().any(|r| r.expression == "readFile"
        && r.alias.as_deref() == Some("readFile")
        && r.module.as_deref() == Some("node:fs/promises")));
    assert!(imports.iter().any(|r| r.expression == "writeFile"
        && r.alias.as_deref() == Some("write")
        && r.module.as_deref() == Some("node:fs/promises")));
    assert!(!imports
        .iter()
        .any(|r| r.module.as_deref() == Some("not-a-module")));
}

#[cfg(feature = "lang-rust")]
#[test]
fn cargo_dependencies_include_workspace_and_target_scoped_declarations() {
    let profile = languages::require("rust").unwrap();
    let manifest = "[dependencies]\nserde = \"1\"\n\"my-crate\" = { version = \"2\", package = \"actual-crate\" }\n\n[dev-dependencies]\npretty_assertions = \"1\"\n\n[target.'cfg(unix)'.dependencies]\nunix-only = \"1\"\n\n[workspace.dependencies]\nshared-crate = \"1\"\n\n[package.metadata.dependencies]\nnot-a-dependency = \"1\"\n\n[package]\nname = \"not-a-dependency\"\n";
    let dependencies = profile.extract_manifest_dependencies("Cargo.toml", manifest);
    assert_eq!(
        dependencies,
        [
            "my_crate",
            "pretty_assertions",
            "serde",
            "shared_crate",
            "unix_only"
        ]
    );
    assert!(profile.is_stdlib("std::collections"));
    assert!(profile.is_stdlib("core::fmt"));
    assert!(profile.is_stdlib("proc_macro::TokenStream"));
    assert!(!profile.is_stdlib("std_extra::thing"));
}

#[cfg(feature = "lang-go")]
#[test]
fn go_mod_dependencies_and_standard_packages_are_recognized() {
    let profile = languages::require("go").unwrap();
    assert_eq!(profile.manifest_filenames(), ["go.mod"]);
    let manifest = "module example.com/app\n\nrequire github.com/a/one v1.2.0\nrequire (\n\tgithub.com/b/two v2.0.0 // indirect\n)\n";
    assert_eq!(
        profile.extract_manifest_dependencies("go.mod", manifest),
        ["github.com/a/one", "github.com/b/two"]
    );
    assert!(profile.is_stdlib("net/http"));
    assert!(!profile.is_stdlib("github.com/net/http"));
}

#[cfg(feature = "lang-java")]
#[test]
fn java_reads_maven_and_gradle_dependencies_and_marks_jdk_packages() {
    let profile = languages::require("java").unwrap();
    let pom = "<project><dependencies><dependency><groupId>org.example</groupId><artifactId>lib</artifactId></dependency></dependencies></project>";
    assert_eq!(
        profile.extract_manifest_dependencies("pom.xml", pom),
        ["org.example"]
    );
    let gradle = "dependencies { implementation(\"com.acme:core:1.0\"); testImplementation(\"org.test:fixture:2\") }";
    assert_eq!(
        profile.extract_manifest_dependencies("build.gradle.kts", gradle),
        ["com.acme", "org.test"]
    );
    assert!(profile.is_stdlib("java.util"));
    assert!(profile.is_stdlib("javax.crypto"));
    assert!(!profile.is_stdlib("com.java.util"));
}

#[cfg(feature = "lang-kotlin")]
#[test]
fn kotlin_reads_gradle_dependencies_and_marks_kotlin_packages() {
    let profile = languages::require("kotlin").unwrap();
    assert_eq!(
        profile.extract_manifest_dependencies(
            "build.gradle",
            "dependencies { implementation 'org.example:library:1.0' }"
        ),
        ["org.example"]
    );
    assert!(profile.is_stdlib("kotlin.collections"));
    assert!(profile.is_stdlib("java.time"));
    assert!(!profile.is_stdlib("kotlinx.coroutines"));
}

#[cfg(feature = "lang-typescript")]
#[test]
fn commonjs_shadowed_require_stays_inside_its_actual_scope() {
    for source in [
        "function run(require) { const local = require('blocked'); } const fs = require('fs');",
        "function run(require = callback) { const local = require('blocked'); } const fs = require('fs');",
        "function run(...require) { const local = require('blocked'); } const fs = require('fs');",
        "const run = require => { const local = require('blocked'); }; const fs = require('fs');",
        "const run = (require = callback) => { const local = require('blocked'); }; const fs = require('fs');",
        "const run = function require() { const local = require('blocked'); }; const fs = require('fs');",
        "const run = function* require() { const local = require('blocked'); }; const fs = require('fs');",
        "const run = function*(require) { const local = require('blocked'); }; const fs = require('fs');",
        "try {} catch (require) { const local = require('blocked'); } const fs = require('fs');",
    ] {
        let facts = ast::extract("sample.cjs", "javascript", source).unwrap();
        let imports: Vec<_> = facts.references.iter().filter(|reference| reference.kind == "imports").collect();
        assert_eq!(imports.len(), 1, "{source}\n{imports:#?}");
        assert_eq!(imports[0].module.as_deref(), Some("fs"), "{source}");
        assert!(facts.references.iter().any(|reference| reference.kind == "calls" && reference.expression == "require"), "shadowed require remains ordinary call: {source}");
    }
}

#[cfg(feature = "lang-typescript")]
#[test]
fn commonjs_destructured_parameters_and_block_bindings_do_not_leak() {
    for source in [
        "function run({load: require}) { const local=require('blocked'); } const fs=require('fs');",
        "const run=({require})=>{ const local=require('blocked'); }; const fs=require('fs');",
        "{ const require=callback; const local=require('blocked'); } const fs=require('fs');",
    ] {
        let facts = ast::extract("sample.cjs", "javascript", source).unwrap();
        let imports: Vec<_> = facts
            .references
            .iter()
            .filter(|reference| reference.kind == "imports")
            .collect();
        assert_eq!(imports.len(), 1, "{source}\n{imports:#?}");
        assert_eq!(imports[0].module.as_deref(), Some("fs"));
    }
}
