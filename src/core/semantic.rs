//! Language-neutral evidence for statically inferred receiver types.

use serde::ser::SerializeSeq;
use serde::{Deserialize, Serialize, Serializer};
use std::collections::BTreeMap;

use super::{
    models::Facts,
    typed_facts::{FlowStore, TypedFacts},
};

/// Workspace linking inputs, without storage-only syntax and documentation.
///
/// Definitions retain their namespace, container, import and export metadata.
/// References and typed value flow remain intact, including source positions
/// used by lexical visibility and assignment ordering.
#[derive(Default)]
pub struct SymbolCatalog {
    pub(crate) files: BTreeMap<String, TypedFacts>,
    pub(crate) occurrences: BTreeMap<String, Vec<u8>>,
}

impl SymbolCatalog {
    /// Consumes extracted facts and keeps their linking projection.
    pub fn from_facts(files: BTreeMap<String, Facts>) -> Self {
        let mut catalog = Self::default();
        for (path, facts) in files {
            catalog.insert_typed(path, TypedFacts::from_public(facts), false);
        }
        catalog
    }

    /// Borrows declarations and references for one workspace file.
    /// Typed value flow is retained separately and borrowed by the linker.
    pub fn file(&self, path: &str) -> Option<&Facts> {
        self.files.get(path).map(AsRef::as_ref)
    }

    /// Restores one file's reference occurrences while definitions stay borrowed.
    pub fn references(&self, path: &str) -> Option<Vec<super::models::Reference>> {
        self.files.get(path).map(|facts| {
            self.occurrences.get(path).map_or_else(
                || facts.references.clone(),
                |bytes| decode_catalog_references(bytes, &facts.references),
            )
        })
    }

    pub(crate) fn into_files(self) -> BTreeMap<String, TypedFacts> {
        self.files
    }

    pub(crate) fn extend(&mut self, other: Self) {
        self.files.extend(other.files);
        self.occurrences.extend(other.occurrences);
    }

    pub(crate) fn insert_typed(
        &mut self,
        path: String,
        mut facts: TypedFacts,
        retain_storage: bool,
    ) -> CatalogStoragePayload {
        let mut payload = CatalogStoragePayload::default();
        if retain_storage {
            payload.flows = facts.flows.clone();
        }
        if facts
            .nodes
            .iter()
            .any(|node| node.kind == "module" && node.language == "python")
        {
            thread_local! {
                static COMPRESSOR: std::cell::RefCell<(zstd::bulk::Compressor<'static>, Vec<u8>)> =
                    std::cell::RefCell::new((zstd::bulk::Compressor::new(1)
                        .expect("reference compressor initialization"), Vec::new()));
            }
            let packed = COMPRESSOR
                .with(|slot| {
                    let mut scratch = slot.borrow_mut();
                    let (compressor, bytes) = &mut *scratch;
                    bytes.clear();
                    crate::db::writer::cache::encode_references(&facts.references, bytes);
                    compressor.compress(bytes)
                })
                .expect("in-memory reference compression");
            self.occurrences.insert(path.clone(), packed);
            if retain_storage {
                payload.references = Some(facts.references.clone());
            }
            // Imports and receiver hints participate in workspace indexes. Plain
            // Python occurrences are consumed only by the per-file reference pass.
            facts.facts.references.retain(|reference| {
                !matches!(reference.kind.as_str(), "calls" | "references")
                    || reference.receiver_hint.is_some()
            });
            facts.facts.references.shrink_to_fit();
        }
        for node in &facts.facts.nodes {
            if matches!(
                facts.flows.state(node),
                Some(super::typed_facts::FlowState::Valid {
                    original: Some(_),
                    ..
                })
            ) {
                facts.flows.get_mut(&node.id);
            }
        }
        for node in &mut facts.nodes {
            if !retain_storage {
                if let Some(details) = node.details.as_object_mut() {
                    details.retain(|key, _| !storage_only_detail(key));
                }
                continue;
            }
            let Some(details) = node.details.as_object() else {
                continue;
            };
            if !details.keys().any(|key| storage_only_detail(key)) {
                continue;
            }
            // Keep the original object, including property order, for durable
            // facts and commitment bytes. Only the smaller linking view is copied.
            let lean = details
                .iter()
                .filter(|(key, _)| !storage_only_detail(key))
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect();
            let original = std::mem::replace(&mut node.details, serde_json::Value::Object(lean));
            if retain_storage {
                payload.node_details.push((node.id.clone(), original));
            }
        }
        for (index, doc) in facts.docs.iter_mut().enumerate() {
            let stored = (
                index,
                std::mem::take(&mut doc.content),
                std::mem::take(&mut doc.invariants),
            );
            if retain_storage {
                payload.docs.push(stored);
            }
        }
        self.files.insert(path, facts);
        payload
    }
}

fn storage_only_detail(key: &str) -> bool {
    matches!(
        key,
        "doc" | "docstring" | "syntax_tree" | "occurrence_token_spans"
    )
}

/// Storage-only fields recovered from one file's durable fact blob.
#[derive(Default)]
pub(crate) struct CatalogStoragePayload {
    node_details: Vec<(String, serde_json::Value)>,
    docs: Vec<(usize, String, Vec<String>)>,
    flows: FlowStore,
    references: Option<Vec<super::models::Reference>>,
}

impl CatalogStoragePayload {
    pub(crate) fn restore_composed_node(&self, node: &mut super::models::Node) {
        let Some(source_id) = node.details["source_id"].as_str() else {
            return;
        };
        if let Some((_, original)) = self.node_details.iter().find(|(id, _)| id == source_id) {
            let mut original = original.clone();
            if let (Some(original), Some(lean)) =
                (original.as_object_mut(), node.details.as_object_mut())
            {
                original.extend(std::mem::take(lean));
            }
            node.details = original;
        }
    }

    pub(crate) fn restore(mut self, facts: &mut TypedFacts) {
        if let Some(mut references) = self.references {
            append_composed_references(&mut references, &facts.references);
            facts.facts.references = references;
        }
        let mut originals: hashbrown::HashMap<_, _> = self.node_details.into_iter().collect();
        for node in &mut facts.nodes {
            if let Some(mut original) = originals.remove(&node.id) {
                if let (Some(original), Some(lean)) =
                    (original.as_object_mut(), node.details.as_object_mut())
                {
                    // Preserve semantic updates made during workspace composition.
                    original.extend(std::mem::take(lean));
                }
                node.details = original;
            }
        }
        for (index, content, invariants) in self.docs {
            facts.docs[index].content = content;
            facts.docs[index].invariants = invariants;
        }
        facts.flows.append(&mut self.flows);
    }
}

fn append_composed_references(
    references: &mut Vec<super::models::Reference>,
    composed: &[super::models::Reference],
) {
    let original_len = references.len();
    for reference in composed
        .iter()
        .filter(|reference| reference.source.contains(":included:"))
    {
        if !references[..original_len].iter().any(|original| {
            original.source == reference.source
                && original.kind == reference.kind
                && original.expression == reference.expression
                && original.line == reference.line
                && original.column == reference.column
        }) {
            references.push(reference.clone());
        }
    }
}

pub(crate) fn decode_catalog_references(
    bytes: &[u8],
    composed: &[super::models::Reference],
) -> Vec<super::models::Reference> {
    use std::io::Read;
    thread_local! {
        static DECODER: std::cell::RefCell<(zstd::zstd_safe::DCtx<'static>, Vec<u8>)> =
            std::cell::RefCell::new((zstd::zstd_safe::DCtx::create(), Vec::new()));
    }
    let mut references = DECODER.with(|slot| {
        let mut scratch = slot.borrow_mut();
        let (context, decoded) = &mut *scratch;
        context.init().expect("reference decoder initialization");
        decoded.clear();
        zstd::stream::read::Decoder::with_context(bytes, context)
            .read_to_end(decoded)
            .expect("catalog reference compression is lossless");
        crate::db::writer::cache::decode_references(decoded).unwrap_or_else(|| {
            serde_json::from_slice(decoded)
                .expect("catalog reference JSON was serialized internally")
        })
    });
    append_composed_references(&mut references, composed);
    references
}

pub(crate) const DEFAULT_FACTS_PER_SCOPE: usize = 65_536;

/// Canonical Python literal constructions identify intrinsic types even when
/// their ordinary constructor names are shadowed by local declarations.
const PYTHON_LITERAL_CONSTRUCTORS: [(&str, &str); 4] = [
    ("{}", "dict"),
    ("[]", "list"),
    ("{...}", "set"),
    ("()", "tuple"),
];

pub(crate) fn python_literal_type(callee: &str) -> Option<&'static str> {
    PYTHON_LITERAL_CONSTRUCTORS
        .iter()
        .find_map(|(literal, builtin)| (*literal == callee).then_some(*builtin))
}

pub(crate) fn python_literal_constructor(builtin: &str) -> Option<&'static str> {
    PYTHON_LITERAL_CONSTRUCTORS
        .iter()
        .find_map(|(literal, name)| (*name == builtin).then_some(*literal))
}

#[derive(
    Clone, Copy, Debug, Default, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize,
)]
#[serde(deny_unknown_fields)]
/// Represents source position data.
pub struct SourcePosition {
    /// The line value.
    pub line: usize,
    /// The column value.
    pub column: usize,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
/// Enumerates the supported type expr values.
pub enum TypeExpr {
    /// Type identified by a declaration name.
    Named {
        /// Declared type name.
        name: String,
    },
    /// Generic type with resolved type arguments.
    Applied {
        /// Generic type name.
        base: String,
        /// Type arguments in source order.
        args: Vec<TypeExpr>,
    },
    /// Represents the unknown case.
    Unknown,
}

/// Bounded positional evidence for a canonical call.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CallSiteEvidence {
    /// Original inner call start.
    pub position: SourcePosition,
    /// Statically represented nested call receiver.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub receiver: Option<Box<ValueExpr>>,
    /// At most four source-ordered positional arguments.
    pub arguments: Vec<StaticArgument>,
}

