//! Typed binary encoding for extracted facts and their semantic sidecars.

use super::TypedFacts;
use crate::core::models::{Diagnostic, DocSection, Edge, Node, Reference};

pub(crate) trait BinaryEncode {
    fn encode(&self, buf: &mut Vec<u8>);
}
pub(crate) trait BinaryDecode: Sized {
    fn decode(bytes: &[u8], cursor: &mut usize) -> Option<Self>;
}

macro_rules! number_codec {
    ($($ty:ty),+ $(,)?) => {$ (
        impl BinaryEncode for $ty { fn encode(&self, buf: &mut Vec<u8>) { buf.extend_from_slice(&self.to_le_bytes()); } }
        impl BinaryDecode for $ty {
            fn decode(bytes: &[u8], cursor: &mut usize) -> Option<Self> {
                let end = cursor.checked_add(std::mem::size_of::<Self>())?;
                let value = Self::from_le_bytes(bytes.get(*cursor..end)?.try_into().ok()?);
                *cursor = end; Some(value)
            }
        }
    )+};
}
number_codec!(u8, u32, u64, i64, f64);

impl BinaryEncode for usize {
    fn encode(&self, buf: &mut Vec<u8>) { (*self as u64).encode(buf); }
}
impl BinaryDecode for usize {
    fn decode(bytes: &[u8], cursor: &mut usize) -> Option<Self> { Self::try_from(u64::decode(bytes, cursor)?).ok() }
}
impl BinaryEncode for bool {
    fn encode(&self, buf: &mut Vec<u8>) { u8::from(*self).encode(buf); }
}
impl BinaryDecode for bool {
    fn decode(bytes: &[u8], cursor: &mut usize) -> Option<Self> {
        match u8::decode(bytes, cursor)? { 0 => Some(false), 1 => Some(true), _ => None }
    }
}
impl BinaryEncode for str {
    fn encode(&self, buf: &mut Vec<u8>) {
        (self.len() as u32).encode(buf); buf.extend_from_slice(self.as_bytes());
    }
}
impl BinaryEncode for String { fn encode(&self, buf: &mut Vec<u8>) { self.as_str().encode(buf); } }
impl BinaryDecode for String {
    fn decode(bytes: &[u8], cursor: &mut usize) -> Option<Self> {
        let len = usize::try_from(u32::decode(bytes, cursor)?).ok()?;
        let end = cursor.checked_add(len)?;
        let value = std::str::from_utf8(bytes.get(*cursor..end)?).ok()?.to_owned();
        *cursor = end; Some(value)
    }
}
impl<T: BinaryEncode> BinaryEncode for Option<T> {
    fn encode(&self, buf: &mut Vec<u8>) {
        self.is_some().encode(buf); if let Some(value) = self { value.encode(buf); }
    }
}
impl<T: BinaryDecode> BinaryDecode for Option<T> {
    fn decode(bytes: &[u8], cursor: &mut usize) -> Option<Self> {
        match u8::decode(bytes, cursor)? { 0 => Some(None), 1 => Some(Some(T::decode(bytes, cursor)?)), _ => None }
    }
}
impl<T: BinaryEncode> BinaryEncode for [T] {
    fn encode(&self, buf: &mut Vec<u8>) {
        (self.len() as u32).encode(buf); for value in self { value.encode(buf); }
    }
}
fn collection_count(bytes: &[u8], cursor: &mut usize) -> Option<usize> {
    let count = usize::try_from(u32::decode(bytes, cursor)?).ok()?;
    (count <= bytes.len().saturating_sub(*cursor)).then_some(count)
}
impl<T: BinaryDecode> BinaryDecode for Vec<T> {
    fn decode(bytes: &[u8], cursor: &mut usize) -> Option<Self> {
        let count = collection_count(bytes, cursor)?; let mut values = Vec::with_capacity(count);
        for _ in 0..count { values.push(T::decode(bytes, cursor)?); }
        Some(values)
    }
}

fn encode_items<I>(items: I, buf: &mut Vec<u8>)
where
    I: ExactSizeIterator,
    I::Item: BinaryEncode,
{
    (items.len() as u32).encode(buf);
    for item in items {
        item.encode(buf);
    }
}

