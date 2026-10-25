//! Zero-allocation borrowed-slice string interner and strongly-typed 32-bit IDs for the linker.
//!
//! Provides `LinkerInterner` to deduplicate symbol and path strings borrowing directly
//! from the input facts catalog without heap reallocation. Also computes 64-bit Bloom filter
//! masks for rapid single-cycle import matching against declarative framework rules.

use crate::core::models::{stable_hash64, Facts, Node};
use hashbrown::{HashMap, HashSet};
use std::{
    borrow::Borrow,
    collections::BTreeMap,
    hash::{Hash, Hasher},
    sync::Arc,
};

/// Strongly-typed 32-bit identifier for an interned symbol.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SymbolId(pub u32);

/// Strongly-typed 32-bit identifier for an interned file path.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PathId(pub u32);

#[derive(Clone, Debug)]
enum InternedSymbol<'a> {
    Borrowed(&'a str),
    Owned(Arc<str>),
}

impl InternedSymbol<'_> {
    #[inline]
    fn as_str(&self) -> &str {
        match self {
            Self::Borrowed(symbol) => symbol,
            Self::Owned(symbol) => symbol,
        }
    }
}

impl Borrow<str> for InternedSymbol<'_> {
    fn borrow(&self) -> &str {
        self.as_str()
    }
}

impl PartialEq for InternedSymbol<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.as_str() == other.as_str()
    }
}

impl Eq for InternedSymbol<'_> {}

impl Hash for InternedSymbol<'_> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.as_str().hash(state);
    }
}

/// Zero-allocation borrowed-slice string interner for linker symbols and file paths.
#[derive(Clone, Debug)]
pub struct LinkerInterner<'a> {
    /// Forward lookup table for borrowed and owned symbols.
    symbol_to_id: HashMap<InternedSymbol<'a>, SymbolId>,
    /// Reverse lookup table from `SymbolId` index to its borrowed or registry-owned symbol.
    id_to_symbol: Vec<InternedSymbol<'a>>,
    /// Forward lookup table from borrowed file path slice to `PathId`.
    pub path_to_id: HashMap<&'a str, PathId>,
    /// Reverse lookup table from `PathId` index to borrowed file path slice.
    pub id_to_path: Vec<&'a str>,
    /// File-level import masks indexed directly by `PathId`.
    file_import_masks: Vec<u64>,
}

impl<'a> Default for LinkerInterner<'a> {
    fn default() -> Self {
        Self::with_capacity(0, 0)
    }
}

impl<'a> LinkerInterner<'a> {
    /// Creates a new `LinkerInterner` with pre-allocated capacities.
    pub fn with_capacity(symbols: usize, paths: usize) -> Self {
        Self {
            symbol_to_id: HashMap::with_capacity(symbols),
            id_to_symbol: Vec::with_capacity(symbols),
            path_to_id: HashMap::with_capacity(paths),
            id_to_path: Vec::with_capacity(paths),
            file_import_masks: Vec::with_capacity(paths),
        }
    }

    /// Interns a borrowed symbol string, returning its strongly-typed `SymbolId`.
    #[inline]
    pub fn intern_symbol(&mut self, s: &'a str) -> SymbolId {
        if let Some(id) = self.lookup_symbol(s) {
            return id;
        }
        let id = SymbolId(self.id_to_symbol.len() as u32);
        let symbol = InternedSymbol::Borrowed(s);
        self.symbol_to_id.insert(symbol.clone(), id);
        self.id_to_symbol.push(symbol);
        id
    }

    /// Interns a registry-owned symbol without borrowing from a temporary context handle.
    #[inline]
    pub fn intern_owned_symbol(&mut self, symbol: impl Into<Arc<str>>) -> SymbolId {
        let symbol = symbol.into();
        if let Some(id) = self.lookup_symbol(&symbol) {
            return id;
        }
        self.insert_owned_symbol(symbol)
    }