/// Literal argument evidence; all other syntax remains unknown.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum StaticArgument {
    /// A decoded, bounded string literal.
    String {
        /// Literal contents.
        value: String,
    },
    /// No static literal proof.
    Unknown,
}

impl ValueExpr {
    /// Checks only the additive call evidence; legacy value validation remains authoritative.
    pub fn valid_call_sites(&self, depth: usize) -> bool {
        if depth > 8 {
            return false;
        }
        match self {
            Self::Call {
                site: Some(site), ..
            } => {
                site.position.line > 0
                    && site.arguments.len() <= 4
                    && site.arguments.iter().all(|argument| match argument {
                        StaticArgument::String { value } => value.len() <= 256,
                        StaticArgument::Unknown => true,
                    })
                    && site
                        .arguments
                        .iter()
                        .map(|argument| match argument {
                            StaticArgument::String { value } => value.len(),
                            StaticArgument::Unknown => 0,
                        })
                        .sum::<usize>()
                        <= 1024
                    && site
                        .receiver
                        .as_deref()
                        .is_none_or(|value| value.valid_call_sites(depth + 1))
            }
            Self::Await { value } | Self::Scoped { value, .. } | Self::Project { value, .. } => {
                value.valid_call_sites(depth + 1)
            }
            Self::BranchJoin { alternatives } => alternatives
                .iter()
                .all(|value| value.valid_call_sites(depth + 1)),
            _ => true,
        }
    }
}

fn deserialize_call_site<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<CallSiteEvidence>, D::Error> {
    let site = Option::<CallSiteEvidence>::deserialize(deserializer)?;
    let probe = ValueExpr::Call {
        callee: String::new(),
        site,
    };
    if !probe.valid_call_sites(0) {
        return Err(serde::de::Error::custom("invalid bounded call evidence"));
    }
    let ValueExpr::Call { site, .. } = probe else {
        unreachable!()
    };
    Ok(site)
}

/// Strict typed-reference ingress, shared with canonical full facts.
pub fn deserialize_typed_call<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<ValueExpr, D::Error> {
    let value = ValueExpr::deserialize(deserializer)?;
    if !matches!(value, ValueExpr::Call { .. }) || !value.valid_call_sites(0) {
        return Err(serde::de::Error::custom(
            "typed call hint requires bounded call",
        ));
    }
    Ok(value)
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
/// Enumerates the supported value expr values.
pub enum ValueExpr {
    /// Value with an explicit type annotation.
    Annotated {
        /// Annotated type.
        type_expr: TypeExpr,
    },
    /// Value created by a constructor call.
    Construct {
        /// Constructor expression.
        callee: String,
    },
    /// Value bound to another name.
    Alias {
        /// Referenced name.
        name: String,
    },
    /// Value returned by a call.
    Call {
        /// Called expression.
        callee: String,
        /// Optional original call evidence; legacy calls retain their behavior.
        #[serde(
            default,
            deserialize_with = "deserialize_call_site",
            skip_serializing_if = "Option::is_none"
        )]
        site: Option<CallSiteEvidence>,
    },
    /// Result obtained by awaiting an expression.
    Await {
        /// Awaited value expression.
        value: Box<ValueExpr>,
    },
    /// Element bound by a Python for-loop while its body executes.
    LoopElement {
        /// Statically named iterable.
        iterable: String,
        /// Exclusive end of the loop body.
        body_end: SourcePosition,
    },
    /// A lexical value available only until its enclosing block ends.
    Scoped {
        /// Value established inside the block.
        value: Box<ValueExpr>,
        /// Exclusive end of the lexical block.
        body_end: SourcePosition,
    },
    /// Values from every explicitly enumerated local control-flow path.
    BranchJoin {
        /// Each path's value, including `Unknown` for an unproven alternative.
        alternatives: Vec<ValueExpr>,
    },
    /// Value obtained from a receiver member.
    Field {
        /// Receiver expression.
        receiver: String,
        /// Accessed member name.
        member: String,
    },
    /// Member projected from a statically evaluated value.
    Project {
        /// Source value, such as a declared function call result.
        value: Box<ValueExpr>,
        /// Object pattern member name.
        member: String,
    },
    /// Object value with known members.
    Object {
        /// Members retained by bounded value flow.
        members: Vec<ObjectMember>,
        /// Require the unshadowed JavaScript Object built-in for a frozen literal.
        #[serde(default, skip_serializing_if = "is_false")]
        builtin_guard: bool,
    },
    /// Represents the unknown case.
    Unknown,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