impl<T: BinaryEncode + ?Sized> BinaryEncode for &T {
    fn encode(&self, buf: &mut Vec<u8>) { (**self).encode(buf); }
}
impl<T: BinaryEncode> BinaryEncode for Box<T> {
    fn encode(&self, buf: &mut Vec<u8>) { (**self).encode(buf); }
}
impl<T: BinaryDecode> BinaryDecode for Box<T> {
    fn decode(bytes: &[u8], cursor: &mut usize) -> Option<Self> { Some(Box::new(T::decode(bytes, cursor)?)) }
}
impl<A: BinaryEncode, B: BinaryEncode> BinaryEncode for (A, B) {
    fn encode(&self, buf: &mut Vec<u8>) { self.0.encode(buf); self.1.encode(buf); }
}
impl<A: BinaryDecode, B: BinaryDecode> BinaryDecode for (A, B) {
    fn decode(bytes: &[u8], cursor: &mut usize) -> Option<Self> { Some((A::decode(bytes, cursor)?, B::decode(bytes, cursor)?)) }
}
impl<K: BinaryEncode, V: BinaryEncode> BinaryEncode for std::collections::BTreeMap<K, V> {
    fn encode(&self, buf: &mut Vec<u8>) {
        (self.len() as u32).encode(buf); for (key, value) in self { key.encode(buf); value.encode(buf); }
    }
}
impl<K: BinaryDecode + Ord, V: BinaryDecode> BinaryDecode for std::collections::BTreeMap<K, V> {
    fn decode(bytes: &[u8], cursor: &mut usize) -> Option<Self> {
        let count = collection_count(bytes, cursor)?; let mut values = Self::new();
        for _ in 0..count { values.insert(K::decode(bytes, cursor)?, V::decode(bytes, cursor)?); }
        Some(values)
    }
}
impl BinaryEncode for serde_json::Value {
    fn encode(&self, buf: &mut Vec<u8>) {
        match self {
            Self::Null => 0u8.encode(buf),
            Self::Bool(false) => 1u8.encode(buf),
            Self::Bool(true) => 2u8.encode(buf),
            Self::Number(number) if number.is_i64() => { 3u8.encode(buf); number.as_i64().expect("signed JSON number").encode(buf); }
            Self::Number(number) if number.is_u64() => { 4u8.encode(buf); number.as_u64().expect("unsigned JSON number").encode(buf); }
            Self::Number(number) => { 5u8.encode(buf); number.as_f64().expect("finite JSON number").encode(buf); }
            Self::String(value) => { 6u8.encode(buf); value.encode(buf); }
            Self::Array(values) => { 7u8.encode(buf); values.encode(buf); }
            Self::Object(values) => {
                8u8.encode(buf);
                (values.len() as u32).encode(buf); for (key, value) in values { key.encode(buf); value.encode(buf); }
            }
        }
    }
}
impl BinaryDecode for serde_json::Value {
    fn decode(bytes: &[u8], cursor: &mut usize) -> Option<Self> {
        use serde_json::{Map, Number, Value};
        match u8::decode(bytes, cursor)? {
            0 => Some(Value::Null),
            1 => Some(Value::Bool(false)),
            2 => Some(Value::Bool(true)),
            3 => Some(Value::Number(Number::from(i64::decode(bytes, cursor)?))),
            4 => Some(Value::Number(Number::from(u64::decode(bytes, cursor)?))),
            5 => Some(Value::Number(Number::from_f64(f64::decode(bytes, cursor)?)?)),
            6 => Some(Value::String(String::decode(bytes, cursor)?)),
            7 => Some(Value::Array(Vec::decode(bytes, cursor)?)),
            8 => {
                let count = collection_count(bytes, cursor)?; let mut values = Map::new();
                for _ in 0..count {
                    let key = String::decode(bytes, cursor)?; let value = Value::decode(bytes, cursor)?;
                    if values.insert(key, value).is_some() { return None; }
                }
                Some(Value::Object(values))
            }
            _ => None,
        }
    }
}

