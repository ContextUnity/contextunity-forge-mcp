use super::value_flow::{SemanticLimits, valid_scope_facts};
use crate::core::models::{Facts, Node};
use crate::core::semantic::{SourcePosition, TypeExpr, ValueExpr, ValueFlowFacts};
use crate::core::typed_facts::{FlowState, FlowStore, TypedFacts};
use crate::engine::languages;
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    borrow::Cow,
    collections::{BTreeSet, HashMap},
    io::{self, Write},
};

const MAX_DEPTH: usize = 8;

struct ResolutionHasher(Sha256);

impl Default for ResolutionHasher {
    fn default() -> Self {
        Self(Sha256::new())
    }
}

impl Write for ResolutionHasher {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.update(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl ResolutionHasher {
    fn finish(self) -> [u8; 16] {
        let digest = self.0.finalize();
        let mut truncated = [0; 16];
        truncated.copy_from_slice(&digest[..16]);
        truncated
    }
}

pub(crate) struct Contracts<'a> {
    symbols: HashMap<(&'a str, &'a str), Vec<&'a Node>>,
    flows: HashMap<&'a str, Cow<'a, ValueFlowFacts>>,
    positions: HashMap<(&'a str, SourcePosition), Vec<&'a Node>>,
    typed_flows: Option<&'a FlowStore>,
}

impl<'a> Contracts<'a> {
    pub(crate) fn new_typed(facts: &'a TypedFacts) -> Self {
        Self::with_flows(&facts.facts, Some(&facts.flows))
    }

    /// Hashes resolution-relevant node identities as a sorted set of fixed-size digests.
    pub(crate) fn resolution_digest(&self, facts: &'a Facts) -> anyhow::Result<BTreeSet<[u8; 16]>> {
        let mut digests = BTreeSet::new();
        for (index, node) in facts
            .nodes
            .iter()
            .enumerate()
            .filter(|(_, node)| node.kind != "component")
        {
            let mut hasher = ResolutionHasher::default();
            serde_json::to_writer(
                &mut hasher,
                &(
                    &node.id,
                    &node.kind,
                    &node.name,
                    &node.qualname,
                    &node.path,
                    node.is_test,
                    &node.language,
                    node.generated,
                    self.details(node, facts.semantic.state(index)),
                ),
            )?;
            digests.insert(hasher.finish());
        }
        Ok(digests)
    }

    fn with_flows(facts: &'a Facts, typed_flows: Option<&'a FlowStore>) -> Self {
        let mut symbols: HashMap<_, Vec<_>> = HashMap::new();
        let mut positions: HashMap<_, Vec<_>> = HashMap::new();
        let mut flows = HashMap::new();
        for node in &facts.nodes {
            symbols
                .entry((node.path.as_str(), node.qualname.as_str()))
                .or_default()
                .push(node);
            positions
                .entry((node.path.as_str(), position(node)))
                .or_default()
                .push(node);
            if let Some(store) = typed_flows {
                if let Some(flow) = store.get(node) {
                    flows.insert(node.id.as_str(), Cow::Borrowed(flow));
                }
            } else if let Some(value) = node.details.get("value_flow") {
                if let Ok(flow) = ValueFlowFacts::deserialize(value) {
                    flows.insert(node.id.as_str(), Cow::Owned(flow));
                }
            }
        }
        Self {
            symbols,
            flows,
            positions,
            typed_flows,
        }
    }

    pub(crate) fn details(
        &self,
        node: &'a Node,
        semantic: Option<&crate::core::typed_facts::SemanticState>,
    ) -> Value {
        let mut details = node.details.clone();
        if let Some(metadata) = node.native_semantic.as_deref() {
            if let Value::Object(view) =
                serde_json::to_value(metadata.view()).expect("semantic DTO serializes")
            {
                if !details.is_object() {
                    details = Value::Object(Default::default());
                }
                details
                    .as_object_mut()
                    .expect("details DTO is an object")
                    .extend(view);
            }
        }
        if let Some(state) = semantic {
            details["semantic_facts"] =
                serde_json::to_value(state.view()).expect("semantic facts serialize");
        }
        if let Some(store) = self.typed_flows {
            let uncertainty = store.named_uncertainties(&node.id);
            if !uncertainty.is_empty() {
                details["named_definition_uncertainties"] = json!(uncertainty);
            }
            if store.named_uncertainty_fault(&node.id) {
                details["named_definition_uncertainty_fault"] = json!(true);
            }
        }
        if !matches!(node.kind.as_str(), "function" | "method") {
            return self.raw_details(node, details);
        }
        let default_flow = ValueFlowFacts::default();
        let flow: &ValueFlowFacts = match self.flows.get(node.id.as_str()) {
            Some(flow) => flow,
            None if self.absent_flow(node) => &default_flow,
            None => return self.raw_details(node, details),
        };
        if !valid_scope_facts(node, flow, SemanticLimits::default()) {
            return self.raw_details(node, details);
        }
        // Empty flows still need the callable contract shape in incremental digests.
        if !details.is_object() {
            details = Value::Object(Default::default());
        }
        let metadata = details
            .as_object_mut()
            .expect("projected node details are an object");
        let mut projected = serde_json::Map::new();
        if flow.annotation_references().any(|fact| fact.value_query) {
            projected.insert(
                "annotation_references".into(),
                json!(flow.annotation_references().collect::<Vec<_>>()),
            );
        }
        for (key, value, empty) in [
            (
                "symbol_attachments",
                json!(flow.symbol_attachments().collect::<Vec<_>>()),
                flow.symbol_attachments().next().is_none(),
            ),
            (
                "factory_results",
                json!(flow.factory_results().collect::<Vec<_>>()),
                flow.factory_results().next().is_none(),
            ),
            (
                "named_bindings",
                json!(flow.named_bindings().collect::<Vec<_>>()),
                flow.named_bindings().next().is_none(),
            ),
            (
                "endpoint_requests",
                json!(flow.endpoint_requests().collect::<Vec<_>>()),
                flow.endpoint_requests().next().is_none(),
            ),
        ] {
            if !empty {
                projected.insert(key.into(), value);
            }
        }
        if let Some(return_type) = &flow.return_type {
            projected.insert("return_type".into(), json!(return_type));
        }
        if let Some(alias_type) = &flow.alias_type {
            projected.insert("alias_type".into(), json!(alias_type));
        }
        projected.insert("fields".into(), Value::Array(flow.fields.iter().map(|field| {
            let contract = if field.conditional {
                json!({"kind": "unknown"})
            } else {
                self.value_contract(node, flow, &field.value, field.position, &mut Vec::new(), 0)
            };
            json!({"name": field.name, "conditional": field.conditional, "contract": contract})
        }).collect()));
        projected.insert(
            "return_contract".into(),
            self.callable(node, flow, &mut Vec::new(), 0),
        );
        metadata.insert("value_flow".into(), Value::Object(projected));
        metadata.remove("bindings");
        metadata.remove("rebindings");
        details
    }

    fn absent_flow(&self, node: &Node) -> bool {
        if let Some(store) = self.typed_flows {
            store.state(node).is_none()
        } else {
            node.details.get("value_flow").is_none_or(Value::is_null)
        }
    }

    fn raw_details(&self, node: &Node, mut details: Value) -> Value {
        if let Some(state) = self.typed_flows.and_then(|store| store.state(node)) {
            let value = match state {
                FlowState::Valid {
                    facts,
                    original: None,
                    ..
                } if facts.is_empty() => None,
                FlowState::Valid {
                    facts,
                    original: None,
                    ..
                } if node.kind == "module" && node.language == "vue" => {
                    let mut stripped = facts.clone();
                    stripped.facts.retain(|f| {
                        !matches!(f, crate::core::semantic::FrameworkFact::ComponentMacro(_))
                    });
                    if stripped.is_empty() {
                        None
                    } else {
                        Some(json!(stripped))
                    }
                }
                FlowState::Valid {
                    original: Some(value),
                    ..
                } if node.kind == "module" && node.language == "vue" => {
                    let mut stripped = value.clone();
                    if let Some(obj) = stripped.as_object_mut() {
                        obj.remove("component_macros");
                    }
                    Some(stripped)
                }
                FlowState::Valid {
                    original: Some(value),
                    ..
                }
                | FlowState::Invalid(value) => Some(value.clone()),
                FlowState::Valid {
                    facts,
                    original: None,
                    ..
                } => Some(json!(facts)),
            };
            if let Some(value) = value {
                if !details.is_object() {
                    details = Value::Object(Default::default());
                }
                details
                    .as_object_mut()
                    .expect("stored node details are an object")
                    .insert("value_flow".into(), value);
            }
        }
        details
    }

    fn callable(
        &self,
        node: &'a Node,
        flow: &ValueFlowFacts,
        stack: &mut Vec<&'a str>,
        depth: usize,
    ) -> Value {
        if depth >= MAX_DEPTH || stack.contains(&node.id.as_str()) {
            return json!({"opaque_callable": nominal(node), "return_type": flow.return_type,
                "return_value": flow.return_value, "bindings": opaque_bindings(flow)});
        }
        stack.push(node.id.as_str());
        let at = flow.return_position.unwrap_or(SourcePosition {
            line: node.end_line,
            column: usize::MAX,
        });
        let contract = if let Some(ty) = &flow.return_type {
            self.type_contract(node, ty, stack, depth + 1)
        } else if let Some(value) = &flow.return_value {
            self.value_contract(node, flow, value, at, stack, depth + 1)
        } else {
            json!({"kind": "unknown"})
        };
        stack.pop();
        contract
    }

    fn type_contract(
        &self,
        owner: &Node,
        ty: &TypeExpr,
        stack: &mut Vec<&'a str>,
        depth: usize,
    ) -> Value {
        if depth >= MAX_DEPTH {
            return json!({"opaque_type": ty});
        }
        match ty {
            TypeExpr::Unknown => json!({"kind": "unknown"}),
            TypeExpr::Applied { base, args } => json!({"applied": base, "args": args.iter()
                .map(|arg| self.type_contract(owner, arg, stack, depth + 1)).collect::<Vec<_>>()}),
            TypeExpr::Named { name } => {
                if let Some(node) = self.local(owner, name, position(owner)) {
                    if matches!(
                        node.kind.as_str(),
                        "class" | "struct" | "enum" | "interface"
                    ) {
                        return nominal(node);
                    }
                    if node.kind == "type" && !stack.contains(&node.id.as_str()) {
                        if let Some(alias) = self
                            .flows
                            .get(node.id.as_str())
                            .and_then(|flow| flow.alias_type.as_ref())
                        {
                            stack.push(node.id.as_str());
                            let result = self.type_contract(node, alias, stack, depth + 1);
                            stack.pop();
                            return result;
                        }
                    }
                }
                if self.builtin_unshadowed(owner, name)
                    && languages::by_id(&owner.language)
                        .is_some_and(|profile| profile.builtin_type(name))
                {
                    json!({"builtin": name})
                } else {
                    json!({"opaque_type": ty})
                }
            }
        }
    }

    fn value_contract(
        &self,
        owner: &Node,
        flow: &ValueFlowFacts,
        value: &ValueExpr,
        at: SourcePosition,
        stack: &mut Vec<&'a str>,
        depth: usize,
    ) -> Value {
        if depth >= MAX_DEPTH {
            return json!({"opaque_value": value, "bindings": opaque_bindings(flow)});
        }
        match value {
            ValueExpr::Unknown => json!({"kind": "unknown"}),
            ValueExpr::LoopElement { .. } => json!({"opaque_value": value}),
            ValueExpr::Scoped { value, body_end } => {
                if at < *body_end {
                    self.value_contract(owner, flow, value, at, stack, depth + 1)
                } else {
                    json!({"kind": "unknown"})
                }
            }
            ValueExpr::BranchJoin { alternatives } => json!({
                "branch_join": {
                    "max_candidate_branch_joins": super::value_flow::MAX_CANDIDATE_BRANCH_JOINS,
                    "alternatives": alternatives.iter().map(|alternative| self.value_contract(
                        owner,
                        flow,
                        alternative,
                        at,
                        stack,
                        depth + 1,
                    )).collect::<Vec<_>>(),
                }
            }),
            ValueExpr::Await { value } => json!({"await": self.value_contract(
                owner,
                flow,
                value,
                at,
                stack,
                depth + 1,
            )}),
            ValueExpr::Annotated { type_expr } => {
                self.type_contract(owner, type_expr, stack, depth + 1)
            }
            ValueExpr::Alias { name } => {
                let latest = flow
                    .bindings
                    .iter()
                    .filter(|binding| binding.name == *name && binding.position < at
                        && !matches!(&binding.value, ValueExpr::Scoped { body_end, .. } if at >= *body_end))
                    .max_by_key(|binding| binding.position);
                if let Some(binding) = latest {
                    if binding.conditional
                        || flow
                            .bindings
                            .iter()
                            .filter(|other| {
                                other.name == *name && other.position == binding.position
                            })
                            .count()
                            > 1
                    {
                        return json!({"kind": "unknown"});
                    }
                    self.value_contract(
                        owner,
                        flow,
                        &binding.value,
                        binding.position,
                        stack,
                        depth + 1,
                    )
                } else {
                    json!({"opaque_value": value})
                }
            }
            ValueExpr::Call {
                site: Some(site), ..
            } => {
                let mut legacy = value.clone();
                if let ValueExpr::Call { site, .. } = &mut legacy {
                    *site = None;
                }
                json!({"callsite":site,"contract":self.value_contract(owner,flow,&legacy,at,stack,depth+1)})
            }
            ValueExpr::Call { callee, .. } | ValueExpr::Construct { callee } => {
                let head = callee.split('.').next().unwrap_or(callee);
                if let Some(binding) = flow
                    .bindings
                    .iter()
                    .filter(|binding| binding.name == head && binding.position < at)
                    .max_by_key(|binding| binding.position)
                {
                    return json!({"opaque_value": value, "binding":
                        self.value_contract(owner, flow, &binding.value, binding.position, stack, depth + 1),
                        "conditional": binding.conditional});
                }
                let Some(target) = self.local(owner, callee, at) else {
                    if matches!(value, ValueExpr::Construct { .. })
                        && self.builtin_unshadowed(owner, callee)
                        && languages::by_id(&owner.language)
                            .is_some_and(|profile| profile.builtin_type(callee))
                    {
                        return json!({"builtin": callee});
                    }
                    return json!({"opaque_value": value});
                };
                if matches!(
                    target.kind.as_str(),
                    "class" | "struct" | "enum" | "interface"
                ) && (matches!(value, ValueExpr::Construct { .. }) || owner.language == "python")
                {
                    nominal(target)
                } else if matches!(value, ValueExpr::Call { .. })
                    && matches!(target.kind.as_str(), "function" | "method")
                {
                    match self.flows.get(target.id.as_str()) {
                        Some(target_flow) => self.callable(target, target_flow, stack, depth + 1),
                        None if self.absent_flow(target) => {
                            json!({"kind": "unknown"})
                        }
                        None => json!({"opaque_value": value}),
                    }
                } else {
                    json!({"opaque_value": value})
                }
            }
            ValueExpr::Object { members, .. } => {
                let mut contracts = std::collections::BTreeMap::new();
                for member in members {
                    let Some([target]) = self
                        .positions
                        .get(&(owner.path.as_str(), member.position))
                        .map(Vec::as_slice)
                    else {
                        return json!({"opaque_value": value});
                    };
                    let Some(target_flow) = self.flows.get(target.id.as_str()) else {
                        return json!({"opaque_value": value});
                    };
                    if !matches!(target.kind.as_str(), "function" | "method")
                        || contracts.contains_key(&member.name)
                    {
                        return json!({"opaque_value": value});
                    }
                    contracts.insert(
                        &member.name,
                        self.callable(target, target_flow, stack, depth + 1),
                    );
                }
                json!({"object": contracts})
            }
            ValueExpr::Field { receiver, .. } => json!({"opaque_value": value, "receiver":
                self.value_contract(owner, flow, &ValueExpr::Alias { name: receiver.clone() }, at, stack, depth + 1)}),
            ValueExpr::Project { value: source, .. } => json!({"opaque_value": value, "source":
                self.value_contract(owner, flow, source, at, stack, depth + 1)}),
        }
    }

    fn local(&self, owner: &Node, name: &str, at: SourcePosition) -> Option<&'a Node> {
        let head = name.split('.').next()?;
        let mut scope = owner.qualname.as_str();
        let mut lookup_key = String::with_capacity(128);
        loop {
            if let Some(nodes) = self.symbols.get(&(owner.path.as_str(), scope)) {
                if nodes
                    .iter()
                    .any(|node| node.rebinding_names().any(|candidate| candidate == head))
                {
                    return None;
                }
            }
            if let Some(nodes) = self.symbols.get(&(
                owner.path.as_str(),
                super::qualified(&mut lookup_key, scope, name),
            )) {
                let mut visible = nodes.iter().copied().filter(|node| {
                    node.language == owner.language
                        && (owner.language != "python"
                            || !scope.contains('.')
                            || position(node) <= at)
                });
                let node = visible.next()?;
                return visible.next().is_none().then_some(node);
            }
            if self
                .symbols
                .get(&(owner.path.as_str(), scope))
                .is_some_and(|nodes| {
                    nodes
                        .iter()
                        .any(|node| node.binding_names().any(|candidate| candidate == head))
                })
            {
                return None;
            }
            match scope.rsplit_once('.') {
                Some((parent, _)) => scope = parent,
                None => break,
            }
        }
        let [node] = self.symbols.get(&(owner.path.as_str(), name))?.as_slice() else {
            return None;
        };
        (node.language == owner.language).then_some(*node)
    }

    fn builtin_unshadowed(&self, owner: &Node, name: &str) -> bool {
        let head = name.split('.').next().unwrap_or(name);
        let mut scope = owner.qualname.as_str();
        let mut lookup_key = String::with_capacity(128);
        loop {
            if self.symbols.contains_key(&(
                owner.path.as_str(),
                super::qualified(&mut lookup_key, scope, head),
            )) {
                return false;
            }
            if self
                .symbols
                .get(&(owner.path.as_str(), scope))
                .is_some_and(|nodes| {
                    nodes.iter().any(|node| {
                        node.binding_names()
                            .chain(node.rebinding_names())
                            .any(|candidate| candidate == head)
                            || self.flows.get(node.id.as_str()).is_some_and(|flow| {
                                flow.bindings.iter().any(|binding| binding.name == head)
                            })
                    })
                })
            {
                return false;
            }
            match scope.rsplit_once('.') {
                Some((parent, _)) => scope = parent,
                None => break,
            }
        }
        !self.symbols.contains_key(&(owner.path.as_str(), head))
    }
}

fn position(node: &Node) -> SourcePosition {
    SourcePosition {
        line: node.line,
        column: node.details["column"].as_u64().unwrap_or(0) as usize,
    }
}

fn nominal(node: &Node) -> Value {
    json!({"nominal": {"path": node.path, "qualname": node.qualname,
        "kind": node.kind, "language": node.language}})
}

fn opaque_bindings(flow: &ValueFlowFacts) -> Vec<Value> {
    flow.bindings
        .iter()
        .map(|binding| {
            json!({"name": binding.name,
        "value": binding.value, "conditional": binding.conditional})
        })
        .collect()
}

#[cfg(test)]
mod resolution_digest_tests {
    use super::Contracts;
    use crate::core::{
        models::{Facts, Node},
        typed_facts::TypedFacts,
    };
    use serde_json::json;