/// Represents object member data.
pub struct ObjectMember {
    /// The name value.
    pub name: String,
    /// The position value.
    pub position: SourcePosition,
    /// Proven declaration name when the public member is an alias.
    #[serde(default, skip_serializing_if = "ObjectMemberTarget::is_same_name")]
    pub target: ObjectMemberTarget,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
/// Closed declaration identity for a projected object member.
pub enum ObjectMemberTarget {
    /// The declaration has the public member name.
    #[default]
    SameName,
    /// The public member aliases this proven declaration.
    Alias {
        /// Actual callable declaration name.
        name: String,
    },
}

impl ObjectMemberTarget {
    fn is_same_name(&self) -> bool {
        matches!(self, Self::SameName)
    }
}

impl ObjectMember {
    /// The declaration name paired with the persisted declaration position.
    pub fn declaration_name(&self) -> &str {
        match &self.target {
            ObjectMemberTarget::SameName => &self.name,
            ObjectMemberTarget::Alias { name } => name,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
/// Represents binding fact data.
pub struct BindingFact {
    /// The name value.
    pub name: String,
    /// The position value.
    pub position: SourcePosition,
    /// The value value.
    pub value: ValueExpr,
    #[serde(default, skip_serializing_if = "is_false")]
    /// Whether conditional applies.
    pub conditional: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
/// A control-flow transfer value kept separately from source assignment facts.
pub struct BranchJoinFact {
    /// The joined local binding name.
    pub name: String,
    /// Position after the joined control-flow statement.
    pub position: SourcePosition,
    /// Values retained for each local control-flow alternative.
    pub value: ValueExpr,
    #[serde(default, skip_serializing_if = "is_false")]
    /// Whether the join is scoped to a conditional parent block.
    pub conditional: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
/// Represents field fact data.
pub struct FieldFact {
    /// The name value.
    pub name: String,
    /// The position value.
    pub position: SourcePosition,
    /// The value value.
    pub value: ValueExpr,
    #[serde(default, skip_serializing_if = "is_false")]
    /// Whether conditional applies.
    pub conditional: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
/// A declared element type for a collection initialized by a literal.
pub struct CollectionElementFact {
    /// The collection binding name.
    pub name: String,
    /// The binding position.
    pub position: SourcePosition,
    /// The declared element type.
    pub element_type: TypeExpr,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
/// Identity of the validated manifest operation that produced an extraction fact.
pub struct ExtractionRuleOrigin {
    /// Manifest filename identity.
    pub pack: String,
    /// Rule identity within the manifest.
    pub rule: String,
}

impl ExtractionRuleOrigin {
    pub(crate) fn new(pack: &str, rule: &str) -> Self {
        Self {
            pack: pack.to_owned(),
            rule: rule.to_owned(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
/// Same-object ownership admitted by a current factory operation.
pub struct ObjectOwnershipFact {
    /// Actual manifest operation identity.
    pub origin: ExtractionRuleOrigin,
    /// Lexical global binding that must remain unshadowed, when configured.
    pub guard: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
/// Finite existing component macro behaviors.
pub enum ComponentMacroRole {
    /// Produces component input fields.
    Props,
    /// Produces a setup callable.
    Emits,
    /// Declares typed slot inputs.
    Slots,
    /// A recognized call with no additional extraction role.
    Marker,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
/// A source-proven component macro invocation in script setup.
pub struct ComponentMacroFact {
    /// Effective operation identity.
    pub origin: ExtractionRuleOrigin,
    /// Existing finite macro behavior.
    pub role: ComponentMacroRole,
    /// Configured lexical callee, independent of its semantic role.
    pub callee: String,
    /// Optional source binding receiving the result.
    pub binding: Option<String>,
    /// Source call occurrence.
    pub position: SourcePosition,
    /// Existing non-object props type argument, when present.
    pub annotation: Option<TypeExpr>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
/// A literal template context passed to an exactly imported render provider.
pub struct TemplateContextFact {
    /// Current rule identity; legacy facts without it cannot establish a provider.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<ExtractionRuleOrigin>,
    /// The unique AST import binding called by this render expression.
    pub import_alias: String,
    /// The literal template target relative to the provider file.
    pub target: String,
    /// Parser-normalized project path for downstream dependency and linker consumers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_path: Option<String>,
    /// Literal dictionary keys available to the rendered template.
    pub keys: Vec<String>,
    /// The position of the source-proven render call.
    pub position: SourcePosition,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
/// Static configuration admitted by one current component environment operation.
pub struct ComponentConfigurationFact {
    /// Manifest operation identity.
    pub origin: ExtractionRuleOrigin,
    /// Whether conventional component discovery remains enabled.
    pub enabled: bool,
    /// Literal configured modules admitted by the operation.
    pub modules: Vec<String>,
    /// Proven default-root callable configuration; absence establishes no authority.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub callable_imports: Option<CallableImportConfiguration>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
/// Finite independent switches for implicit callable discovery.
pub struct CallableImportConfiguration {
    /// Implicit named imports are enabled.
    pub enabled: bool,
    /// Immediate provider discovery is enabled.
    pub scan: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
/// A Python import beneath a syntactic type-checking guard.
pub struct TypeOnlyImportFact {
    /// The lexical name bound by the import.
    pub alias: String,
    /// The import statement position.
    pub position: SourcePosition,
    /// Lexical guard import that requires provider verification.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub guard_import: Option<GuardImportFact>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
/// Import binding whose provider must be verified before annotation admission.
pub struct GuardImportFact {
    /// The lexical name bound by the import.
    pub alias: String,
    /// The import statement position.
    pub position: SourcePosition,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
/// A name read from a Python annotation AST node.
pub struct AnnotationReferenceFact {
    /// The referenced type expression.
    pub expression: String,
    /// The exact source position of that expression.
    pub position: SourcePosition,
    /// AST-proven value namespace query, rather than a type namespace annotation.
    #[serde(default, skip_serializing_if = "is_false")]
    pub value_query: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
/// A typed prop declared by a Vue component slot.
pub struct VueSlotFact {
    /// AST-declared slot name.
    pub slot: String,
    /// AST-declared slot prop binding.
    pub binding: String,
    /// Declaration position in the provider module.
    pub position: SourcePosition,
    /// Type resolved in the provider module's lexical scope.
    pub type_expr: TypeExpr,
}

fn is_false(value: &bool) -> bool {
    !value
}

/// A literal established by extraction, or an observed uncertain value.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum StaticString {
    /// A nonempty bounded literal key or request value.
    Literal {
        /// Source literal.
        #[serde(deserialize_with = "literal_string")]
        value: String,
    },
    /// The expression does not establish a literal.
    Unknown,
}

fn literal_string<'de, D: serde::Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    let value = String::deserialize(d)?;
    if value.is_empty() || value.len() > 256 || value.chars().any(char::is_control) {
        return Err(serde::de::Error::custom("invalid bounded literal"));
    }
    Ok(value)
}

/// Symbol authority retained from an existing declaration or lexical expression.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SymbolOperand {
    /// Existing canonical declaration.
    Node {
        /// Declaration ID.
        id: String,
    },
    /// A name requiring ordinary symbol resolution.
    Name {
        /// Lexical expression.
        expression: String,
    },
    /// An observed operand without a proven symbol.
    Unknown,
}

/// Finite relations produced by symbol attachments.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AttachmentRelation {
    /// Callable uses a model class.
    UsesModel,
    /// Handler references an exception class.
    References,
}

/// A configured callable-to-class attachment.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SymbolAttachmentFact {
    /// Effective rule identity.
    pub origin: ExtractionRuleOrigin,
    /// Source occurrence.
    pub position: SourcePosition,
    /// Real callable or lexical handler operand.
    pub source: SymbolOperand,
    /// Real class operand.
    pub target: SymbolOperand,
    /// Configured finite relation.
    pub relation: AttachmentRelation,
}

/// Members exposed when the uniquely bound factory is called.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FactoryResultFact {
    /// Effective rule identity.
    pub origin: ExtractionRuleOrigin,
    /// Factory call occurrence.
    pub position: SourcePosition,
    /// Existing local factory binding.
    pub binding: String,
    /// Optional effective module required at both provider and consumer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub configured_module: Option<String>,
    /// Actual declaration positions of projected members.
    #[serde(deserialize_with = "bounded_projection_vector")]
    pub members: Vec<ObjectMember>,
}

/// Expected declaration kind for a named registration.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NamedTargetKind {
    /// Existing object owner.
    Object,
    /// Existing named or inline callable.
    Callable,
    /// Existing embedded event scope.
    ListenerScope,
}

/// A definition or reference in a literal-key domain.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum NamedBindingRole {
    /// Registers an existing symbol.
    Definition {
        /// Registered symbol operand.
        target: SymbolOperand,
        /// Required declaration kind.
        target_kind: NamedTargetKind,
    },
    /// Registers a callable through a canonically type-proven registry receiver.
    RegistryDefinition {
        /// Lexical receiver at the registration occurrence.
        receiver: SymbolOperand,
        /// Existing callable declaration operand.
        target: SymbolOperand,
    },
    /// Reads a key in the configured domain.
    Reference,
}

/// Source-proven registration or reference with explicit uncertainty.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct NamedBindingFact {
    /// Effective rule identity.
    pub origin: ExtractionRuleOrigin,
    /// Source occurrence.
    pub position: SourcePosition,
    /// Effective project-scoped key domain.
    pub domain: String,
    /// Known key or observed unknown key.
    pub key: StaticString,
    /// Definition authority or reference.
    pub role: NamedBindingRole,
}

/// Independently retained identity of a canonical definition lost to scope limits.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct NamedDefinitionUncertainty {
    pub(crate) domain: String,
    pub(crate) key: StaticString,
    pub(crate) origin: ExtractionRuleOrigin,
}

impl NamedDefinitionUncertainty {
    pub(crate) fn valid(&self) -> bool {
        projection_identifier(&self.domain)
            && [&self.origin.pack, &self.origin.rule]
                .into_iter()
                .all(|s| !s.is_empty() && s.len() <= 4096 && !s.chars().any(char::is_control))
            && match &self.key {
                StaticString::Unknown => true,
                StaticString::Literal { value } => {
                    !value.is_empty() && value.len() <= 256 && !value.chars().any(char::is_control)
                }
            }
    }
}

/// A directly attached request with independently proven URL and method.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EndpointRequestFact {
    /// Effective rule identity.
    pub origin: ExtractionRuleOrigin,
    /// Source occurrence.
    pub position: SourcePosition,
    /// Local literal URL or explicit uncertainty.
    pub url: StaticString,
    /// Literal normalized method or explicit uncertainty.
    pub method: StaticString,
}

trait ProjectionFact {
    fn valid(&self) -> bool;
}
fn projection_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value.bytes().enumerate().all(|(index, byte)| {
            byte.is_ascii_alphabetic()
                || matches!(byte, b'_' | b'$')
                || (index > 0 && byte.is_ascii_digit())
        })
}
fn projection_origin(origin: &ExtractionRuleOrigin, position: SourcePosition) -> bool {
    !origin.pack.is_empty() && !origin.rule.is_empty() && position.line > 0
}
fn projection_operand(value: &SymbolOperand) -> bool {
    match value {
        SymbolOperand::Node { id } => !id.is_empty(),
        SymbolOperand::Name { expression } => {
            !expression.is_empty() && !expression.chars().any(char::is_control)
        }
        SymbolOperand::Unknown => true,
    }
}
impl ProjectionFact for ObjectMember {
    fn valid(&self) -> bool {
        projection_identifier(&self.name)
            && projection_identifier(self.declaration_name())
            && self.position.line > 0
    }
}
impl ProjectionFact for SymbolAttachmentFact {
    fn valid(&self) -> bool {
        projection_origin(&self.origin, self.position)
            && projection_operand(&self.source)
            && projection_operand(&self.target)
    }
}
impl ProjectionFact for FactoryResultFact {
    fn valid(&self) -> bool {
        let mut names = std::collections::HashSet::new();
        projection_origin(&self.origin, self.position)
            && projection_identifier(&self.binding)
            && self.configured_module.as_ref().is_none_or(|module| {
                !module.is_empty()
                    && module.len() <= 256
                    && !module.chars().any(char::is_whitespace)
            })
            && self.members.len() <= 128
            && self
                .members
                .iter()
                .all(|member| member.valid() && names.insert(&member.name))
    }
}
impl ProjectionFact for NamedBindingFact {
    fn valid(&self) -> bool {
        projection_origin(&self.origin, self.position)
            && projection_identifier(&self.domain)
            && match &self.role {
                NamedBindingRole::Definition { target, .. } => projection_operand(target),
                NamedBindingRole::RegistryDefinition { receiver, target } => {
                    projection_operand(receiver) && projection_operand(target)
                }
                NamedBindingRole::Reference => true,
            }
    }
}
impl ProjectionFact for EndpointRequestFact {
    fn valid(&self) -> bool {
        projection_origin(&self.origin, self.position)
            && match &self.method {
                StaticString::Literal { value } => matches!(
                    value.as_str(),
                    "GET"
                        | "POST"
                        | "PUT"
                        | "PATCH"
                        | "DELETE"
                        | "HEAD"
                        | "OPTIONS"
                        | "TRACE"
                        | "CONNECT"
                ),
                StaticString::Unknown => true,
            }
    }
}

fn bounded_projection_vector<'de, D, T>(d: D) -> Result<Vec<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de> + ProjectionFact,
{
    let values = Vec::<T>::deserialize(d)?;
    if values.len() > 128 {
        return Err(serde::de::Error::custom(
            "projection vector exceeds 128 entries",
        ));
    }
    if values.iter().any(|value| !value.valid()) {
        return Err(serde::de::Error::custom(
            "invalid configured projection fact",
        ));
    }
    Ok(values)
}

#[derive(Clone, Debug, Default, Deserialize, Eq, Serialize)]
#[serde(default, deny_unknown_fields)]
/// Represents value flow facts data.
pub struct ValueFlowFacts {
    /// Optional storage for less common scope and framework facts.
    #[serde(flatten, deserialize_with = "deserialize_framework_facts")]
    pub framework: Option<Box<FrameworkScopeFacts>>,
    /// The bindings value.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub bindings: Vec<BindingFact>,
    /// Control-flow transfer facts, separate from source assignment writes.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub branch_joins: Vec<BranchJoinFact>,
    /// Positions of completed immutable lexical initializers.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub immutable_initializers: Vec<SourcePosition>,
    /// Optional return type value.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub return_type: Option<TypeExpr>,
    /// The fields value.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub fields: Vec<FieldFact>,
}

#[derive(Clone, Debug, Default)]
/// Less common scope facts, stored only when a scope needs them.
pub struct FrameworkScopeFacts {
    /// Rule-proven ownership of a factory-created object.
    pub object_ownership: Option<ObjectOwnershipFact>,
    /// The scope exceeded the semantic fact limit and cannot prove import flow.
    pub fact_limit_exceeded: bool,
    /// Explicit result type exposed only after awaiting an async callable.
    pub async_return_type: Option<TypeExpr>,
    /// Optional return value value.
    pub return_value: Option<ValueExpr>,
    /// Optional return position value.
    pub return_position: Option<SourcePosition>,
    /// Optional alias type value.
    pub alias_type: Option<TypeExpr>,
    /// Tagged storage for infrequent framework and projection facts.
    pub(crate) facts: Vec<FrameworkFact>,
}

/// One infrequent framework or projection fact stored in a scope catalog.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum FrameworkFact {
    /// Callable-to-class attachment from a finite configured operation.
    SymbolAttachment(SymbolAttachmentFact),
    /// Factory binding to existing result member associations.
    FactoryResult(FactoryResultFact),
    /// Project-scoped named definition or reference.
    NamedBinding(NamedBindingFact),
    /// Directly attached URL and method observation.
    EndpointRequest(EndpointRequestFact),
    /// Static component configuration with the rule that admitted it.
    ComponentConfiguration(ComponentConfigurationFact),
    /// Rule that admitted a generated semantic scope or declaration.
    RuleOrigin(ExtractionRuleOrigin),
    /// Source-proven component macro invocation with its rule origin.
    ComponentMacro(ComponentMacroFact),
    /// Element annotation kept separately from literal value provenance.
    CollectionElement(CollectionElementFact),
    /// Typed Vue slot props declared by a module.
    VueSlot(VueSlotFact),
    /// Render context with exact imported providers and literal arguments.
    TemplateContext(TemplateContextFact),
    /// Import under a statically identified TYPE_CHECKING guard.
    TypeOnlyImport(TypeOnlyImportFact),
    /// Exact source position of a reference extracted from a type annotation.
    AnnotationReference(AnnotationReferenceFact),
}

impl FrameworkFact {
    fn symbol_attachment(&self) -> Option<&SymbolAttachmentFact> {
        if let Self::SymbolAttachment(fact) = self {
            Some(fact)
        } else {
            None
        }
    }
    fn symbol_attachment_mut(&mut self) -> Option<&mut SymbolAttachmentFact> {
        if let Self::SymbolAttachment(fact) = self {
            Some(fact)
        } else {
            None
        }
    }
    fn factory_result(&self) -> Option<&FactoryResultFact> {
        if let Self::FactoryResult(fact) = self {
            Some(fact)
        } else {
            None
        }
    }
    fn named_binding(&self) -> Option<&NamedBindingFact> {
        if let Self::NamedBinding(fact) = self {
            Some(fact)
        } else {
            None
        }
    }
    fn named_binding_mut(&mut self) -> Option<&mut NamedBindingFact> {
        if let Self::NamedBinding(fact) = self {
            Some(fact)
        } else {
            None
        }
    }
    fn endpoint_request(&self) -> Option<&EndpointRequestFact> {
        if let Self::EndpointRequest(fact) = self {
            Some(fact)
        } else {
            None
        }
    }
    fn component_configuration(&self) -> Option<&ComponentConfigurationFact> {
        if let Self::ComponentConfiguration(fact) = self {
            Some(fact)
        } else {
            None
        }
    }
    fn rule_origin(&self) -> Option<&ExtractionRuleOrigin> {
        if let Self::RuleOrigin(fact) = self {
            Some(fact)
        } else {
            None
        }
    }
    fn component_macro(&self) -> Option<&ComponentMacroFact> {
        if let Self::ComponentMacro(fact) = self {
            Some(fact)
        } else {
            None
        }
    }
    fn collection_element(&self) -> Option<&CollectionElementFact> {
        if let Self::CollectionElement(fact) = self {
            Some(fact)
        } else {
            None
        }
    }
    fn vue_slot(&self) -> Option<&VueSlotFact> {
        if let Self::VueSlot(fact) = self {
            Some(fact)
        } else {
            None
        }
    }
    fn template_context(&self) -> Option<&TemplateContextFact> {
        if let Self::TemplateContext(fact) = self {
            Some(fact)
        } else {
            None
        }
    }
    fn type_only_import(&self) -> Option<&TypeOnlyImportFact> {
        if let Self::TypeOnlyImport(fact) = self {
            Some(fact)
        } else {
            None
        }
    }
    fn annotation_reference(&self) -> Option<&AnnotationReferenceFact> {
        if let Self::AnnotationReference(fact) = self {
            Some(fact)
        } else {
            None
        }
    }
}

#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct FrameworkScopeFactsInput {
    object_ownership: Option<ObjectOwnershipFact>,
    fact_limit_exceeded: bool,
    async_return_type: Option<TypeExpr>,
    return_value: Option<ValueExpr>,
    return_position: Option<SourcePosition>,
    alias_type: Option<TypeExpr>,
    #[serde(deserialize_with = "bounded_projection_vector")]
    symbol_attachments: Vec<SymbolAttachmentFact>,
    #[serde(deserialize_with = "bounded_projection_vector")]
    factory_results: Vec<FactoryResultFact>,
    #[serde(deserialize_with = "bounded_projection_vector")]
    named_bindings: Vec<NamedBindingFact>,
    #[serde(deserialize_with = "bounded_projection_vector")]
    endpoint_requests: Vec<EndpointRequestFact>,
    component_configurations: Vec<ComponentConfigurationFact>,
    rule_origins: Vec<ExtractionRuleOrigin>,
    component_macros: Vec<ComponentMacroFact>,
    collection_elements: Vec<CollectionElementFact>,
    vue_slots: Vec<VueSlotFact>,
    template_contexts: Vec<TemplateContextFact>,
    type_only_imports: Vec<TypeOnlyImportFact>,
    annotation_references: Vec<AnnotationReferenceFact>,
}

impl<'de> Deserialize<'de> for FrameworkScopeFacts {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let input = FrameworkScopeFactsInput::deserialize(deserializer)?;
        let fact_count = input
            .symbol_attachments
            .len()
            .saturating_add(input.factory_results.len())
            .saturating_add(input.named_bindings.len())
            .saturating_add(input.endpoint_requests.len())
            .saturating_add(input.component_configurations.len())
            .saturating_add(input.rule_origins.len())
            .saturating_add(input.component_macros.len())
            .saturating_add(input.collection_elements.len())
            .saturating_add(input.vue_slots.len())
            .saturating_add(input.template_contexts.len())
            .saturating_add(input.type_only_imports.len())
            .saturating_add(input.annotation_references.len());
        let mut facts = Vec::with_capacity(fact_count);
        facts.extend(
            input
                .symbol_attachments
                .into_iter()
                .map(FrameworkFact::SymbolAttachment),
        );
        facts.extend(
            input
                .factory_results
                .into_iter()
                .map(FrameworkFact::FactoryResult),
        );
        facts.extend(
            input
                .named_bindings
                .into_iter()
                .map(FrameworkFact::NamedBinding),
        );
        facts.extend(
            input
                .endpoint_requests
                .into_iter()
                .map(FrameworkFact::EndpointRequest),
        );
        facts.extend(
            input
                .component_configurations
                .into_iter()
                .map(FrameworkFact::ComponentConfiguration),
        );
        facts.extend(
            input
                .rule_origins
                .into_iter()
                .map(FrameworkFact::RuleOrigin),
        );
        facts.extend(
            input
                .component_macros
                .into_iter()
                .map(FrameworkFact::ComponentMacro),
        );
        facts.extend(
            input
                .collection_elements
                .into_iter()
                .map(FrameworkFact::CollectionElement),
        );
        facts.extend(input.vue_slots.into_iter().map(FrameworkFact::VueSlot));
        facts.extend(
            input
                .template_contexts
                .into_iter()
                .map(FrameworkFact::TemplateContext),
        );
        facts.extend(
            input
                .type_only_imports
                .into_iter()
                .map(FrameworkFact::TypeOnlyImport),
        );
        facts.extend(
            input
                .annotation_references
                .into_iter()
                .map(FrameworkFact::AnnotationReference),
        );
        Ok(Self {
            object_ownership: input.object_ownership,
            fact_limit_exceeded: input.fact_limit_exceeded,
            async_return_type: input.async_return_type,
            return_value: input.return_value,
            return_position: input.return_position,
            alias_type: input.alias_type,
            facts,
        })
    }
}

struct FrameworkFactList<'a, T> {
    facts: &'a [FrameworkFact],
    project: for<'fact> fn(&'fact FrameworkFact) -> Option<&'fact T>,
    count: usize,
}

impl<'a, T> FrameworkFactList<'a, T> {
    fn new(
        facts: &'a [FrameworkFact],
        project: for<'fact> fn(&'fact FrameworkFact) -> Option<&'fact T>,
    ) -> Self {
        let count = facts.iter().filter_map(project).count();
        Self {
            facts,
            project,
            count,
        }
    }

    fn is_empty(&self) -> bool {
        self.count == 0
    }
}

impl<T: Serialize> Serialize for FrameworkFactList<'_, T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut sequence = serializer.serialize_seq(Some(self.count))?;
        for fact in self.facts {
            if let Some(value) = (self.project)(fact) {
                sequence.serialize_element(value)?;
            }
        }
        sequence.end()
    }
}

#[derive(Serialize)]
pub(crate) struct FrameworkFactCollectionsDto<'a> {
    #[serde(skip_serializing_if = "FrameworkFactList::is_empty")]
    symbol_attachments: FrameworkFactList<'a, SymbolAttachmentFact>,
    #[serde(skip_serializing_if = "FrameworkFactList::is_empty")]
    factory_results: FrameworkFactList<'a, FactoryResultFact>,
    #[serde(skip_serializing_if = "FrameworkFactList::is_empty")]
    named_bindings: FrameworkFactList<'a, NamedBindingFact>,
    #[serde(skip_serializing_if = "FrameworkFactList::is_empty")]
    endpoint_requests: FrameworkFactList<'a, EndpointRequestFact>,
    #[serde(skip_serializing_if = "FrameworkFactList::is_empty")]
    component_configurations: FrameworkFactList<'a, ComponentConfigurationFact>,
    #[serde(skip_serializing_if = "FrameworkFactList::is_empty")]
    rule_origins: FrameworkFactList<'a, ExtractionRuleOrigin>,
    #[serde(skip_serializing_if = "FrameworkFactList::is_empty")]
    component_macros: FrameworkFactList<'a, ComponentMacroFact>,
    #[serde(skip_serializing_if = "FrameworkFactList::is_empty")]
    collection_elements: FrameworkFactList<'a, CollectionElementFact>,
    #[serde(skip_serializing_if = "FrameworkFactList::is_empty")]
    vue_slots: FrameworkFactList<'a, VueSlotFact>,
    #[serde(skip_serializing_if = "FrameworkFactList::is_empty")]
    template_contexts: FrameworkFactList<'a, TemplateContextFact>,
    #[serde(skip_serializing_if = "FrameworkFactList::is_empty")]
    type_only_imports: FrameworkFactList<'a, TypeOnlyImportFact>,
    #[serde(skip_serializing_if = "FrameworkFactList::is_empty")]
    annotation_references: FrameworkFactList<'a, AnnotationReferenceFact>,
}