macro_rules! struct_codecs {
    ($($ty:ty { $($field:ident),+ $(,)? };)+) => {
        $(
            impl BinaryEncode for $ty {
                fn encode(&self, buf: &mut Vec<u8>) { $(self.$field.encode(buf);)+ }
            }
            impl BinaryDecode for $ty {
                fn decode(bytes: &[u8], cursor: &mut usize) -> Option<Self> {
                    Some(Self { $($field: BinaryDecode::decode(bytes, cursor)?,)+ })
                }
            }
        )+
    };
}
pub(crate) use struct_codecs;

macro_rules! validated_struct_codec {
    ($ty:ty { $($field:ident),+ $(,)? } => $validate:ident) => {
        impl BinaryEncode for $ty {
            fn encode(&self, buf: &mut Vec<u8>) { $(self.$field.encode(buf);)+ }
        }
        impl BinaryDecode for $ty {
            fn decode(bytes: &[u8], cursor: &mut usize) -> Option<Self> {
                let value = Self { $($field: BinaryDecode::decode(bytes, cursor)?,)+ };
                $validate(value)
            }
        }
    };
}

struct_codecs! {
    crate::core::typed_facts::NodeSemanticEntry { bindings, rebindings, param_types, admitted };
    Node { id, kind, name, qualname, path, line, end_line, is_test, language, generated, details, native_semantic };
    Edge { src, dst, kind, path, line, evidence, confidence };
    Reference { source, expression, kind, line, column, alias, module, dynamic, receiver_hint };
    DocSection { doc_id, path, section_title, doc_type, content, invariants, referenced_symbols, mtime, size, is_invariant };
    Diagnostic { path, line, message };
}
macro_rules! enum_codec {
    ($ty:ty, $validate:ident { $($tag:literal => $variant:ident $( { $($field:ident),+ } )?),+ $(,)? }) => {
        impl BinaryEncode for $ty {
            fn encode(&self, buf: &mut Vec<u8>) {
                match self { $(Self::$variant $({ $($field),+ })? => {
                    let tag: u8 = $tag;
                    tag.encode(buf);
                    $($( $field.encode(buf); )+)?
                },)+ }
            }
        }
        impl BinaryDecode for $ty {
            fn decode(bytes: &[u8], cursor: &mut usize) -> Option<Self> {
                let value = match u8::decode(bytes, cursor)? {
                    $($tag => Self::$variant $({ $($field: BinaryDecode::decode(bytes, cursor)?),+ })?,)+
                    _ => return None,
                };
                $validate(value)
            }
        }
    };
    ($ty:ty { $($tag:literal => $variant:ident $( { $($field:ident),+ } )?),+ $(,)? }) => {
        enum_codec!($ty, identity_codec { $($tag => $variant $( { $($field),+ } )?),+ });
    };
}
macro_rules! tuple_enum_codec {
    ($ty:ty { $($tag:literal => $variant:ident($payload:ty)),+ $(,)? }) => {
        impl BinaryEncode for $ty {
            fn encode(&self, buf: &mut Vec<u8>) {
                match self {
                    $(Self::$variant(value) => {
                        ($tag as u8).encode(buf);
                        value.encode(buf);
                    },)+
                }
            }
        }
        impl BinaryDecode for $ty {
            fn decode(bytes: &[u8], cursor: &mut usize) -> Option<Self> {
                match u8::decode(bytes, cursor)? {
                    $($tag => Some(Self::$variant(
                        <$payload as BinaryDecode>::decode(bytes, cursor)?,
                    )),)+
                    _ => None,
                }
            }
        }
    };
}

