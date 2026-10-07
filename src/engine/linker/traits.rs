//! Language-owned import and receiver resolution used by the shared linker.

use crate::{
    core::models::{Facts, Node, Reference},
    engine::languages::{
        manifests::{DependencyRegistry, FrameworkManifest},
        ImportPath, LanguageFamily, LanguageProfile,
    },
};
use hashbrown::HashMap;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

/// Package re-exports indexed by owner file and imported name.
pub type PackageExports<'a> = HashMap<&'a str, HashMap<&'a str, Option<&'a Node>>>;

/// Cross-file indexes available while resolving one import reference.
pub type ModulesByNamespace<'a> = HashMap<LanguageFamily, HashMap<String, Vec<&'a Node>>>;

/// Input for a language linker resolving one import.
pub struct ImportContext<'ctx, 'a, 'input> {
    /// Workspace-relative importing file.
    pub path: &'input str,
    /// Dependency and framework manifests admitted for the indexed workspace.
    pub dependencies: &'ctx DependencyRegistry,
    /// Parsed import reference.
    pub reference: &'input Reference,
    /// Normalized import path from the language profile.
    pub normalized: Option<&'input ImportPath>,
    /// Namespace prefix that selected the indexed module.
    pub selected_namespace: &'input str,
    /// Language family of the importing file.
    pub family: LanguageFamily,
    /// Candidate modules for the import.
    pub modules: &'ctx mut Vec<&'a Node>,
    /// Candidate symbols selected by the linker.
    pub candidates: &'ctx mut Vec<&'a Node>,
    /// Modules grouped under language-specific import namespaces.
    pub modules_by_namespace: &'a ModulesByNamespace<'a>,
    /// Nodes grouped by source file.
    pub by_module: &'a HashMap<&'a str, Vec<&'a Node>>,
    /// Nodes grouped by fully qualified name.
    pub by_qual: &'a HashMap<&'a str, Vec<&'a Node>>,
    /// Explicit package re-exports discovered for the workspace.
    pub package_exports: &'a PackageExports<'a>,
    /// Reusable buffer for qualified-name lookup.
    pub lookup_key: &'ctx mut String,
}

impl ImportContext<'_, '_, '_> {
    /// Returns the imported symbol suffix, or an empty suffix for a module import.
    pub fn tail(&self) -> Option<&str> {
        let normalized = self.normalized?;
        Some(if normalized.symbol_path {
            normalized
                .namespace
                .strip_prefix(self.selected_namespace)
                .unwrap_or("")
                .trim_start_matches('.')
        } else if self.reference.expression == "*"
            || self.reference.module.as_deref() == Some(&self.reference.expression)
        {
            ""
        } else {
            &self.reference.expression
        })
    }
}

/// Language-specific import-resolution details returned to the common graph builder.
#[derive(Default)]
pub struct ImportResolution<'a> {
    /// Paired declaration-only module used when the runtime module lacks a symbol.
    pub paired_stub: Option<&'a Node>,
    /// The selected import names a child module rather than a declaration.
    pub child_module: bool,
    /// The selected declaration comes from a paired stub.
    pub stub_symbol: bool,
    /// The selected declaration is re-exported through a package initializer.
    pub reexport_symbol: bool,
}