impl<'a> From<&'a FrameworkScopeFacts> for FrameworkFactCollectionsDto<'a> {
    fn from(facts: &'a FrameworkScopeFacts) -> Self {
        let tagged = &facts.facts;
        Self {
            symbol_attachments: FrameworkFactList::new(tagged, FrameworkFact::symbol_attachment),
            factory_results: FrameworkFactList::new(tagged, FrameworkFact::factory_result),
            named_bindings: FrameworkFactList::new(tagged, FrameworkFact::named_binding),
            endpoint_requests: FrameworkFactList::new(tagged, FrameworkFact::endpoint_request),
            component_configurations: FrameworkFactList::new(
                tagged,
                FrameworkFact::component_configuration,
            ),
            rule_origins: FrameworkFactList::new(tagged, FrameworkFact::rule_origin),
            component_macros: FrameworkFactList::new(tagged, FrameworkFact::component_macro),
            collection_elements: FrameworkFactList::new(tagged, FrameworkFact::collection_element),
            vue_slots: FrameworkFactList::new(tagged, FrameworkFact::vue_slot),
            template_contexts: FrameworkFactList::new(tagged, FrameworkFact::template_context),
            type_only_imports: FrameworkFactList::new(tagged, FrameworkFact::type_only_import),
            annotation_references: FrameworkFactList::new(
                tagged,
                FrameworkFact::annotation_reference,
            ),
        }
    }
}