    /// Interns a borrowed registry symbol without allocating when it is already present.
    #[inline]
    pub(crate) fn intern_owned_symbol_str(&mut self, symbol: &str) -> SymbolId {
        if let Some(id) = self.lookup_symbol(symbol) {
            return id;
        }
        self.insert_owned_symbol(Arc::from(symbol))
    }

    #[inline]
    fn insert_owned_symbol(&mut self, symbol: Arc<str>) -> SymbolId {
        let id = SymbolId(self.id_to_symbol.len() as u32);
        let symbol = InternedSymbol::Owned(symbol);
        self.symbol_to_id.insert(symbol.clone(), id);
        self.id_to_symbol.push(symbol);
        id
    }

    /// Looks up a symbol string by value without allocating.
    #[inline]
    pub fn lookup_symbol(&self, s: &str) -> Option<SymbolId> {
        self.symbol_to_id.get(s).copied()
    }

    /// Looks up a node by its interned identity without allocating a key string.
    #[inline]
    pub fn lookup_node<'node>(
        &self,
        nodes: &HashMap<SymbolId, &'node Node>,
        id: &str,
    ) -> Option<&'node Node> {
        self.lookup_symbol(id)
            .and_then(|symbol| nodes.get(&symbol).copied())
    }

    /// Resolves a `SymbolId` back to its original borrowed slice.
    #[inline]
    pub fn resolve_symbol(&self, id: SymbolId) -> &str {
        self.id_to_symbol[id.0 as usize].as_str()
    }

    /// Interns a borrowed path string, returning its strongly-typed `PathId`.
    #[inline]
    pub fn intern_path(&mut self, p: &'a str) -> PathId {
        if let Some(&id) = self.path_to_id.get(p) {
            return id;
        }
        let id = PathId(self.id_to_path.len() as u32);
        self.path_to_id.insert(p, id);
        self.id_to_path.push(p);
        self.file_import_masks.push(0);
        id
    }

    /// Looks up a path string by value without allocating.
    #[inline]
    pub fn lookup_path(&self, p: &str) -> Option<PathId> {
        self.path_to_id.get(p).copied()
    }

    /// Resolves a `PathId` back to its original borrowed slice.
    #[inline]
    pub fn resolve_path(&self, id: PathId) -> &'a str {
        self.id_to_path[id.0 as usize]
    }

    /// Returns the precomputed import mask for an interned file path.
    #[inline]
    pub fn file_import_mask_for_path(&self, path: PathId) -> u64 {
        self.file_import_masks[path.0 as usize]
    }

    /// Stores the file-level import mask for an interned path.
    #[inline]
    pub fn set_file_import_mask(&mut self, path: PathId, mask: u64) {
        self.file_import_masks[path.0 as usize] = mask;
    }

    /// Returns the number of interned symbols.
    #[inline]
    pub fn symbol_count(&self) -> usize {
        self.id_to_symbol.len()
    }

    /// Returns the number of interned paths.
    #[inline]
    pub fn path_count(&self) -> usize {
        self.id_to_path.len()
    }

    /// Returns `true` if the interner contains no interned symbols or paths.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.id_to_symbol.is_empty() && self.id_to_path.is_empty()
    }

    /// Pre-interns catalog paths, node identities and semantic occurrence owners.
    /// An interned occurrence owner is not proof that its declaration exists.
    pub fn from_catalog<F: AsRef<Facts>>(all: &'a BTreeMap<String, F>) -> Self {
        let num_paths = all.len();
        let num_nodes: usize = all.values().map(|f| f.as_ref().nodes.len()).sum();
        let estimated_symbols = num_nodes.saturating_mul(3);
        let mut interner = Self::with_capacity(estimated_symbols, num_paths);
        let mut suffixed_paths = HashSet::<&'a str>::with_capacity(num_paths);

        for (path, facts) in all {
            let file_path = path.as_str();
            let path_id = interner.intern_path(file_path);
            if suffixed_paths.insert(file_path) {
                intern_module_path_suffixes(&mut interner, file_path);
            }
            let facts = facts.as_ref();
            interner.set_file_import_mask(path_id, Self::file_import_mask(facts));
            for (index, node) in facts.nodes.iter().enumerate() {
                let node_path = node.path.as_str();
                if node_path != file_path && suffixed_paths.insert(node_path) {
                    interner.intern_path(node_path);
                    intern_module_path_suffixes(&mut interner, node_path);
                }
                interner.intern_symbol(node.id.as_str());
                interner.intern_symbol(node.name.as_str());
                interner.intern_symbol(node.qualname.as_str());
                interner.intern_symbol(node.language.as_str());
                for name in node.binding_names().chain(node.rebinding_names()) {
                    interner.intern_symbol(name);
                }
                if !facts.semantic.is_complete() && facts.semantic.state(index).is_none() {
                    for fact in node.details["semantic_facts"]
                        .as_array()
                        .into_iter()
                        .flatten()
                    {
                        if let Some(expression) = fact["expression"].as_str() {
                            interner.intern_symbol(expression);
                        }
                        if let Some(owner) = fact["occurrence"]["owner_id"].as_str() {
                            interner.intern_symbol(owner);
                        }
                    }
                }
                for export in node.details["exports"].as_array().into_iter().flatten() {
                    for key in ["name", "local", "module"] {
                        if let Some(value) = export.get(key).and_then(serde_json::Value::as_str) {
                            interner.intern_symbol(value);
                        }
                    }
                }
            }
            for reference in &facts.references {
                interner.intern_symbol(&reference.expression);
                if let Some(module) = reference.module.as_deref() {
                    interner.intern_symbol(module);
                }
                if let Some(alias) = reference.alias.as_deref() {
                    interner.intern_symbol(alias);
                }
            }
            for (_, entry) in facts.semantic.entries() {
                if let crate::core::typed_facts::SemanticState::Valid { facts, .. } = &entry.state {
                    for fact in facts {
                        interner.intern_symbol(&fact.expression);
                        interner.intern_symbol(&fact.occurrence.owner_id);
                    }
                }
            }
        }

        interner
    }

    /// Computes a 64-bit Bloom bitmask from all `reference.kind == "imports"` module/expression root segments
    /// in `facts.references` (using a deterministic FNV-1a 2-bit Bloom filter per root package token) so framework
    /// rule evaluators can test `(file_mask & rule_mask) == rule_mask` in 1 CPU cycle.
    #[inline]
    pub fn file_import_mask(facts: &Facts) -> u64 {
        let mut mask = 0u64;
        for reference in facts.references.iter().filter(|r| r.kind == "imports") {
            if let Some(module) = reference.module.as_deref().filter(|s| !s.is_empty()) {
                mask |= Self::package_mask(module);
            }
            if !reference.expression.is_empty() {
                mask |= Self::package_mask(&reference.expression);
            }
        }
        mask
    }

    /// Computes the corresponding 2-bit Bloom mask for a package root name (`package.split(['.', '/', ':']).next().unwrap_or(package)`).
    #[inline]
    pub fn package_mask(package: &str) -> u64 {
        let root = package.split(['.', '/', ':']).next().unwrap_or(package);
        if root.is_empty() {
            return 0;
        }
        let hash = stable_hash64(root) as u64;
        let bit1 = (hash & 0x3f) as u32;
        let mut bit2 = ((hash >> 32) & 0x3f) as u32;
        if bit1 == bit2 {
            bit2 = (bit1 + 1) & 0x3f;
        }
        (1u64 << bit1) | (1u64 << bit2)
    }
}

