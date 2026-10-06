use crate::core::models::{Facts, Node, Reference};
use hashbrown::{HashMap, HashSet};
use std::collections::BTreeMap;

#[derive(Clone, Debug)]
pub(crate) enum Base<'a> {
    Local(&'a str),
    External(String, &'a str, usize),
    Unknown,
}

impl PartialEq for Base<'_> {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Local(left), Self::Local(right)) => left == right,
            (Self::External(left, ..), Self::External(right, ..)) => left == right,
            (Self::Unknown, Self::Unknown) => true,
            _ => false,
        }
    }
}
impl Eq for Base<'_> {}

#[derive(Clone, Copy)]
pub(crate) enum Member<'a> {
    Local(&'a Node),
    External(&'a str, usize, Option<&'static str>),
    Ambiguous,
    Unknown,
}

/// Python C3 order and member lookup are computed before linking references.
pub(crate) struct PythonReceivers<'a> {
    members: HashMap<&'a str, HashMap<&'a str, Member<'a>>>,
    supers: HashMap<&'a str, HashMap<&'a str, Member<'a>>>,
}

impl<'a> PythonReceivers<'a> {
    pub(crate) fn build<F: AsRef<Facts>>(
        all: &'a BTreeMap<String, F>,
        mut resolve: impl FnMut(&'a Node, &'a Reference) -> Base<'a>,
    ) -> Self {
        let classes: HashMap<&str, &Node> = all
            .values()
            .flat_map(|facts| &facts.as_ref().nodes)
            .filter(|node| node.language == "python" && node.kind == "class")
            .map(|node| (node.id.as_str(), node))
            .collect();
        let mut bases: HashMap<&str, Vec<Base<'a>>> = HashMap::new();
        let mut own: HashMap<&str, HashMap<&str, Member<'a>>> = HashMap::new();
        let mut class_names: HashMap<(&str, &str), Vec<&'a Node>> = HashMap::new();
        for class in classes.values() {
            class_names
                .entry((class.path.as_str(), class.qualname.as_str()))
                .or_default()
                .push(*class);
        }
        let find_class = |path: &str, qualname: &str, child: &Node| -> Option<&'a Node> {
            let candidates = class_names.get(&(path, qualname))?;
            if candidates.len() == 1 {
                return Some(candidates[0]);
            }
            if let Some(enclosing) = candidates
                .iter()
                .find(|class| class.line <= child.line && child.end_line <= class.end_line)
            {
                return Some(*enclosing);
            }
            let child_is_stub = child
                .details
                .get("is_stub")
                .and_then(|v| v.as_bool())
                .unwrap_or(false)
                || child
                    .details
                    .get("is_overload")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
            if let Some(matching) = candidates.iter().find(|class| {
                let class_is_stub = class
                    .details
                    .get("is_stub")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false)
                    || class
                        .details
                        .get("is_overload")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);
                class_is_stub == child_is_stub
            }) {
                return Some(*matching);
            }
            candidates.first().copied()
        };
        let mut owner_classes = HashMap::new();
        for facts in all.values() {
            let facts = facts.as_ref();
            let assigned_fields: HashSet<&str> = facts
                .edges
                .iter()
                .filter(|edge| edge.kind == "mutates")
                .map(|edge| edge.dst.as_str())
                .collect();
            for node in &facts.nodes {
                // Member reads create synthetic field nodes; they do not override methods.
                if node.kind == "field" && !assigned_fields.contains(node.id.as_str()) {
                    continue;
                }
                if let Some((parent, _)) = node.qualname.rsplit_once('.') {
                    if let Some(class) = find_class(node.path.as_str(), parent, node) {
                        let members = own.entry(class.id.as_str()).or_default();
                        let callable = matches!(node.kind.as_str(), "function" | "method");
                        let conditional = callable
                            && node
                                .details
                                .get("is_conditional")
                                .and_then(|value| value.as_bool())
                                .unwrap_or(false);
                        let node_is_stub = node
                            .details
                            .get("is_stub")
                            .and_then(|value| value.as_bool())
                            .unwrap_or(false)
                            || node
                                .details
                                .get("is_overload")
                                .and_then(|value| value.as_bool())
                                .unwrap_or(false);
                        match members.get(node.name.as_str()) {
                            None => {
                                members.insert(
                                    node.name.as_str(),
                                    if conditional {
                                        Member::Unknown
                                    } else {
                                        Member::Local(node)
                                    },
                                );
                            }
                            Some(Member::Local(existing)) => {
                                let existing_is_stub = existing
                                    .details
                                    .get("is_stub")
                                    .and_then(|v| v.as_bool())
                                    .unwrap_or(false)
                                    || existing
                                        .details
                                        .get("is_overload")
                                        .and_then(|v| v.as_bool())
                                        .unwrap_or(false);
                                if existing_is_stub && !node_is_stub {
                                    members.insert(node.name.as_str(), Member::Local(node));
                                } else if !existing_is_stub && node_is_stub {
                                    // keep existing non-stub
                                } else if existing_is_stub && node_is_stub {
                                    // both are stubs/overloads; keep existing stub so runtime implementation can replace it
                                } else if conditional || !callable {
                                    members.insert(node.name.as_str(), Member::Ambiguous);
                                } else {
                                    members.insert(node.name.as_str(), Member::Local(node));
                                }
                            }
                            Some(Member::Ambiguous | Member::Unknown)
                                if callable && !node_is_stub =>
                            {
                                members.insert(
                                    node.name.as_str(),
                                    if conditional {
                                        Member::Ambiguous
                                    } else {
                                        Member::Local(node)
                                    },
                                );
                            }
                            Some(Member::Ambiguous | Member::Unknown) => {}
                            Some(Member::External(..)) => {}
                        }
                    }
                    let mut scope = parent;
                    while !scope.is_empty() {
                        if let Some(class) = find_class(node.path.as_str(), scope, node) {
                            owner_classes.insert(node.id.as_str(), class);
                            break;
                        }
                        scope = scope.rsplit_once('.').map_or("", |(parent, _)| parent);
                    }
                }
            }
            for reference in &facts.references {
                if reference.kind == "inherits" {
                    if let Some(class) = classes.get(reference.source.as_str()) {
                        bases
                            .entry(class.id.as_str())
                            .or_default()
                            .push(resolve(class, reference));
                    }
                }
                if reference.kind == "mutates" {
                    if let Some(("self" | "cls", member)) = reference.expression.split_once('.') {
                        if let Some(class) = owner_classes.get(reference.source.as_str()) {
                            own.entry(class.id.as_str())
                                .or_default()
                                .insert(member, Member::Unknown);
                        }
                    }
                }
            }
        }
        for class in classes.values() {
            if let Some(bindings) = class.details["bindings"].as_array() {
                for binding in bindings.iter().filter_map(|binding| binding.as_str()) {
                    let member = own
                        .entry(class.id.as_str())
                        .or_default()
                        .entry(binding)
                        .or_insert(Member::Unknown);
                    if !matches!(member, Member::Local(node) if node.kind == "field") {
                        *member = Member::Unknown;
                    }
                }
            }
        }
        let mut orders = HashMap::new();
        for id in classes.keys() {
            linearize(id, &bases, &mut orders, &mut HashSet::new());
        }
        Self {
            members: own,
            supers: HashMap::new(),
        }
        .finish(&classes, &orders)
    }

    fn finish(
        mut self,
        classes: &HashMap<&'a str, &'a Node>,
        orders: &HashMap<&'a str, Option<Vec<Base<'a>>>>,
    ) -> Self {
        let own = std::mem::take(&mut self.members);
        for id in classes.keys() {
            let Some(Some(order)) = orders.get(id) else {
                if let Some(members) = own.get(id) {
                    self.members.insert(id, members.clone());
                }
                continue;
            };
            for super_call in [false, true] {
                let mut inherited = HashMap::new();
                for base in order.iter().skip(usize::from(super_call)) {
                    if let Base::Local(base_id) = base {
                        if let Some(members) = own.get(base_id) {
                            for (name, member) in members {
                                inherited.entry(*name).or_insert(*member);
                            }
                        }
                    } else if let Base::External(key, origin, line) = base {
                        if let Some((provider, members)) = framework_members(key) {
                            for name in members {
                                if name.starts_with("objects.") && inherited.contains_key("objects")
                                {
                                    continue;
                                }
                                inherited.entry(*name).or_insert(Member::External(
                                    origin,
                                    *line,
                                    Some(provider),
                                ));
                            }
                        }
                        // An unindexed base can override any later base's members.
                        break;
                    } else if matches!(base, Base::Unknown) {
                        break;
                    }
                }
                if super_call {
                    self.supers.insert(id, inherited);
                } else {
                    self.members.insert(id, inherited);
                }
            }
        }
        self
    }

    pub(crate) fn lookup(&self, class: &Node, member: &str, super_call: bool) -> Member<'a> {
        let map = if super_call {
            &self.supers
        } else {
            &self.members
        };
        if let Some((head, _)) = member.split_once('.') {
            if map
                .get(class.id.as_str())
                .is_some_and(|members| members.contains_key(head))
            {
                return Member::Unknown;
            }
        }
        if let Some(result) = map
            .get(class.id.as_str())
            .and_then(|members| members.get(member))
        {
            return *result;
        }
        Member::Unknown
    }
}

