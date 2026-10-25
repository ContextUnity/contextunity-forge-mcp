use super::{FlowState, FlowStore, NodeSemanticEntry, SemanticState, TypedFacts};
use crate::core::{
    models::{Facts, Node},
    semantic::ValueFlowFacts,
};
use serde::ser::SerializeStruct;
use serde::{Deserialize, Serialize, Serializer};
use serde_json::Value;

macro_rules! serde_dto {
    ($vis:vis $name:ident<$($lifetime:lifetime),+> { $($(#[$attr:meta])* $field:ident: $ty:ty),+ $(,)? }) => {
        #[derive(Serialize)]
        $vis struct $name<$($lifetime),+> { $($(#[$attr])* $field: $ty,)+ }
    };
}

pub(crate) struct ParameterTypesView<'a>(pub(crate) &'a NodeSemanticEntry);

impl Serialize for ParameterTypesView<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_map(self.0.parameters())
    }
}

#[derive(Serialize)]
#[serde(untagged)]
enum SemanticStateView<'a> {
    Raw(&'a Value),
    Facts(&'a [crate::core::semantic::SemanticFact]),
}

impl<'a> From<&'a SemanticState> for SemanticStateView<'a> {
    fn from(state: &'a SemanticState) -> Self {
        match state {
            SemanticState::Valid { original: Some(value), .. } | SemanticState::Invalid { original: value, .. } => Self::Raw(value),
            SemanticState::Valid { facts, .. } => Self::Facts(facts),
        }
    }
}

impl SemanticState {
    pub(crate) fn view(&self) -> impl Serialize + '_ { SemanticStateView::from(self) }
}

#[derive(Serialize)]
#[serde(untagged)]
enum FlowStateView<'a> {
    Raw(&'a Value),
    Facts(&'a ValueFlowFacts),
    FieldsFirst(ClassicFieldsFirstDto<'a>),
    BindingsFirst(ClassicBindingsFirstDto<'a>),
}

impl<'a> From<&'a FlowState> for FlowStateView<'a> {
    fn from(state: &'a FlowState) -> Self {
        match state {
            FlowState::Valid { original: Some(value), .. } | FlowState::Invalid(value) => Self::Raw(value),
            FlowState::Valid { facts, original: None, classic_fields_first: Some(fields_first) } => {
                let fields = (!facts.fields.is_empty()).then_some(facts.fields.as_slice());
                let bindings = (!facts.bindings.is_empty()).then_some(facts.bindings.as_slice());
                let rest = ClassicFlowRestDto::from(facts.as_ref());
                if *fields_first {
                    Self::FieldsFirst(ClassicFieldsFirstDto { fields, bindings, rest })
                } else {
                    Self::BindingsFirst(ClassicBindingsFirstDto { bindings, fields, rest })
                }
            }
            FlowState::Valid { facts, original: None, classic_fields_first: None } => Self::Facts(facts),
        }
    }
}

impl FlowState {
    pub(crate) fn view(&self) -> impl Serialize + '_ { FlowStateView::from(self) }
}

serde_dto!(ClassicFlowRestDto<'a> {
    #[serde(skip_serializing_if = "Option::is_none")] immutable_initializers: Option<&'a Vec<crate::core::semantic::SourcePosition>>, #[serde(skip_serializing_if = "Option::is_none")] branch_joins: Option<&'a Vec<crate::core::semantic::BranchJoinFact>>,
    #[serde(flatten)] framework_facts: Option<crate::core::semantic::FrameworkFactCollectionsDto<'a>>,
    #[serde(skip_serializing_if = "Option::is_none")] object_ownership: Option<&'a crate::core::semantic::ObjectOwnershipFact>,
    #[serde(skip_serializing_if = "Option::is_none")] return_type: Option<&'a crate::core::semantic::TypeExpr>, #[serde(skip_serializing_if = "Option::is_none")] async_return_type: Option<&'a crate::core::semantic::TypeExpr>,
    #[serde(skip_serializing_if = "Option::is_none")] return_value: Option<&'a crate::core::semantic::ValueExpr>, #[serde(skip_serializing_if = "Option::is_none")] return_position: Option<&'a crate::core::semantic::SourcePosition>,
    #[serde(skip_serializing_if = "Option::is_none")] alias_type: Option<&'a crate::core::semantic::TypeExpr>, #[serde(skip_serializing_if = "Option::is_none")] fact_limit_exceeded: Option<bool>,
});

impl<'a> From<&'a ValueFlowFacts> for ClassicFlowRestDto<'a> {
    fn from(facts: &'a ValueFlowFacts) -> Self {
        let framework = facts.framework.as_deref();
        Self {
            immutable_initializers: (!facts.immutable_initializers.is_empty()).then_some(&facts.immutable_initializers), branch_joins: (!facts.branch_joins.is_empty()).then_some(&facts.branch_joins),
            framework_facts: framework.map(crate::core::semantic::FrameworkFactCollectionsDto::from),
            object_ownership: framework.and_then(|scope| scope.object_ownership.as_ref()), return_type: facts.return_type.as_ref(),
            async_return_type: framework.and_then(|scope| scope.async_return_type.as_ref()), return_value: framework.and_then(|scope| scope.return_value.as_ref()),
            return_position: framework.and_then(|scope| scope.return_position.as_ref()), alias_type: framework.and_then(|scope| scope.alias_type.as_ref()),
            fact_limit_exceeded: framework.filter(|scope| scope.fact_limit_exceeded).map(|_| true),
        }
    }
}

serde_dto!(ClassicFieldsFirstDto<'a> {
    fields: Option<&'a [crate::core::semantic::FieldFact]>, bindings: Option<&'a [crate::core::semantic::BindingFact]>, #[serde(flatten)] rest: ClassicFlowRestDto<'a>,
});
serde_dto!(ClassicBindingsFirstDto<'a> {
    bindings: Option<&'a [crate::core::semantic::BindingFact]>, fields: Option<&'a [crate::core::semantic::FieldFact]>, #[serde(flatten)] rest: ClassicFlowRestDto<'a>,
});

pub(crate) type NodesView<'node, 'flow> = PublicNodesView<'node, 'flow>;

serde_dto!(pub(crate) NodeView<'node, 'flow> {
    id: &'node str, kind: &'node str, name: &'node str, qualname: &'node str,
    path: &'node str, line: usize, end_line: usize, is_test: bool,
    language: &'node str, generated: bool,
    #[serde(serialize_with = "serialize_node_details")] details: NodeDetailsInput<'node, 'flow>,
});

struct NodeDetailsInput<'node, 'flow> { node: &'node Node, flows: &'flow FlowStore, semantic: Option<&'node SemanticState> }

impl<'node, 'flow> NodeView<'node, 'flow> {
    pub(crate) fn new(
        node: &'node Node,
        flows: &'flow FlowStore,
        semantic: Option<&'node SemanticState>,
    ) -> Self {
        Self {
            id: &node.id, kind: &node.kind, name: &node.name, qualname: &node.qualname,
            path: &node.path, line: node.line, end_line: node.end_line, is_test: node.is_test,
            language: &node.language, generated: node.generated,
            details: NodeDetailsInput { node, flows, semantic },
        }
    }
}

serde_dto!(NodeSemanticDto<'a> {
    #[serde(skip_serializing_if = "Option::is_none")] bindings: Option<&'a [String]>,
    #[serde(skip_serializing_if = "Option::is_none")] rebindings: Option<&'a [String]>,
    #[serde(skip_serializing_if = "Option::is_none")] param_types: Option<ParameterTypesView<'a>>,
});

impl<'a> NodeSemanticDto<'a> {
    fn new(entry: Option<&'a NodeSemanticEntry>) -> Option<Self> {
        let entry = entry?;
        let has_bindings = entry.has("bindings"); let has_rebindings = entry.has("rebindings");
        let has_param_types = entry.has("param_types");
        if !has_bindings && !has_rebindings && !has_param_types {
            return None;
        }
        Some(Self {
            bindings: has_bindings.then_some(entry.bindings.as_slice()), rebindings: has_rebindings.then_some(entry.rebindings.as_slice()),
            param_types: has_param_types.then_some(ParameterTypesView(entry)),
        })
    }
}

fn project_if_absent<T>(
    base: &serde_json::Map<String, Value>,
    key: &str,
    project: impl FnOnce() -> Option<T>,
) -> Option<T> {
    if base.contains_key(key) { None } else { project() }
}

impl NodeSemanticEntry {
    pub(crate) fn view(&self) -> impl Serialize + '_ { NodeSemanticDto::new(Some(self)).expect("semantic entry has admitted fields") }
}

serde_dto!(NodeDetailsDto<'node, 'flow> {
    #[serde(flatten)] base: &'node serde_json::Map<String, Value>, #[serde(flatten, skip_serializing_if = "Option::is_none")] semantic: Option<NodeSemanticDto<'node>>,
    #[serde(skip_serializing_if = "Option::is_none")] value_flow: Option<FlowStateView<'flow>>, #[serde(skip_serializing_if = "Option::is_none")] semantic_facts: Option<SemanticStateView<'node>>,
    #[serde(skip_serializing_if = "Option::is_none")] named_definition_uncertainties: Option<&'flow [crate::core::semantic::NamedDefinitionUncertainty]>,
    #[serde(skip_serializing_if = "Option::is_none")] named_definition_uncertainty_fault: Option<bool>,
});

static EMPTY_BASE_DETAILS: std::sync::LazyLock<serde_json::Map<String, Value>> =
    std::sync::LazyLock::new(serde_json::Map::new);

impl<'node, 'flow> NodeDetailsDto<'node, 'flow> {
    fn new(
        node: &'node Node,
        base: &'node serde_json::Map<String, Value>,
        flows: &'flow FlowStore,
        semantic_facts: Option<&'node SemanticState>,
    ) -> Option<Self> {
        let mut semantic = NodeSemanticDto::new(node.native_semantic.as_deref());
        if let Some(view) = &mut semantic {
            if base.contains_key("bindings") { view.bindings = None; }
            if base.contains_key("rebindings") { view.rebindings = None; }
            if base.contains_key("param_types") { view.param_types = None; }
        }
        let semantic = semantic.filter(|view| view.bindings.is_some() || view.rebindings.is_some() || view.param_types.is_some());
        let value_flow = project_if_absent(base, "value_flow", || flows.state(node).map(FlowStateView::from));
        let semantic_facts = project_if_absent(base, "semantic_facts", || semantic_facts.map(SemanticStateView::from));
        let uncertainties = project_if_absent(base, "named_definition_uncertainties", || {
            let markers = flows.named_uncertainties(&node.id);
            (!markers.is_empty()).then_some(markers)
        });
        let uncertainty_fault = project_if_absent(base, "named_definition_uncertainty_fault", || flows.named_uncertainty_fault(&node.id).then_some(true));
        let has_projection = semantic.is_some() || value_flow.is_some() || semantic_facts.is_some()
            || uncertainties.is_some() || uncertainty_fault.is_some();
        if !has_projection {
            return None;
        }
        Some(Self { base, semantic, value_flow, semantic_facts,
            named_definition_uncertainties: uncertainties, named_definition_uncertainty_fault: uncertainty_fault })
    }
}

fn serialize_node_details<S: Serializer>(
    input: &NodeDetailsInput<'_, '_>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    let base = match &input.node.details {
        Value::Object(base) => base, Value::Null => &EMPTY_BASE_DETAILS, value => return value.serialize(serializer),
    };
    match NodeDetailsDto::new(input.node, base, input.flows, input.semantic) {
        Some(projected) => projected.serialize(serializer),
        None if base.is_empty() => serializer.serialize_unit(),
        None => base.serialize(serializer),
    }
}

#[derive(Serialize)]
#[serde(transparent)]
pub(crate) struct PublicNodesView<'node, 'flow>(Vec<NodeView<'node, 'flow>>);

impl<'node, 'flow> PublicNodesView<'node, 'flow> {
    pub(crate) fn new(facts: &'node Facts, flows: &'flow FlowStore) -> Self {
        Self(facts.nodes.iter().enumerate().map(|(index, node)| NodeView::new(node, flows, facts.semantic.state(index))).collect())
    }
}

impl Serialize for TypedFacts {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut state = serializer.serialize_struct("Facts", 5)?;
        state.serialize_field("nodes", &self.nodes_view())?;
        state.serialize_field("edges", &self.facts.edges)?; state.serialize_field("references", &self.facts.references)?;
        state.serialize_field("docs", &self.facts.docs)?; state.serialize_field("errors", &self.facts.errors)?;
        state.end()
    }
}

impl<'de> Deserialize<'de> for TypedFacts {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let facts = Self::from_public(Facts::deserialize(deserializer)?);
        if facts.flows.has_invalid_named_uncertainty_carriers() { return Err(serde::de::Error::custom("malformed named-definition uncertainty carrier")); }
        Ok(facts)
    }
}