#[derive(Serialize)]
struct FrameworkScopeFactsDto<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    object_ownership: Option<&'a ObjectOwnershipFact>,
    #[serde(skip_serializing_if = "Option::is_none")]
    fact_limit_exceeded: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    async_return_type: Option<&'a TypeExpr>,
    #[serde(skip_serializing_if = "Option::is_none")]
    return_value: Option<&'a ValueExpr>,
    #[serde(skip_serializing_if = "Option::is_none")]
    return_position: Option<&'a SourcePosition>,
    #[serde(skip_serializing_if = "Option::is_none")]
    alias_type: Option<&'a TypeExpr>,
    #[serde(flatten)]
    facts: FrameworkFactCollectionsDto<'a>,
}

impl<'a> From<&'a FrameworkScopeFacts> for FrameworkScopeFactsDto<'a> {
    fn from(facts: &'a FrameworkScopeFacts) -> Self {
        Self {
            object_ownership: facts.object_ownership.as_ref(),
            fact_limit_exceeded: facts.fact_limit_exceeded.then_some(true),
            async_return_type: facts.async_return_type.as_ref(),
            return_value: facts.return_value.as_ref(),
            return_position: facts.return_position.as_ref(),
            alias_type: facts.alias_type.as_ref(),
            facts: FrameworkFactCollectionsDto::from(facts),
        }
    }
}

impl Serialize for FrameworkScopeFacts {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        FrameworkScopeFactsDto::from(self).serialize(serializer)
    }
}

impl FrameworkScopeFacts {
    pub(crate) fn is_empty(&self) -> bool {
        self.object_ownership.is_none()
            && !self.fact_limit_exceeded
            && self.async_return_type.is_none()
            && self.return_value.is_none()
            && self.return_position.is_none()
            && self.alias_type.is_none()
            && self.facts.is_empty()
    }

    fn offset_fact_positions(&mut self, offset: usize) {
        let offset_position = |position: &mut SourcePosition| {
            position.line = position.line.saturating_add(offset);
        };
        for fact in &mut self.facts {
            match fact {
                FrameworkFact::SymbolAttachment(fact) => offset_position(&mut fact.position),
                FrameworkFact::FactoryResult(fact) => {
                    offset_position(&mut fact.position);
                    for member in &mut fact.members {
                        offset_position(&mut member.position);
                    }
                }
                FrameworkFact::NamedBinding(fact) => offset_position(&mut fact.position),
                FrameworkFact::EndpointRequest(fact) => offset_position(&mut fact.position),
                FrameworkFact::ComponentMacro(fact) => offset_position(&mut fact.position),
                FrameworkFact::CollectionElement(fact) => offset_position(&mut fact.position),
                FrameworkFact::VueSlot(fact) => offset_position(&mut fact.position),
                FrameworkFact::TemplateContext(fact) => offset_position(&mut fact.position),
                FrameworkFact::TypeOnlyImport(fact) => offset_position(&mut fact.position),
                FrameworkFact::AnnotationReference(fact) => offset_position(&mut fact.position),
                FrameworkFact::ComponentConfiguration(_) | FrameworkFact::RuleOrigin(_) => {}
            }
        }
    }

    pub(crate) fn symbol_attachments(&self) -> impl Iterator<Item = &SymbolAttachmentFact> + Clone {
        self.facts
            .iter()
            .filter_map(FrameworkFact::symbol_attachment)
    }
    pub(crate) fn symbol_attachments_mut(
        &mut self,
    ) -> impl Iterator<Item = &mut SymbolAttachmentFact> {
        self.facts
            .iter_mut()
            .filter_map(FrameworkFact::symbol_attachment_mut)
    }
    pub(crate) fn push_symbol_attachment(&mut self, fact: SymbolAttachmentFact) {
        self.facts.push(FrameworkFact::SymbolAttachment(fact));
    }

    pub(crate) fn factory_results(&self) -> impl Iterator<Item = &FactoryResultFact> + Clone {
        self.facts.iter().filter_map(FrameworkFact::factory_result)
    }
    pub(crate) fn push_factory_result(&mut self, fact: FactoryResultFact) {
        self.facts.push(FrameworkFact::FactoryResult(fact));
    }

    pub(crate) fn named_bindings(&self) -> impl Iterator<Item = &NamedBindingFact> + Clone {
        self.facts.iter().filter_map(FrameworkFact::named_binding)
    }
    pub(crate) fn named_bindings_mut(&mut self) -> impl Iterator<Item = &mut NamedBindingFact> {
        self.facts
            .iter_mut()
            .filter_map(FrameworkFact::named_binding_mut)
    }
    pub(crate) fn push_named_binding(&mut self, fact: NamedBindingFact) {
        self.facts.push(FrameworkFact::NamedBinding(fact));
    }

    pub(crate) fn endpoint_requests(&self) -> impl Iterator<Item = &EndpointRequestFact> + Clone {
        self.facts
            .iter()
            .filter_map(FrameworkFact::endpoint_request)
    }
    pub(crate) fn push_endpoint_request(&mut self, fact: EndpointRequestFact) {
        self.facts.push(FrameworkFact::EndpointRequest(fact));
    }

    pub(crate) fn component_configurations(
        &self,
    ) -> impl Iterator<Item = &ComponentConfigurationFact> + Clone {
        self.facts
            .iter()
            .filter_map(FrameworkFact::component_configuration)
    }
    pub(crate) fn push_component_configuration(&mut self, fact: ComponentConfigurationFact) {
        self.facts.push(FrameworkFact::ComponentConfiguration(fact));
    }

    pub(crate) fn rule_origins(&self) -> impl Iterator<Item = &ExtractionRuleOrigin> + Clone {
        self.facts.iter().filter_map(FrameworkFact::rule_origin)
    }
    pub(crate) fn push_rule_origin(&mut self, fact: ExtractionRuleOrigin) {
        self.facts.push(FrameworkFact::RuleOrigin(fact));
    }

    pub(crate) fn component_macros(&self) -> impl Iterator<Item = &ComponentMacroFact> + Clone {
        self.facts.iter().filter_map(FrameworkFact::component_macro)
    }
    pub(crate) fn push_component_macro(&mut self, fact: ComponentMacroFact) {
        self.facts.push(FrameworkFact::ComponentMacro(fact));
    }

    pub(crate) fn collection_elements(
        &self,
    ) -> impl Iterator<Item = &CollectionElementFact> + Clone {
        self.facts
            .iter()
            .filter_map(FrameworkFact::collection_element)
    }
    pub(crate) fn push_collection_element(&mut self, fact: CollectionElementFact) {
        self.facts.push(FrameworkFact::CollectionElement(fact));
    }

    pub(crate) fn vue_slots(&self) -> impl Iterator<Item = &VueSlotFact> + Clone {
        self.facts.iter().filter_map(FrameworkFact::vue_slot)
    }
    pub(crate) fn push_vue_slot(&mut self, fact: VueSlotFact) {
        self.facts.push(FrameworkFact::VueSlot(fact));
    }