macro_rules! tagged_state_codec {
    ($ty:ty, $buf:ident, $bytes:ident, $cursor:ident {
        $($tag:literal => ($pattern:pat) => $encode:block => $decode:block),+ $(,)?
    }) => {
        impl BinaryEncode for $ty {
            fn encode(&self, $buf: &mut Vec<u8>) {
                match self {
                    $($pattern => { ($tag as u8).encode($buf); $encode },)+
                }
            }
        }
        impl BinaryDecode for $ty {
            fn decode($bytes: &[u8], $cursor: &mut usize) -> Option<Self> {
                match u8::decode($bytes, $cursor)? {
                    $($tag => $decode,)+
                    _ => None,
                }
            }
        }
    };
}
fn identity_codec<T>(value: T) -> Option<T> {
    Some(value)
}
enum_codec!(crate::core::models::ReceiverHint, validate_receiver_hint {
    0 => StringLiteral { member }, 1 => CallResult { callee, member }, 2 => ConstructorResult { callee, member },
    3 => Super { member }, 4 => VueSetupBinding, 5 => VueComponentTag, 6 => VueTypedPropMember { field_id },
    7 => VueSetupMacroCallable, 8 => VueSetupRuntimeCallable, 9 => TypeOnlyImport, 10 => HtmlClassicScriptSource,
    11 => ConfiguredTypedMember { origin, field_id }, 12 => ConfiguredMacroCallable { origin, producer },
    13 => ConfiguredReference { origin }, 14 => NamedRoute { origin, key }, 15 => TypedCallResult { value, member },
});

struct_codecs! {
    crate::core::semantic::SourcePosition { line, column }; crate::core::semantic::CallSiteEvidence { position, receiver, arguments };
    crate::core::semantic::ObjectMember { name, position, target }; crate::core::semantic::BindingFact { name, position, value, conditional };
    crate::core::semantic::BranchJoinFact { name, position, value, conditional }; crate::core::semantic::FieldFact { name, position, value, conditional };
    crate::core::semantic::CollectionElementFact { name, position, element_type }; crate::core::semantic::ExtractionRuleOrigin { pack, rule };
    crate::core::semantic::ObjectOwnershipFact { origin, guard };
    crate::core::semantic::ComponentMacroFact { origin, role, callee, binding, position, annotation };
    crate::core::semantic::TemplateContextFact { origin, import_alias, target, target_path, keys, position };
    crate::core::semantic::ComponentConfigurationFact { origin, enabled, modules, callable_imports };
    crate::core::semantic::CallableImportConfiguration { enabled, scan }; crate::core::semantic::TypeOnlyImportFact { alias, position, guard_import };
    crate::core::semantic::GuardImportFact { alias, position }; crate::core::semantic::AnnotationReferenceFact { expression, position, value_query };
    crate::core::semantic::VueSlotFact { slot, binding, position, type_expr };
    crate::core::semantic::SymbolAttachmentFact { origin, position, source, target, relation };
    crate::core::semantic::FactoryResultFact { origin, position, binding, configured_module, members };
    crate::core::semantic::NamedBindingFact { origin, position, domain, key, role };
    crate::core::semantic::EndpointRequestFact { origin, position, url, method }; crate::core::semantic::SemanticIdentity { scope, namespace, target, path };
    crate::core::semantic::SemanticOccurrence { owner_id, line, column }; crate::core::semantic::SemanticFact { pack, rule, identity, occurrence, expression, role };
    crate::core::typed_facts::SemanticEntry { owner, line, qualname, state };
}

tuple_enum_codec!(crate::core::semantic::FrameworkFact {
    0 => SymbolAttachment(crate::core::semantic::SymbolAttachmentFact), 1 => FactoryResult(crate::core::semantic::FactoryResultFact),
    2 => NamedBinding(crate::core::semantic::NamedBindingFact), 3 => EndpointRequest(crate::core::semantic::EndpointRequestFact),
    4 => ComponentConfiguration(crate::core::semantic::ComponentConfigurationFact), 5 => RuleOrigin(crate::core::semantic::ExtractionRuleOrigin),
    6 => ComponentMacro(crate::core::semantic::ComponentMacroFact), 7 => CollectionElement(crate::core::semantic::CollectionElementFact),
    8 => VueSlot(crate::core::semantic::VueSlotFact), 9 => TemplateContext(crate::core::semantic::TemplateContextFact),
    10 => TypeOnlyImport(crate::core::semantic::TypeOnlyImportFact), 11 => AnnotationReference(crate::core::semantic::AnnotationReferenceFact),
});