fn intern_module_path_suffixes<'a>(interner: &mut LinkerInterner<'a>, path: &'a str) {
    for start in std::iter::once(0).chain(path.match_indices('/').map(|(index, _)| index + 1)) {
        let suffix = &path[start..];
        if suffix.is_empty() {
            continue;
        }
        interner.intern_path(suffix);
        if let Some((stem, extension)) = suffix.rsplit_once('.') {
            if !stem.is_empty()
                && !extension.is_empty()
                && extension.bytes().all(|b| b.is_ascii_alphanumeric())
            {
                interner.intern_path(stem);
            }
        }
    }
}

/// Module-level convenience function forwarding to `LinkerInterner::package_mask`.
#[inline]
pub fn package_mask(package: &str) -> u64 {
    LinkerInterner::package_mask(package)
}

/// Module-level convenience function forwarding to `LinkerInterner::file_import_mask`.
#[inline]
pub fn file_import_mask(facts: &Facts) -> u64 {
    LinkerInterner::file_import_mask(facts)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::models::{Node, Reference};

    #[test]
    fn symbol_id_round_trip() {
        let mut interner = LinkerInterner::with_capacity(16, 16);
        let s1 = "symbol_one";
        let s2 = "symbol_two";

        let id1 = interner.intern_symbol(s1);
        let id2 = interner.intern_symbol(s2);
        let id1_again = interner.intern_symbol(s1);

        assert_eq!(id1, id1_again);
        assert_ne!(id1, id2);
        assert_eq!(interner.lookup_symbol("symbol_one"), Some(id1));
        assert_eq!(interner.lookup_symbol("symbol_two"), Some(id2));
        assert_eq!(interner.lookup_symbol("nonexistent"), None);

        assert_eq!(interner.resolve_symbol(id1), s1);
        assert_eq!(interner.resolve_symbol(id2), s2);
        assert_eq!(interner.symbol_count(), 2);
    }

    #[test]
    fn path_id_round_trip() {
        let mut interner = LinkerInterner::with_capacity(16, 16);
        let p1 = "src/engine/linker.rs";
        let p2 = "src/core/models.rs";

        let id1 = interner.intern_path(p1);
        let id2 = interner.intern_path(p2);
        let id1_again = interner.intern_path(p1);

        assert_eq!(id1, id1_again);
        assert_ne!(id1, id2);
        assert_eq!(interner.lookup_path("src/engine/linker.rs"), Some(id1));
        assert_eq!(interner.lookup_path("src/core/models.rs"), Some(id2));
        assert_eq!(interner.lookup_path("not/found.rs"), None);

        assert_eq!(interner.resolve_path(id1), p1);
        assert_eq!(interner.resolve_path(id2), p2);
        assert_eq!(interner.path_count(), 2);
    }

    #[test]
    fn from_catalog_pre_interns_paths_and_symbols() {
        let mut catalog = BTreeMap::new();
        let mut facts1 = Facts::default();
        facts1.nodes.push(Node {
            native_semantic: None,
            id: "node_1".into(),
            kind: "function".into(),
            name: "test_fn".into(),
            qualname: "mod::test_fn".into(),
            path: "src/file1.rs".into(),
            line: 1,
            end_line: 5,
            is_test: false,
            language: "rust".into(),
            generated: false,
            details: serde_json::json!({
                "semantic_facts": [{"occurrence": {"owner_id": "missing_owner"}}]
            }),
        });
        facts1.references.push(Reference {
            source: "node_1".into(),
            expression: "fastapi.routing".into(),
            kind: "imports".into(),
            line: 1,
            column: 0,
            alias: None,
            module: Some("fastapi.routing".into()),
            dynamic: false,
            receiver_hint: None,
        });

        let mut facts2 = Facts::default();
        facts2.nodes.push(Node {
            native_semantic: None,
            id: "node_2".into(),
            kind: "class".into(),
            name: "TestClass".into(),
            qualname: "pkg.TestClass".into(),
            path: "src/file2.py".into(),
            line: 10,
            end_line: 20,
            is_test: false,
            language: "python".into(),
            generated: false,
            details: serde_json::Value::Null,
        });

        catalog.insert("src/file1.rs".into(), facts1);
        catalog.insert("src/file2.py".into(), facts2);

        let interner = LinkerInterner::from_catalog(&catalog);

        // Paths verified
        assert!(interner.lookup_path("src/file1.rs").is_some());
        assert!(interner.lookup_path("src/file2.py").is_some());
        assert_eq!(
            interner.file_import_mask_for_path(
                interner
                    .lookup_path("src/file1.rs")
                    .expect("file path is interned")
            ),
            LinkerInterner::package_mask("fastapi")
        );

        // Symbols verified
        let id_node1 = interner
            .lookup_symbol("node_1")
            .expect("node_1 must be interned");
        assert_eq!(interner.resolve_symbol(id_node1), "node_1");
        assert!(interner.lookup_symbol("test_fn").is_some());
        assert!(interner.lookup_symbol("mod::test_fn").is_some());

        let id_node2 = interner
            .lookup_symbol("node_2")
            .expect("node_2 must be interned");
        assert_eq!(interner.resolve_symbol(id_node2), "node_2");
        assert!(interner.lookup_symbol("TestClass").is_some());
        assert!(interner.lookup_symbol("pkg.TestClass").is_some());
        let missing = interner
            .lookup_symbol("missing_owner")
            .expect("raw semantic owner");
        assert_eq!(interner.resolve_symbol(missing), "missing_owner");
        assert!(catalog
            .values()
            .flat_map(|facts| &facts.nodes)
            .all(|node| node.id != "missing_owner"));
    }

    #[test]
    fn package_mask_bloom_properties() {
        assert_eq!(LinkerInterner::package_mask(""), 0);

        let django_mask = LinkerInterner::package_mask("django");
        assert_eq!(django_mask.count_ones(), 2);

        // Splitting on '.', '/', ':'
        assert_eq!(
            LinkerInterner::package_mask("django.shortcuts.render"),
            django_mask
        );
        assert_eq!(
            LinkerInterner::package_mask("django/views/generic"),
            django_mask
        );

        let rust_std_mask = LinkerInterner::package_mask("std::collections::HashMap");
        assert_eq!(LinkerInterner::package_mask("std"), rust_std_mask);
        assert_eq!(rust_std_mask.count_ones(), 2);

        let fastapi_mask = LinkerInterner::package_mask("fastapi");
        assert_eq!(fastapi_mask.count_ones(), 2);
        assert_ne!(django_mask, fastapi_mask);
    }

    #[test]
    fn file_import_mask_filtering() {
        let mut facts = Facts::default();
        facts.references.push(Reference {
            source: "src1".into(),
            expression: "django".into(),
            kind: "imports".into(),
            line: 1,
            column: 0,
            alias: None,
            module: Some("django.http".into()),
            dynamic: false,
            receiver_hint: None,
        });
        facts.references.push(Reference {
            source: "src2".into(),
            expression: "fastapi".into(),
            kind: "imports".into(),
            line: 2,
            column: 0,
            alias: None,
            module: None,
            dynamic: false,
            receiver_hint: None,
        });
        // A calls reference must not affect the imports mask
        facts.references.push(Reference {
            source: "src3".into(),
            expression: "unrelated_call".into(),
            kind: "calls".into(),
            line: 3,
            column: 0,
            alias: None,
            module: None,
            dynamic: false,
            receiver_hint: None,
        });

        let file_mask = LinkerInterner::file_import_mask(&facts);

        let django_mask = LinkerInterner::package_mask("django");
        let fastapi_mask = LinkerInterner::package_mask("fastapi");
        let flask_mask = LinkerInterner::package_mask("flask");
        let unrelated_mask = LinkerInterner::package_mask("unrelated_call");

        // Evaluators test: (file_mask & rule_mask) == rule_mask in 1 CPU cycle
        assert_eq!(
            file_mask & django_mask,
            django_mask,
            "file must match django rule"
        );
        assert_eq!(
            file_mask & fastapi_mask,
            fastapi_mask,
            "file must match fastapi rule"
        );
        assert_ne!(
            file_mask & flask_mask,
            flask_mask,
            "file must NOT match flask rule"
        );
        assert_ne!(
            file_mask & unrelated_mask,
            unrelated_mask,
            "file must NOT match calls reference"
        );
    }
}
