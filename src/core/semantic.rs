//! Language-neutral evidence for statically inferred receiver types.

use serde::{Deserialize, Serialize};

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
    },
    /// Value obtained from a receiver member.
    Field {
        /// Receiver expression.
        receiver: String,
        /// Accessed member name.
        member: String,
    },
    /// Object value with known members.
    Object {
        /// Members retained by bounded value flow.
        members: Vec<ObjectMember>,
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

fn is_false(value: &bool) -> bool {
    !value
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default, deny_unknown_fields)]
/// Represents value flow facts data.
pub struct ValueFlowFacts {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    /// The bindings value.
    pub bindings: Vec<BindingFact>,
    #[serde(skip_serializing_if = "Option::is_none")]
    /// Optional return type value.
    pub return_type: Option<TypeExpr>,
    #[serde(skip_serializing_if = "Option::is_none")]
    /// Optional return value value.
    pub return_value: Option<ValueExpr>,
    #[serde(skip_serializing_if = "Option::is_none")]
    /// Optional return position value.
    pub return_position: Option<SourcePosition>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    /// The fields value.
    pub fields: Vec<FieldFact>,
    #[serde(skip_serializing_if = "Option::is_none")]
    /// Optional alias type value.
    pub alias_type: Option<TypeExpr>,
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
                    visit(base);
                    for argument in args.iter().take(16) {
                        type_keys(argument, visit, depth + 1);
                    }
                }
                TypeExpr::Unknown => {}
            }
        }
        fn value_keys<'a>(value: &'a ValueExpr, visit: &mut impl FnMut(&'a str)) {
            match value {
                ValueExpr::Annotated { type_expr } => type_keys(type_expr, visit, 0),
                ValueExpr::Construct { callee } => {
                    if python_literal_type(callee).is_none() {
                        visit(callee);
                    }
                }
                ValueExpr::Call { callee } => visit(callee),
                ValueExpr::Alias { name } => visit(name),
                ValueExpr::Field { receiver, member } => {
                    visit(receiver);
                    visit(member);
                }
                ValueExpr::Object { .. } | ValueExpr::Unknown => {}
            }
        }
        for ty in self.return_type.iter().chain(self.alias_type.iter()) {
            type_keys(ty, &mut visit, 0);
        }
        for value in self
            .return_value
            .iter()
            .chain(self.bindings.iter().map(|fact| &fact.value))
            .chain(self.fields.iter().map(|fact| &fact.value))
        {
            value_keys(value, &mut visit);
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
        fn value_keys<'a>(value: &'a serde_json::Value, visit: &mut impl FnMut(&'a str)) {
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
                Some("field") => &["receiver", "member"],
                _ => &[],
            };
            for field in fields {
                if let Some(name) = value.get(field).and_then(serde_json::Value::as_str) {
                    visit(name);
                }
            }
        }
        for key in ["return_type", "alias_type"] {
            if let Some(ty) = flow.get(key) {
                type_key(ty, &mut visit);
            }
        }
        if let Some(value) = flow.get("return_value") {
            value_keys(value, &mut visit);
        }
        for key in ["bindings", "fields"] {
            if let Some(facts) = flow.get(key).and_then(serde_json::Value::as_array) {
                for fact in facts {
                    if let Some(value) = fact.get("value") {
                        value_keys(value, &mut visit);
                    }
                }
            }
        }
    }

    /// Reports whether empty applies.
    pub fn is_empty(&self) -> bool {
        self.bindings.is_empty()
            && self.return_type.is_none()
            && self.return_value.is_none()
            && self.fields.is_empty()
            && self.alias_type.is_none()
    }

    /// Performs offset lines.
    pub fn offset_lines(&mut self, offset: usize) {
        if let Some(position) = &mut self.return_position {
            position.line = position.line.saturating_add(offset);
        }
        for position in self
            .bindings
            .iter_mut()
            .map(|fact| &mut fact.position)
            .chain(self.fields.iter_mut().map(|fact| &mut fact.position))
        {
            position.line = position.line.saturating_add(offset);
        }
        for member in self
            .bindings
            .iter_mut()
            .map(|fact| &mut fact.value)
            .chain(self.fields.iter_mut().map(|fact| &mut fact.value))
            .chain(self.return_value.iter_mut())
            .filter_map(|value| match value {
                ValueExpr::Object { members } => Some(members),
                _ => None,
            })
            .flatten()
        {
            member.position.line = member.position.line.saturating_add(offset);
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
        assert!(serde_json::from_str::<ValueFlowFacts>(r#"{"guessed_type":"Worker"}"#).is_err());
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
}