struct_codecs! {
    crate::core::semantic::FrameworkScopeFacts {
        object_ownership,
        fact_limit_exceeded,
        async_return_type,
        return_value,
        return_position,
        alias_type,
        facts
    };
}

enum_codec!(crate::core::semantic::TypeExpr { 0 => Named { name }, 1 => Applied { base, args }, 2 => Unknown });
enum_codec!(crate::core::semantic::StaticArgument { 0 => String { value }, 1 => Unknown });
enum_codec!(crate::core::semantic::ValueExpr, validate_value_expr {
    0 => Annotated { type_expr }, 1 => Construct { callee }, 2 => Alias { name }, 3 => Call { callee, site },
    4 => Await { value }, 5 => LoopElement { iterable, body_end }, 6 => Scoped { value, body_end },
    7 => BranchJoin { alternatives }, 8 => Field { receiver, member }, 9 => Project { value, member },
    10 => Object { members, builtin_guard }, 11 => Unknown,
});
enum_codec!(crate::core::semantic::ObjectMemberTarget { 0 => SameName, 1 => Alias { name } });
enum_codec!(crate::core::semantic::ComponentMacroRole { 0 => Props, 1 => Emits, 2 => Slots, 3 => Marker });
enum_codec!(crate::core::semantic::StaticString, validate_static_string { 0 => Literal { value }, 1 => Unknown });
enum_codec!(crate::core::semantic::SymbolOperand { 0 => Node { id }, 1 => Name { expression }, 2 => Unknown });
enum_codec!(crate::core::semantic::AttachmentRelation { 0 => UsesModel, 1 => References });
enum_codec!(crate::core::semantic::NamedTargetKind { 0 => Object, 1 => Callable, 2 => ListenerScope });
enum_codec!(crate::core::semantic::NamedBindingRole {
    0 => Definition { target, target_kind }, 1 => RegistryDefinition { receiver, target }, 2 => Reference,
});
enum_codec!(crate::core::semantic::SemanticTarget {
    0 => ProjectPathSuffix { prefix, language }, 1 => ImportedSymbol { language }, 2 => PythonSymbol { language },
});
enum_codec!(crate::core::semantic::SemanticRelation { 0 => RendersTemplate, 1 => InjectsDependency, 2 => UsesModel, 3 => References });
enum_codec!(crate::core::semantic::SemanticFactRole {
    0 => FileProvider { target }, 1 => CallToFile { import_owner, alias, module, export, relation },
    2 => ClassAttributeToFile { attribute, relation }, 3 => ClassAttributeToSymbol { attribute, relation },
});
validated_struct_codec!(crate::core::semantic::NamedDefinitionUncertainty {
    domain, key, origin
} => validate_named_uncertainty);

fn validate_named_uncertainty(
    value: crate::core::semantic::NamedDefinitionUncertainty,
) -> Option<crate::core::semantic::NamedDefinitionUncertainty> {
    value.valid().then_some(value)
}

fn validate_value_expr(
    value: crate::core::semantic::ValueExpr,
) -> Option<crate::core::semantic::ValueExpr> {
    value.valid_call_sites(0).then_some(value)
}
fn validate_static_string(
    value: crate::core::semantic::StaticString,
) -> Option<crate::core::semantic::StaticString> {
    match &value {
        crate::core::semantic::StaticString::Literal { value }
            if value.is_empty() || value.len() > 256 || value.chars().any(char::is_control) =>
        {
            None
        }
        _ => Some(value),
    }
}
validated_struct_codec!(crate::core::semantic::ValueFlowFacts {
    bindings, branch_joins, immutable_initializers, return_type, fields, framework
} => validate_value_flow_facts);

fn validate_value_flow_facts(
    value: crate::core::semantic::ValueFlowFacts,
) -> Option<crate::core::semantic::ValueFlowFacts> {
    value.valid_for_cache().then_some(value)
}