    pub(crate) fn template_contexts(&self) -> impl Iterator<Item = &TemplateContextFact> + Clone {
        self.facts
            .iter()
            .filter_map(FrameworkFact::template_context)
    }
    pub(crate) fn template_contexts_mut(
        &mut self,
    ) -> impl Iterator<Item = &mut TemplateContextFact> {
        self.facts.iter_mut().filter_map(|fact| {
            if let FrameworkFact::TemplateContext(value) = fact {
                Some(value)
            } else {
                None
            }
        })
    }
    pub(crate) fn push_template_context(&mut self, fact: TemplateContextFact) {
        self.facts.push(FrameworkFact::TemplateContext(fact));
    }

    pub(crate) fn type_only_imports(&self) -> impl Iterator<Item = &TypeOnlyImportFact> + Clone {
        self.facts
            .iter()
            .filter_map(FrameworkFact::type_only_import)
    }
    pub(crate) fn push_type_only_import(&mut self, fact: TypeOnlyImportFact) {
        self.facts.push(FrameworkFact::TypeOnlyImport(fact));
    }

    pub(crate) fn annotation_references(
        &self,
    ) -> impl Iterator<Item = &AnnotationReferenceFact> + Clone {
        self.facts
            .iter()
            .filter_map(FrameworkFact::annotation_reference)
    }
    pub(crate) fn push_annotation_reference(&mut self, fact: AnnotationReferenceFact) {
        self.facts.push(FrameworkFact::AnnotationReference(fact));
    }
}

impl PartialEq for FrameworkScopeFacts {
    fn eq(&self, other: &Self) -> bool {
        self.object_ownership == other.object_ownership
            && self.fact_limit_exceeded == other.fact_limit_exceeded
            && self.async_return_type == other.async_return_type
            && self.return_value == other.return_value
            && self.return_position == other.return_position
            && self.alias_type == other.alias_type
            && self.symbol_attachments().eq(other.symbol_attachments())
            && self.factory_results().eq(other.factory_results())
            && self.named_bindings().eq(other.named_bindings())
            && self.endpoint_requests().eq(other.endpoint_requests())
            && self
                .component_configurations()
                .eq(other.component_configurations())
            && self.rule_origins().eq(other.rule_origins())
            && self.component_macros().eq(other.component_macros())
            && self.collection_elements().eq(other.collection_elements())
            && self.vue_slots().eq(other.vue_slots())
            && self.template_contexts().eq(other.template_contexts())
            && self.type_only_imports().eq(other.type_only_imports())
            && self
                .annotation_references()
                .eq(other.annotation_references())
    }
}

impl Eq for FrameworkScopeFacts {}
impl ValueFlowFacts {
    pub(crate) fn valid_for_cache(&self) -> bool {
        if super::typed_facts::scope_fact_count(self) > DEFAULT_FACTS_PER_SCOPE {
            return false;
        }
        let values_are_valid = self
            .bindings
            .iter()
            .map(|fact| &fact.value)
            .chain(self.fields.iter().map(|fact| &fact.value))
            .chain(self.branch_joins.iter().map(|fact| &fact.value))
            .chain(self.return_value.iter())
            .all(|value| value.valid_call_sites(0));
        if !values_are_valid {
            return false;
        }
        self.framework.as_deref().is_none_or(|framework| {
            !framework.fact_limit_exceeded
                && framework.symbol_attachments().count() <= 128
                && framework.factory_results().count() <= 128
                && framework.named_bindings().count() <= 128
                && framework.endpoint_requests().count() <= 128
                && framework.symbol_attachments().all(ProjectionFact::valid)
                && framework.factory_results().all(ProjectionFact::valid)
                && framework.named_bindings().all(ProjectionFact::valid)
                && framework.endpoint_requests().all(ProjectionFact::valid)
                && framework
                    .factory_results()
                    .all(|fact| fact.members.len() <= 128)
        })
    }
}

fn deserialize_framework_facts<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<Box<FrameworkScopeFacts>>, D::Error> {
    let facts = FrameworkScopeFacts::deserialize(deserializer)?;
    Ok((!facts.is_empty()).then(|| Box::new(facts)))
}

impl std::ops::Deref for ValueFlowFacts {
    type Target = FrameworkScopeFacts;

    fn deref(&self) -> &Self::Target {
        static EMPTY: FrameworkScopeFacts = FrameworkScopeFacts {
            object_ownership: None,
            fact_limit_exceeded: false,
            async_return_type: None,
            return_value: None,
            return_position: None,
            alias_type: None,
            facts: Vec::new(),
        };
        self.framework.as_deref().unwrap_or(&EMPTY)
    }
}

impl std::ops::DerefMut for ValueFlowFacts {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.framework.get_or_insert_with(Default::default)
    }
}

impl PartialEq for ValueFlowFacts {
    fn eq(&self, other: &Self) -> bool {
        std::ops::Deref::deref(self) == std::ops::Deref::deref(other)
            && self.bindings == other.bindings
            && self.branch_joins == other.branch_joins
            && self.immutable_initializers == other.immutable_initializers
            && self.return_type == other.return_type
            && self.fields == other.fields
    }
}

impl ValueFlowFacts {
    pub(crate) fn deserialize_for_scope(
        encoded: &serde_json::Value,
        classic_global: bool,
    ) -> Result<Self, serde_json::Error> {
        if classic_global {
            if let Some(object) = encoded.as_object() {
                let entries = object
                    .iter()
                    .filter(|(key, value)| {
                        !(matches!(key.as_str(), "bindings" | "fields") && value.is_null())
                    })
                    .map(|(key, value)| (key.as_str(), value));
                return Self::deserialize(serde::de::value::MapDeserializer::new(entries));
            }
        }
        Self::deserialize(encoded)
    }

