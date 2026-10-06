use super::value_flow::{valid_scope_facts, SemanticLimits};
use crate::core::models::{Facts, Node};
use crate::core::semantic::{SourcePosition, TypeExpr, ValueExpr, ValueFlowFacts};
use crate::core::typed_facts::{FlowState, FlowStore, TypedFacts};
use crate::engine::languages;
use serde::Deserialize;
use serde_json::{json, Value};
use std::{borrow::Cow, collections::HashMap};

const MAX_DEPTH: usize = 8;

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

    pub(crate) fn details(&self, node: &'a Node) -> Value {
        let mut details = node.details.clone();
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
        let Some(metadata) = details.as_object_mut() else {
            return details;
        };
        let mut projected = serde_json::Map::new();
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
        if node.kind == "module" && node.language == "vue" {
            if let Some(metadata) = details.as_object_mut() {
                metadata.remove("vue_script_setup");
            }
        }
        if let Some(state) = self.typed_flows.and_then(|store| store.state(node)) {
            if let Some(metadata) = details.as_object_mut() {
                let value = match state {
                    FlowState::Valid {
                        original: Some(value),
                        ..
                    }
                    | FlowState::Invalid(value) => value.clone(),
                    FlowState::Valid {
                        facts,
                        original: None,
                        ..
                    } => json!(facts),
                };
                metadata.insert("value_flow".into(), value);
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
            ValueExpr::Call { callee } | ValueExpr::Construct { callee } => {
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
        loop {
            let qualified = format!("{scope}.{name}");
            if let Some(nodes) = self.symbols.get(&(owner.path.as_str(), scope)) {
                if nodes.iter().any(|node| {
                    node.details["rebindings"]
                        .as_array()
                        .is_some_and(|names| names.iter().any(|name| name.as_str() == Some(head)))
                }) {
                    return None;
                }
            }
            if let Some(nodes) = self.symbols.get(&(owner.path.as_str(), qualified.as_str())) {
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
                    nodes.iter().any(|node| {
                        node.details["bindings"].as_array().is_some_and(|names| {
                            names.iter().any(|name| name.as_str() == Some(head))
                        })
                    })
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
        loop {
            let qualified = format!("{scope}.{head}");
            if self
                .symbols
                .contains_key(&(owner.path.as_str(), qualified.as_str()))
            {
                return false;
            }
            if self
                .symbols
                .get(&(owner.path.as_str(), scope))
                .is_some_and(|nodes| {
                    nodes.iter().any(|node| {
                        ["bindings", "rebindings"].iter().any(|key| {
                            node.details[*key].as_array().is_some_and(|names| {
                                names.iter().any(|name| name.as_str() == Some(head))
                            })
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