tagged_state_codec!(crate::core::typed_facts::FlowState, buf, bytes, cursor {
    0 => (crate::core::typed_facts::FlowState::Valid { facts, classic_fields_first, .. }) => {
        facts.encode(buf);
        classic_fields_first.encode(buf);
    } => {
        Some(crate::core::typed_facts::FlowState::Valid {
            facts: Box::decode(bytes, cursor)?,
            original: None,
            classic_fields_first: Option::decode(bytes, cursor)?,
        })
    },
    1 => (crate::core::typed_facts::FlowState::Invalid(value)) => { value.encode(buf); } => {
        Some(crate::core::typed_facts::FlowState::Invalid(
            serde_json::Value::decode(bytes, cursor)?,
        ))
    },
});

tagged_state_codec!(crate::core::typed_facts::SemanticState, buf, bytes, cursor {
    0 => (crate::core::typed_facts::SemanticState::Valid { facts, .. }) => { facts.encode(buf); } => {
        Some(crate::core::typed_facts::SemanticState::Valid {
            facts: Vec::decode(bytes, cursor)?, original: None,
        })
    },
    1 => (crate::core::typed_facts::SemanticState::Invalid { original, error }) => {
        original.encode(buf);
        error.encode(buf);
    } => {
        Some(crate::core::typed_facts::SemanticState::Invalid {
            original: serde_json::Value::decode(bytes, cursor)?,
            error: String::decode(bytes, cursor)?,
        })
    },
});

fn validate_receiver_hint(
    hint: crate::core::models::ReceiverHint,
) -> Option<crate::core::models::ReceiverHint> {
    if let crate::core::models::ReceiverHint::TypedCallResult { value, member } = &hint {
        if !matches!(value, crate::core::semantic::ValueExpr::Call { .. })
            || !value.valid_call_sites(0)
            || member.is_empty()
            || member.len() > 4096
            || member.chars().any(char::is_control)
        {
            return None;
        }
    }
    Some(hint)
}

impl BinaryEncode for TypedFacts {
    fn encode(&self, buf: &mut Vec<u8>) {
        encode_items(self.facts.nodes.iter(), buf);
        encode_items(self.facts.edges.iter(), buf);
        encode_items(self.facts.references.iter(), buf);
        encode_items(self.facts.docs.iter(), buf);
        encode_items(self.facts.errors.iter(), buf);
        encode_items(self.facts.pending_flow_nodes.iter(), buf);
        encode_items(self.flows.cache_entries(), buf);
        encode_items(self.flows.named_uncertainty_entries(), buf);
        encode_items(self.flows.named_uncertainty_faults(), buf);
        encode_items(self.facts.semantic.entries(), buf);
    }
}
impl BinaryDecode for TypedFacts {
    fn decode(bytes: &[u8], cursor: &mut usize) -> Option<Self> {
        let mut facts = crate::core::models::Facts {
            nodes: Vec::decode(bytes, cursor)?,
            edges: Vec::decode(bytes, cursor)?,
            references: Vec::decode(bytes, cursor)?,
            docs: Vec::decode(bytes, cursor)?,
            errors: Vec::decode(bytes, cursor)?,
            pending_flow_nodes: Vec::decode(bytes, cursor)?,
            ..Default::default()
        };
        let mut flows = crate::core::typed_facts::FlowStore::default();
        for (owner, state) in Vec::<(String, crate::core::typed_facts::FlowState)>::decode(
            bytes, cursor,
        )? {
            flows.restore_cache_entry(owner, Some(state), Vec::new(), false);
        }
        for (owner, uncertainties) in Vec::<(
            String,
            Vec<crate::core::semantic::NamedDefinitionUncertainty>,
        )>::decode(bytes, cursor)?
        {
            flows.restore_cache_entry(owner, None, uncertainties, false);
        }
        for owner in Vec::<String>::decode(bytes, cursor)? {
            flows.restore_cache_entry(owner, None, Vec::new(), true);
        }
        for (index, entry) in Vec::<(
            usize,
            crate::core::typed_facts::SemanticEntry,
        )>::decode(bytes, cursor)?
        {
            facts.semantic.restore_cache_entry(index, entry);
        }
        facts.semantic.mark_complete();
        Some(Self { facts, flows })
    }
}