    fn module(line: usize, details: serde_json::Value) -> Node {
        Node {
            native_semantic: None,
            id: "module:src/lib.py".into(),
            kind: "module".into(),
            name: "lib.py".into(),
            qualname: "lib".into(),
            path: "src/lib.py".into(),
            line,
            end_line: line,
            is_test: false,
            language: "python".into(),
            generated: false,
            details,
        }
    }

    fn component() -> Node {
        Node {
            native_semantic: None,
            id: "component:src".into(),
            kind: "component".into(),
            name: "src".into(),
            qualname: "src".into(),
            path: "src".into(),
            line: 0,
            end_line: 0,
            is_test: false,
            language: "unknown".into(),
            generated: false,
            details: serde_json::Value::Null,
        }
    }

    fn digest(nodes: Vec<Node>) -> std::collections::BTreeSet<[u8; 16]> {
        let typed = TypedFacts::from_public(Facts {
            nodes,
            ..Facts::default()
        });
        Contracts::new_typed(&typed)
            .resolution_digest(&typed.facts)
            .unwrap()
    }

    #[test]
    fn resolution_digest_is_order_stable_ignores_positions_and_tracks_details() {
        let details = json!({"exports": ["alpha"]});
        let first = digest(vec![module(2, details.clone())]);
        let reordered_with_component = digest(vec![component(), module(27, details)]);
        assert_eq!(first, reordered_with_component);

        let changed_details = digest(vec![module(2, json!({"exports": ["beta"]}))]);
        assert_ne!(first, changed_details);

        let origin = json!({"pack":"proof","rule":"projection"});
        let position = json!({"line":2,"column":3});
        for (vector, before, after) in [
            (
                "symbol_attachments",
                json!({"origin":origin,"position":position,"source":{"kind":"node","id":"callback"},"target":{"kind":"name","expression":"Model"},"relation":"uses_model"}),
                json!({"origin":origin,"position":position,"source":{"kind":"node","id":"callback"},"target":{"kind":"name","expression":"Alternate"},"relation":"uses_model"}),
            ),
            (
                "factory_results",
                json!({"origin":origin,"position":position,"binding":"first","members":[{"name":"run","position":position}]}),
                json!({"origin":origin,"position":position,"binding":"second","members":[{"name":"run","position":position}]}),
            ),
            (
                "named_bindings",
                json!({"origin":origin,"position":position,"domain":"renderer","key":{"kind":"literal","value":"cart"},"role":{"kind":"definition","target":{"kind":"name","expression":"drawA"},"target_kind":"callable"}}),
                json!({"origin":origin,"position":position,"domain":"renderer","key":{"kind":"literal","value":"cart"},"role":{"kind":"definition","target":{"kind":"name","expression":"drawB"},"target_kind":"callable"}}),
            ),
            (
                "endpoint_requests",
                json!({"origin":origin,"position":position,"url":{"kind":"literal","value":"/records"},"method":{"kind":"literal","value":"GET"}}),
                json!({"origin":origin,"position":position,"url":{"kind":"literal","value":"/records"},"method":{"kind":"literal","value":"POST"}}),
            ),
            (
                "named_bindings",
                json!({"origin":origin,"position":position,"domain":"filters","key":{"kind":"literal","value":"decorate"},"role":{"kind":"registry_definition","receiver":{"kind":"name","expression":"env"},"target":{"kind":"name","expression":"first"}}}),
                json!({"origin":origin,"position":position,"domain":"filters","key":{"kind":"literal","value":"decorate"},"role":{"kind":"registry_definition","receiver":{"kind":"name","expression":"other"},"target":{"kind":"name","expression":"first"}}}),
            ),
            (
                "named_bindings",
                json!({"origin":origin,"position":position,"domain":"filters","key":{"kind":"literal","value":"decorate"},"role":{"kind":"registry_definition","receiver":{"kind":"name","expression":"env"},"target":{"kind":"name","expression":"first"}}}),
                json!({"origin":origin,"position":position,"domain":"filters","key":{"kind":"literal","value":"decorate"},"role":{"kind":"registry_definition","receiver":{"kind":"name","expression":"env"},"target":{"kind":"name","expression":"second"}}}),
            ),
        ] {
            let callable = |fact| {
                let mut node = module(2, json!({"value_flow":{vector:[fact]}}));
                node.id = "function:src/lib.py:callback".into();
                node.kind = "function".into();
                node.name = "callback".into();
                node.qualname = "lib.callback".into();
                node
            };
            assert_ne!(
                digest(vec![callable(before)]),
                digest(vec![callable(after)]),
                "{vector}: callable projection affects canonical identity"
            );
        }
    }
}