fn framework_members(base: &str) -> Option<(&'static str, &'static [&'static str])> {
    match base {
        "django.core.management.base.BaseCommand" => Some((
            "django.core.management.base.BaseCommand",
            &[
                "stdout.write",
                "stderr.write",
                "add_arguments",
                "execute",
                "run_from_argv",
                "check",
                "check_migrations",
                "print_help",
                "create_parser",
            ],
        )),
        "django.http.HttpRequest" | "django.http.request.HttpRequest" => Some((
            "django.http.HttpRequest",
            &[
                "GET.get",
                "GET.items",
                "GET.keys",
                "GET.values",
                "POST.get",
                "POST.items",
                "POST.keys",
                "POST.values",
            ],
        )),
        "unittest.TestCase"
        | "unittest.case.TestCase"
        | "django.test.TestCase"
        | "django.test.testcases.TestCase" => Some((
            "TestCase",
            &[
                "assertEqual",
                "assertNotEqual",
                "assertTrue",
                "assertFalse",
                "assertIs",
                "assertIsNot",
                "assertIsNone",
                "assertIsNotNone",
                "assertIn",
                "assertNotIn",
                "assertIsInstance",
                "assertNotIsInstance",
                "assertRaises",
                "assertRaisesRegex",
                "assertWarns",
                "assertWarnsRegex",
                "assertLogs",
                "assertNoLogs",
                "assertAlmostEqual",
                "assertNotAlmostEqual",
                "assertGreater",
                "assertGreaterEqual",
                "assertLess",
                "assertLessEqual",
                "assertRegex",
                "assertNotRegex",
                "assertCountEqual",
                "assertSequenceEqual",
                "assertListEqual",
                "assertTupleEqual",
                "assertSetEqual",
                "assertDictEqual",
                "fail",
                "skipTest",
                "addCleanup",
                "doCleanups",
                "subTest",
            ],
        )),
        "django.db.models.Model" | "django.db.models.base.Model" => Some((
            "django.db.models.Model",
            &[
                "objects.all",
                "objects.filter",
                "objects.exclude",
                "objects.get",
                "objects.create",
                "objects.get_or_create",
                "objects.update_or_create",
                "objects.bulk_create",
                "objects.bulk_update",
                "objects.count",
                "objects.exists",
                "objects.first",
                "objects.last",
                "objects.order_by",
                "objects.values",
                "objects.values_list",
                "objects.select_related",
                "objects.prefetch_related",
                "objects.aggregate",
                "objects.annotate",
                "objects.none",
                "objects.update",
                "objects.delete",
                "objects.using",
            ],
        )),
        "pydantic.BaseModel" | "pydantic.main.BaseModel" => Some((
            "pydantic.BaseModel",
            &[
                "model_validate",
                "model_validate_json",
                "model_dump",
                "model_dump_json",
                "model_copy",
                "model_construct",
                "model_json_schema",
            ],
        )),
        _ => None,
    }
}

fn linearize<'a>(
    id: &'a str,
    bases: &HashMap<&'a str, Vec<Base<'a>>>,
    cache: &mut HashMap<&'a str, Option<Vec<Base<'a>>>>,
    active: &mut HashSet<&'a str>,
) -> Option<Vec<Base<'a>>> {
    if let Some(order) = cache.get(id) {
        return order.clone();
    }
    if !active.insert(id) {
        return None;
    }
    let direct = bases.get(id).cloned().unwrap_or_default();
    let mut sequences = Vec::new();
    let mut valid = true;
    for base in &direct {
        match base {
            Base::Local(base_id) => match linearize(base_id, bases, cache, active) {
                Some(order) => sequences.push(order),
                None => valid = false,
            },
            Base::External(..) | Base::Unknown => sequences.push(vec![base.clone()]),
        }
    }
    sequences.push(direct);
    let mut order = vec![Base::Local(id)];
    while valid && sequences.iter().any(|sequence| !sequence.is_empty()) {
        let head = sequences
            .iter()
            .filter_map(|sequence| sequence.first())
            .find(|head| {
                !sequences
                    .iter()
                    .any(|sequence| sequence.iter().skip(1).any(|item| item == *head))
            })
            .cloned();
        let Some(head) = head else {
            valid = false;
            break;
        };
        order.push(head.clone());
        for sequence in &mut sequences {
            if sequence.first() == Some(&head) {
                sequence.remove(0);
            }
        }
    }
    active.remove(id);
    let result = valid.then_some(order);
    cache.insert(id, result.clone());
    result
}