    /// Performs reference keys.
    pub fn reference_keys<'a>(&'a self, mut visit: impl FnMut(&'a str)) {
        fn type_keys<'a>(ty: &'a TypeExpr, visit: &mut impl FnMut(&'a str), depth: usize) {
            if depth >= 8 {
                return;
            }
            match ty {
                TypeExpr::Named { name } => visit(name),
                TypeExpr::Applied { base, args } => {
                    if !matches!(base.as_str(), "&" | "&mut") {
                        visit(base);
                    }
                    for argument in args.iter().take(16) {
                        type_keys(argument, visit, depth + 1);
                    }
                }
                TypeExpr::Unknown => {}
            }
        }
        fn value_keys<'a>(value: &'a ValueExpr, visit: &mut impl FnMut(&'a str), depth: usize) {
            if depth >= 8 {
                return;
            }
            match value {
                ValueExpr::Annotated { type_expr } => type_keys(type_expr, visit, 0),
                ValueExpr::Construct { callee } => {
                    if python_literal_type(callee).is_none() {
                        visit(callee);
                    }
                }
                ValueExpr::Call { callee, site } => {
                    visit(callee);
                    if let Some(receiver) = site.as_ref().and_then(|site| site.receiver.as_deref())
                    {
                        value_keys(receiver, visit, depth + 1);
                    }
                }
                ValueExpr::Await { value } => value_keys(value, visit, depth + 1),
                ValueExpr::LoopElement { iterable, .. } => visit(iterable),
                ValueExpr::Scoped { value, .. } => value_keys(value, visit, depth + 1),
                ValueExpr::BranchJoin { alternatives } => {
                    for alternative in alternatives {
                        value_keys(alternative, visit, depth + 1);
                    }
                }
                ValueExpr::Alias { name } => visit(name),
                ValueExpr::Field { receiver, member } => {
                    visit(receiver);
                    visit(member);
                }
                ValueExpr::Project { value, member } => {
                    value_keys(value, visit, depth + 1);
                    visit(member);
                }
                ValueExpr::Object { .. } | ValueExpr::Unknown => {}
            }
        }
        for ty in self
            .return_type
            .iter()
            .chain(self.async_return_type.iter())
            .chain(self.alias_type.iter())
        {
            type_keys(ty, &mut visit, 0);
        }
        for fact in self.collection_elements() {
            type_keys(&fact.element_type, &mut visit, 0);
        }
        for fact in self.vue_slots() {
            type_keys(&fact.type_expr, &mut visit, 0);
        }
        for value in self
            .return_value
            .iter()
            .chain(self.bindings.iter().map(|fact| &fact.value))
            .chain(self.branch_joins.iter().map(|fact| &fact.value))
            .chain(self.fields.iter().map(|fact| &fact.value))
        {
            value_keys(value, &mut visit, 0);
        }
        let mut operand = |value: &'a SymbolOperand| match value {
            SymbolOperand::Node { id } => visit(id),
            SymbolOperand::Name { expression } => visit(expression),
            SymbolOperand::Unknown => {}
        };
        for fact in self.symbol_attachments() {
            operand(&fact.source);
            operand(&fact.target);
        }
        for fact in self.named_bindings() {
            match &fact.role {
                NamedBindingRole::Definition { target, .. } => operand(target),
                NamedBindingRole::RegistryDefinition { receiver, target } => {
                    operand(receiver);
                    operand(target);
                }
                NamedBindingRole::Reference => {}
            }
        }
        for fact in self.factory_results() {
            visit(&fact.binding);
            for member in &fact.members {
                visit(&member.name);
                visit(member.declaration_name());
            }
        }
        for fact in self.named_bindings() {
            visit(&fact.domain);
            if let StaticString::Literal { value } = &fact.key {
                visit(value);
            }
        }
        for fact in self.endpoint_requests() {
            for value in [&fact.url, &fact.method] {
                if let StaticString::Literal { value } = value {
                    visit(value);
                }
            }
        }
    }

    /// Performs reference keys from details.
    pub fn reference_keys_from_details<'a>(
        details: &'a serde_json::Value,
        visit: impl FnMut(&'a str),
    ) {
        if let Some(flow) = details.get("value_flow") {
            Self::reference_keys_from_value(flow, visit);
        }
    }

    pub(crate) fn reference_keys_from_value<'a>(
        flow: &'a serde_json::Value,
        mut visit: impl FnMut(&'a str),
    ) {
        fn type_key<'a>(value: &'a serde_json::Value, visit: &mut impl FnMut(&'a str)) {
            fn walk<'a>(
                value: &'a serde_json::Value,
                visit: &mut impl FnMut(&'a str),
                depth: usize,
            ) {
                if depth >= 8 {
                    return;
                }
                match value.get("kind").and_then(serde_json::Value::as_str) {
                    Some("named") => {
                        if let Some(name) = value.get("name").and_then(serde_json::Value::as_str) {
                            visit(name);
                        }
                    }
                    Some("applied") => {
                        if let Some(base) = value.get("base").and_then(serde_json::Value::as_str) {
                            visit(base);
                        }
                        if let Some(args) = value.get("args").and_then(serde_json::Value::as_array)
                        {
                            for argument in args.iter().take(16) {
                                walk(argument, visit, depth + 1);
                            }
                        }
                    }
                    _ => {}
                }
            }
            walk(value, visit, 0);
        }
        fn value_keys<'a>(
            value: &'a serde_json::Value,
            visit: &mut impl FnMut(&'a str),
            depth: usize,
        ) {
            if depth >= 8 {
                return;
            }
            if matches!(
                value.get("kind").and_then(serde_json::Value::as_str),
                Some("scoped" | "await")
            ) {
                if let Some(inner) = value.get("value") {
                    value_keys(inner, visit, depth + 1);
                }
                return;
            }
            if value.get("kind").and_then(serde_json::Value::as_str) == Some("branch_join") {
                if let Some(alternatives) = value
                    .get("alternatives")
                    .and_then(serde_json::Value::as_array)
                {
                    for alternative in alternatives {
                        value_keys(alternative, visit, depth + 1);
                    }
                }
                return;
            }
            let fields: &[&str] = match value.get("kind").and_then(serde_json::Value::as_str) {
                Some("annotated") => {
                    if let Some(ty) = value.get("type_expr") {
                        type_key(ty, visit);
                    }
                    &[]
                }
                Some("construct")
                    if value
                        .get("callee")
                        .and_then(serde_json::Value::as_str)
                        .is_some_and(|callee| python_literal_type(callee).is_some()) =>
                {
                    &[]
                }
                Some("construct" | "call") => &["callee"],
                Some("alias") => &["name"],
                Some("loop_element") => &["iterable"],
                Some("field") => &["receiver", "member"],
                _ => &[],
            };
            for field in fields {
                if let Some(name) = value.get(field).and_then(serde_json::Value::as_str) {
                    visit(name);
                }
            }
        }
        for key in ["return_type", "async_return_type", "alias_type"] {
            if let Some(ty) = flow.get(key) {
                type_key(ty, &mut visit);
            }
        }
        if let Some(facts) = flow
            .get("collection_elements")
            .and_then(serde_json::Value::as_array)
        {
            for fact in facts {
                if let Some(ty) = fact.get("element_type") {
                    type_key(ty, &mut visit);
                }
            }
        }
        if let Some(facts) = flow.get("vue_slots").and_then(serde_json::Value::as_array) {
            for fact in facts {
                if let Some(ty) = fact.get("type_expr") {
                    type_key(ty, &mut visit);
                }
            }
        }
        if let Some(value) = flow.get("return_value") {
            value_keys(value, &mut visit, 0);
        }
        for key in ["bindings", "branch_joins", "fields"] {
            if let Some(facts) = flow.get(key).and_then(serde_json::Value::as_array) {
                for fact in facts {
                    if let Some(value) = fact.get("value") {
                        value_keys(value, &mut visit, 0);
                    }
                }
            }
        }
        for key in [
            "symbol_attachments",
            "named_bindings",
            "factory_results",
            "endpoint_requests",
        ] {
            for fact in flow
                .get(key)
                .and_then(serde_json::Value::as_array)
                .into_iter()
                .flatten()
            {
                for operand in [
                    fact.get("source"),
                    fact.get("target"),
                    fact.get("role").and_then(|role| role.get("target")),
                    fact.get("role").and_then(|role| role.get("receiver")),
                ]
                .into_iter()
                .flatten()
                {
                    for field in ["id", "expression"] {
                        if let Some(value) = operand.get(field).and_then(serde_json::Value::as_str)
                        {
                            visit(value);
                        }
                    }
                }
                for field in ["binding", "domain"] {
                    if let Some(value) = fact.get(field).and_then(serde_json::Value::as_str) {
                        visit(value);
                    }
                }
                for member in fact
                    .get("members")
                    .and_then(serde_json::Value::as_array)
                    .into_iter()
                    .flatten()
                {
                    if let Some(name) = member.get("name").and_then(serde_json::Value::as_str) {
                        visit(name);
                    }
                    if let Some(name) = member
                        .get("target")
                        .and_then(|target| target.get("name"))
                        .and_then(serde_json::Value::as_str)
                    {
                        visit(name);
                    }
                }
                for field in ["key", "url", "method"] {
                    if let Some(value) = fact
                        .get(field)
                        .filter(|value| value["kind"] == "literal")
                        .and_then(|value| value.get("value"))
                        .and_then(serde_json::Value::as_str)
                    {
                        visit(value);
                    }
                }
            }
        }
    }

    /// Reports whether empty applies.
    pub fn is_empty(&self) -> bool {
        self.framework
            .as_deref()
            .is_none_or(FrameworkScopeFacts::is_empty)
            && self.bindings.is_empty()
            && self.branch_joins.is_empty()
            && self.immutable_initializers.is_empty()
            && self.return_type.is_none()
            && self.fields.is_empty()
    }

    /// Performs offset lines.
    pub fn offset_lines(&mut self, offset: usize) {
        if let Some(framework) = self.framework.as_mut() {
            framework.offset_fact_positions(offset);
            if let Some(position) = &mut framework.return_position {
                position.line = position.line.saturating_add(offset);
            }
        }
        for position in self
            .bindings
            .iter_mut()
            .map(|fact| &mut fact.position)
            .chain(self.branch_joins.iter_mut().map(|fact| &mut fact.position))
            .chain(self.immutable_initializers.iter_mut())
            .chain(self.fields.iter_mut().map(|fact| &mut fact.position))
        {
            position.line = position.line.saturating_add(offset);
        }
        fn offset_members(value: &mut ValueExpr, offset: usize) {
            match value {
                ValueExpr::Call {
                    site: Some(site), ..
                } => {
                    site.position.line = site.position.line.saturating_add(offset);
                    if let Some(receiver) = site.receiver.as_deref_mut() {
                        offset_members(receiver, offset);
                    }
                }
                ValueExpr::Object { members, .. } => {
                    for member in members {
                        member.position.line = member.position.line.saturating_add(offset);
                    }
                }
                ValueExpr::Scoped { value, .. } | ValueExpr::Project { value, .. } => {
                    offset_members(value, offset);
                }
                ValueExpr::BranchJoin { alternatives } => {
                    for alternative in alternatives {
                        offset_members(alternative, offset);
                    }
                }
                _ => {}
            }
        }
        for value in self
            .bindings
            .iter_mut()
            .map(|fact| &mut fact.value)
            .chain(self.branch_joins.iter_mut().map(|fact| &mut fact.value))
            .chain(self.fields.iter_mut().map(|fact| &mut fact.value))
        {
            offset_members(value, offset);
        }
        if let Some(value) = self
            .framework
            .as_mut()
            .and_then(|framework| framework.return_value.as_mut())
        {
            offset_members(value, offset);
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
/// Represents export binding data.
pub struct ExportBinding {
    /// The name value.
    pub name: String,
    /// Optional local value.
    pub local: Option<String>,
    /// Optional module value.
    pub module: Option<String>,
    #[serde(default)]
    /// Whether type only applies.
    pub type_only: bool,
    #[serde(default)]
    /// Whether star applies.
    pub star: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
/// TypeScript / CommonJS import and require binding fact.
pub struct TypeScriptBinding {
    /// Alias bound in the current scope.
    pub alias: String,
    /// Required or imported module specifier.
    pub module: String,
    /// Source position where declaration starts.
    pub declaration: SourcePosition,
    /// Source position where declaration completes.
    pub completed: SourcePosition,
    /// Whether the binding is invalidated by subsequent reassignments.
    #[serde(default)]
    pub invalidated: bool,
    /// Whether capture was proven safe without mutations.
    #[serde(default)]
    pub captured_safe: bool,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
/// Module-level TypeScript / CommonJS bindings catalog.
pub struct TypeScriptBindings {
    /// List of require and import bindings.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bindings: Vec<TypeScriptBinding>,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
/// Language-neutral receiver catalog mapping type names and expressions to receiver hints and target types.
pub struct ReceiverCatalog {
    /// Statically known receiver hints indexed by canonical type or expression.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub receivers: std::collections::BTreeMap<String, String>,
}

impl ReceiverCatalog {
    /// Creates an empty receiver catalog.
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a receiver hint for a canonical type or expression.
    pub fn register(&mut self, type_name: impl Into<String>, receiver_hint: impl Into<String>) {
        self.receivers
            .insert(type_name.into(), receiver_hint.into());
    }

    /// Looks up a receiver hint for a type or expression.
    pub fn lookup(&self, type_name: &str) -> Option<&str> {
        self.receivers.get(type_name).map(String::as_str)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dependency_keys_borrow_only_proven_semantic_expressions() {
        let details = serde_json::json!({"value_flow": {
            "return_type": {"kind":"named", "name":"pkg.Worker"},
            "alias_type": {"kind":"named", "name":"AliasTarget"},
            "bindings": [
                {"value":{"kind":"construct", "callee":"pkg.Factory"}},
                {"value":{"kind":"field", "receiver":"client", "member":"run"}},
                {"value":{"kind":"unknown", "callee":"invented"}}
            ],
            "branch_joins": [{"value":{"kind":"branch_join", "alternatives":[
                {"kind":"construct", "callee":"JoinTarget"},
                {"kind":"unknown"}
            ]}}],
            "fields": [{"value":{"kind":"annotated", "type_expr":{"kind":"named", "name":"FieldType"}}}]
        }});
        let mut keys = std::collections::BTreeSet::new();
        ValueFlowFacts::reference_keys_from_details(&details, |key| {
            keys.insert(key);
        });
        assert_eq!(
            keys,
            [
                "AliasTarget",
                "FieldType",
                "JoinTarget",
                "client",
                "pkg.Factory",
                "pkg.Worker",
                "run"
            ]
            .into_iter()
            .collect()
        );
    }

    #[test]
    fn semantic_facts_round_trip_and_default_optional_sections() {
        use crate::core::typed_facts::{decode_facts, encode_facts, TypedFacts};
        use serde_json::json;
        let hot_layout = std::mem::size_of::<ValueFlowFacts>();
        assert!(
            hot_layout <= 160,
            "ValueFlowFacts hot layout: {hot_layout} bytes"
        );
        eprintln!("ValueFlowFacts hot layout: {hot_layout} bytes");
        let origin = json!({"pack":"proof", "rule":"projection"});
        let position = json!({"line":2,"column":3});
        let rows = [
            (
                "symbol_attachments",
                json!({"origin":origin,"position":position,"source":{"kind":"node","id":"callback"},"target":{"kind":"name","expression":"Model"},"relation":"uses_model"}),
            ),
            (
                "factory_results",
                json!({"origin":origin,"position":position,"binding":"useCart","members":[{"name":"checkout","position":position}]}),
            ),
            (
                "named_bindings",
                json!({"origin":origin,"position":position,"domain":"event","key":{"kind":"literal","value":"opened"},"role":{"kind":"reference"}}),
            ),
            (
                "endpoint_requests",
                json!({"origin":origin,"position":position,"url":{"kind":"unknown"},"method":{"kind":"unknown"}}),
            ),
        ];
        for (vector, fact) in rows {
            let encoded = json!({vector:[fact.clone()]});
            let mut flow: ValueFlowFacts = serde_json::from_value(encoded.clone()).unwrap();
            assert_eq!(
                crate::core::typed_facts::scope_fact_count(&flow),
                1,
                "{vector}: scope accounting"
            );
            assert_eq!(
                serde_json::to_value(&flow).unwrap()[vector],
                encoded[vector]
            );
            let mut typed_keys = std::collections::BTreeSet::new();
            flow.reference_keys(|key| {
                typed_keys.insert(key.to_owned());
            });
            let mut raw_keys = std::collections::BTreeSet::new();
            ValueFlowFacts::reference_keys_from_value(&encoded, |key| {
                raw_keys.insert(key.to_owned());
            });
            assert_eq!(typed_keys, raw_keys, "{vector}: dependency projection");
            flow.offset_lines(10);
            assert_eq!(
                serde_json::to_value(&flow).unwrap()[vector][0]["position"]["line"],
                12
            );
            if vector == "factory_results" {
                assert_eq!(
                    flow.factory_results().next().unwrap().members[0]
                        .position
                        .line,
                    12
                );
            }
            let public = json!({"nodes":[{"id":"scope","kind":"template_scope","name":"scope","qualname":"scope","path":"page.html","line":1,"end_line":3,"is_test":false,"language":"javascript","generated":false,"details":{"classic_global":true,"value_flow":encoded}}],"edges":[],"references":[],"docs":[],"errors":[]});
            let mut typed: TypedFacts = serde_json::from_value(public).unwrap();
            // Generated projections can create the first flow on an otherwise
            // empty scope; canonical serialization must publish that entry too.
            typed.facts.nodes[0]
                .details
                .as_object_mut()
                .unwrap()
                .remove("value_flow");
            typed.flows.insert_classic(
                "scope".into(),
                serde_json::from_value(encoded.clone()).unwrap(),
            );
            let reopened = decode_facts(&encode_facts(&typed).unwrap()).unwrap();
            assert_eq!(
                serde_json::to_value(&typed).unwrap(),
                serde_json::to_value(&reopened).unwrap(),
                "{vector}: compressed classic round trip"
            );
            assert_eq!(
                reopened.flows.get(&reopened.nodes[0]).unwrap(),
                &serde_json::from_value::<ValueFlowFacts>(encoded.clone()).unwrap()
            );
            for count in [128, 129] {
                let bounded = json!({vector:vec![fact.clone(); count]});
                assert_eq!(
                    serde_json::from_value::<ValueFlowFacts>(bounded).is_ok(),
                    count == 128,
                    "{vector}: vector boundary"
                );
            }
            let mut unknown_field = fact.clone();
            unknown_field["unchecked"] = true.into();
            let mut missing_position = fact.clone();
            missing_position.as_object_mut().unwrap().remove("position");
            for malformed in [
                unknown_field,
                missing_position,
                json!({"kind":"unsupported"}),
            ] {
                assert!(
                    serde_json::from_value::<ValueFlowFacts>(json!({vector:[malformed]})).is_err(),
                    "{vector}: strict fact"
                );
            }
        }
        let facts = ValueFlowFacts {
            bindings: vec![BindingFact {
                name: "client".into(),
                position: SourcePosition { line: 2, column: 3 },
                value: ValueExpr::Construct {
                    callee: "Worker".into(),
                },
                conditional: false,
            }],
            ..Default::default()
        };
        let json = serde_json::to_value(&facts).unwrap();
        assert_eq!(
            serde_json::from_value::<ValueFlowFacts>(json).unwrap(),
            facts
        );
        assert!(serde_json::from_str::<ValueFlowFacts>("{}")
            .unwrap()
            .is_empty());
        let legacy = serde_json::json!({"bindings": [], "fields": []});
        assert!(serde_json::from_value::<ValueFlowFacts>(legacy)
            .unwrap()
            .branch_joins
            .is_empty());
        assert!(serde_json::from_str::<ValueFlowFacts>(r#"{"guessed_type":"Worker"}"#).is_err());
        for encoded in ["{}", r#"{"bindings":[],"fields":[],"rule_origins":[]}"#] {
            let mut empty: ValueFlowFacts = serde_json::from_str(encoded).unwrap();
            empty.offset_lines(10);
            assert!(
                empty.framework.is_none(),
                "empty scopes need no framework allocation"
            );
        }
        let ordered: ValueFlowFacts = serde_json::from_value(json!({
            "bindings": [{"name":"x","position":{"line":1,"column":0},"value":{"kind":"unknown"},"conditional":false}],
            "rule_origins": [{"pack":"proof","rule":"order"}],
            "return_type": {"kind":"named","name":"Worker"}
        })).unwrap();
        assert_eq!(
            serde_json::to_value(&ordered)
                .unwrap()
                .as_object()
                .unwrap()
                .keys()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            ["rule_origins", "bindings", "return_type"]
        );
    }

    #[test]
    fn source_order_retains_same_line_columns_and_document_offsets() {
        let mut facts = ValueFlowFacts {
            bindings: vec![BindingFact {
                name: "x".into(),
                position: SourcePosition { line: 1, column: 4 },
                value: ValueExpr::Unknown,
                conditional: true,
            }],
            ..Default::default()
        };
        facts.offset_lines(10);
        assert_eq!(
            facts.bindings[0].position,
            SourcePosition {
                line: 11,
                column: 4
            }
        );
        assert!(
            SourcePosition {
                line: 11,
                column: 4
            } < SourcePosition {
                line: 11,
                column: 5
            }
        );
    }

    #[test]
    fn receiver_catalog_and_typescript_bindings_round_trip() {
        let mut catalog = ReceiverCatalog::new();
        catalog.register("AppService", "service:app");
        assert_eq!(catalog.lookup("AppService"), Some("service:app"));
        assert_eq!(catalog.lookup("Missing"), None);

        let json = serde_json::to_string(&catalog).expect("serialize catalog");
        let deserialized: ReceiverCatalog =
            serde_json::from_str(&json).expect("deserialize catalog");
        assert_eq!(catalog, deserialized);

        let bindings = TypeScriptBindings {
            bindings: vec![TypeScriptBinding {
                alias: "express".into(),
                module: "express".into(),
                declaration: SourcePosition { line: 1, column: 1 },
                completed: SourcePosition {
                    line: 1,
                    column: 25,
                },
                invalidated: false,
                captured_safe: true,
            }],
        };
        let bindings_json = serde_json::to_string(&bindings).expect("serialize bindings");
        let deserialized_bindings: TypeScriptBindings =
            serde_json::from_str(&bindings_json).expect("deserialize bindings");
        assert_eq!(bindings, deserialized_bindings);
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum SemanticTarget {
    ProjectPathSuffix { prefix: String, language: String },
    ImportedSymbol { language: String },
    PythonSymbol { language: String },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SemanticRelation {
    RendersTemplate,
    InjectsDependency,
    UsesModel,
    References,
}

impl SemanticRelation {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::RendersTemplate => "renders_template",
            Self::InjectsDependency => "injects_dependency",
            Self::UsesModel => "uses_model",
            Self::References => "references",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SemanticIdentity {
    pub(crate) scope: String,
    pub(crate) namespace: String,
    pub(crate) target: SemanticTarget,
    pub(crate) path: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SemanticOccurrence {
    pub(crate) owner_id: String,
    pub(crate) line: usize,
    pub(crate) column: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum SemanticFactRole {
    FileProvider {
        target: String,
    },
    CallToFile {
        import_owner: String,
        alias: String,
        module: String,
        export: String,
        relation: SemanticRelation,
    },
    ClassAttributeToFile {
        attribute: String,
        relation: SemanticRelation,
    },
    ClassAttributeToSymbol {
        attribute: String,
        relation: SemanticRelation,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SemanticFact {
    pub(crate) pack: String,
    pub(crate) rule: String,
    pub(crate) identity: SemanticIdentity,
    pub(crate) occurrence: SemanticOccurrence,
    pub(crate) expression: String,
    pub(crate) role: SemanticFactRole,
}