/// Owns language-specific import, receiver, and incremental-linking rules.
pub trait LanguageLinker: Send + Sync {
    /// Resolves a parsed import against indexed modules and declarations.
    fn resolve_import<'ctx, 'a, 'input>(
        &self,
        context: &mut ImportContext<'ctx, 'a, 'input>,
    ) -> ImportResolution<'a> {
        resolve_default_import(context)
    }

    /// Reports whether a receiver name is valid for its owning declaration.
    fn resolve_receiver(&self, profile: &dyn LanguageProfile, name: &str, owner: &Node) -> bool {
        profile.receiver(name, owner)
    }

    /// Returns a declared framework's user rules for the owning source path.
    ///
    /// Framework data stays behind the existing language-linker seam. The
    /// dependency registry enforces package declaration and workspace scope.
    fn framework_manifest<'a>(
        &self,
        dependencies: &'a DependencyRegistry,
        family: LanguageFamily,
        path: &str,
        framework: &str,
        package: &str,
    ) -> Option<&'a FrameworkManifest> {
        dependencies.framework_manifest_for_path(family, path, framework, package)
    }

    /// Returns whether a declared framework owns a receiver rule.
    fn framework_receiver(
        &self,
        dependencies: &DependencyRegistry,
        family: LanguageFamily,
        path: &str,
        framework: &str,
        package: &str,
        rule: &str,
    ) -> bool {
        self.framework_manifest(dependencies, family, path, framework, package)
            .is_some_and(|manifest| manifest.has_receiver(rule))
    }

    /// Returns whether a declared framework owns a builtin rule.
    fn framework_builtin(
        &self,
        dependencies: &DependencyRegistry,
        family: LanguageFamily,
        path: &str,
        framework: &str,
        package: &str,
        rule: &str,
    ) -> bool {
        self.framework_manifest(dependencies, family, path, framework, package)
            .is_some_and(|manifest| manifest.has_builtin(rule))
    }

    /// Returns whether a declared framework owns a template-filter rule.
    fn framework_filter(
        &self,
        dependencies: &DependencyRegistry,
        family: LanguageFamily,
        path: &str,
        framework: &str,
        package: &str,
        rule: &str,
    ) -> bool {
        self.framework_manifest(dependencies, family, path, framework, package)
            .is_some_and(|manifest| manifest.has_filter(rule))
    }

    /// Returns the target associated with a declared framework route rule.
    fn framework_route_target<'a>(
        &self,
        dependencies: &'a DependencyRegistry,
        family: LanguageFamily,
        path: &str,
        framework: &str,
        package: &str,
        source: &str,
    ) -> Option<&'a str> {
        self.framework_manifest(dependencies, family, path, framework, package)?
            .route_target(source)
    }

    /// Resolves a member through language-specific companion declarations.
    fn resolve_imported_member<'a>(
        &self,
        _module: &'a Node,
        _member: &str,
        _by_module: &HashMap<&'a str, Vec<&'a Node>>,
        _by_qual: &HashMap<&'a str, Vec<&'a Node>>,
        _exports: &PackageExports<'a>,
        _lookup_key: &mut String,
    ) -> Vec<&'a Node> {
        Vec::new()
    }

    /// Builds language-specific package re-export indexes before parallel linking.
    fn package_exports<'a>(
        &self,
        _all: &'a BTreeMap<String, Facts>,
        _modules_by_namespace: &ModulesByNamespace<'a>,
        _by_module: &HashMap<&'a str, Vec<&'a Node>>,
        _root: Option<&Path>,
    ) -> PackageExports<'a> {
        HashMap::new()
    }

    /// Builds package re-export indexes from borrowed per-file facts.
    ///
    /// The borrowed form avoids cloning facts during incremental relinking.
    fn package_exports_borrowed<'a>(
        &self,
        _all: &'a BTreeMap<String, &'a Facts>,
        _modules_by_namespace: &ModulesByNamespace<'a>,
        _by_module: &HashMap<&'a str, Vec<&'a Node>>,
        _root: Option<&Path>,
    ) -> PackageExports<'a> {
        HashMap::new()
    }

    /// Returns files whose full facts are required to relink affected owners.
    fn required_full_facts(
        &self,
        _facts: &BTreeMap<String, Facts>,
        _affected: &BTreeSet<String>,
        _catalog: &[(&str, &str)],
    ) -> BTreeSet<String> {
        BTreeSet::new()
    }

    /// Returns files whose borrowed full facts are needed to relink affected owners.
    ///
    /// Implementations keep the returned set scoped to the affected import graph.
    fn required_full_facts_borrowed(
        &self,
        _facts: &BTreeMap<String, &Facts>,
        _affected: &BTreeSet<String>,
        _catalog: &[(&str, &str)],
    ) -> BTreeSet<String> {
        BTreeSet::new()
    }

    /// Reports whether a file's references can change identity when it changes.
    fn needs_reference_identity(&self, _path: &str, _facts: &Facts) -> bool {
        false
    }

    /// Compares language-specific reference identity across an incremental update.
    fn reference_identity_changed(&self, _path: &str, _old: &Facts, _new: &Facts) -> bool {
        false
    }

    /// Reports whether an indexed path is a declaration-only companion file.
    fn is_declaration_only(&self, _path: &str) -> bool {
        false
    }

    /// Reports whether unresolved external imports provide call aliases.
    fn tracks_external_aliases(&self) -> bool {
        false
    }
}

/// Generic linker used by profiles without language-specific resolution rules.
pub struct GenericLanguageLinker;

/// Shared generic linker instance.
pub static GENERIC_LINKER: GenericLanguageLinker = GenericLanguageLinker;

impl LanguageLinker for GenericLanguageLinker {}

pub(crate) fn resolve_default_import<'ctx, 'a, 'input>(
    context: &mut ImportContext<'ctx, 'a, 'input>,
) -> ImportResolution<'a> {
    let Some(tail) = context.tail().map(str::to_owned) else {
        return ImportResolution::default();
    };
    let Some(module) = context
        .modules
        .first()
        .copied()
        .filter(|_| context.modules.len() == 1)
    else {
        return ImportResolution::default();
    };
    if tail.is_empty() {
        context.candidates.push(module);
        return ImportResolution::default();
    }
    context.lookup_key.clear();
    context.lookup_key.push_str(&module.qualname);
    context.lookup_key.push('.');
    context.lookup_key.push_str(&tail);
    let target = context.lookup_key.as_str();
    context.candidates.extend(
        context
            .by_module
            .get(module.path.as_str())
            .into_iter()
            .flatten()
            .copied()
            .filter(|node| {
                if tail == "default" {
                    node.details["default_export"] == true
                } else {
                    node.kind != "component" && node.qualname == target
                }
            }),
    );
    ImportResolution::default()
}
