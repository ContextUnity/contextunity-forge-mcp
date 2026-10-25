use crate::core::models::Node;
use crate::core::semantic::ExtractionRuleOrigin;
use crate::engine::languages::dependency_registry::DependencyRegistry;
use crate::engine::languages::manifests::syntax::SyntaxOperation;
use std::borrow::Cow;
use std::collections::{HashMap, HashSet};

pub(super) struct PreparedHttpTargetContext {
    request_owners: HashSet<String>,
    route_owners: HashSet<String>,
}

type RuleTargetContexts = HashMap<String, HashMap<String, PreparedHttpTargetContext>>;

/// Current, validated HTTP target policy for the endpoint request owners being linked.
pub(super) struct HttpTargetContexts {
    by_owner: HashMap<String, RuleTargetContexts>,
}

impl HttpTargetContexts {
    pub(super) fn build<'p>(
        paths: impl Iterator<Item = &'p str>,
        dependencies: &DependencyRegistry,
    ) -> Self {
        let mut by_owner = HashMap::<String, RuleTargetContexts>::new();
        for path in paths {
            let rules = dependencies.language_context_for_path(path);
            let mut owner_targets: Option<RuleTargetContexts> = None;
            for rule in &rules.syntax_rules {
                let SyntaxOperation::HttpClientCall {
                    target_context: Some(context),
                    ..
                } = &rule.operation
                else {
                    continue;
                };
                if !context.request_owners.iter().any(|owner| owner == path) {
                    continue;
                }
                owner_targets
                    .get_or_insert_with(HashMap::new)
                    .entry(rule.pack.clone())
                    .or_default()
                    .insert(
                        rule.id.clone(),
                        PreparedHttpTargetContext {
                            request_owners: context.request_owners.iter().cloned().collect(),
                            route_owners: context.route_owners.iter().cloned().collect(),
                        },
                    );
            }
            if let Some(targets) = owner_targets {
                by_owner.insert(path.to_owned(), targets);
            }
        }
        Self { by_owner }
    }

    pub(super) fn get(
        &self,
        request_owner: &str,
        origin: &ExtractionRuleOrigin,
    ) -> Option<&PreparedHttpTargetContext> {
        self.by_owner
            .get(request_owner)?
            .get(&origin.pack)?
            .get(&origin.rule)
    }
}

struct RouteEntry<'a> {
    node: &'a Node,
    method: &'a str,
    segments: Vec<Cow<'a, str>>,
}

pub(super) struct RouteIndex<'a> {
    entries: Vec<RouteEntry<'a>>,
    names: HashMap<&'a str, Vec<&'a Node>>,
}

fn single_candidate<'a>(candidates: &[&'a Node]) -> Result<Option<&'a Node>, ()> {
    match candidates {
        [route] => Ok(Some(*route)),
        [] => Ok(None),
        _ => Err(()),
    }
}

fn score_route_match(
    route_segments: &[Cow<'_, str>],
    client_segments: &[Cow<'_, str>],
) -> Option<usize> {
    if route_segments.len() != client_segments.len() {
        return None;
    }
    let mut score = 0;
    for (rs, cs) in route_segments.iter().zip(client_segments) {
        if rs == cs {
            score += if rs == "{param}" { 2 } else { 3 };
        } else if rs == "{param}" {
            score += 1;
        } else if cs == "{param}" {
            // cs is param, rs is not param: valid match with base score 0
        } else {
            return None;
        }
    }
    Some(score)
}

impl<'a> RouteIndex<'a> {
    pub(super) fn build(nodes: &[&'a Node]) -> Self {
        let mut entries = Vec::new();
        let mut names = HashMap::<&'a str, Vec<&'a Node>>::new();
        for node in nodes.iter().copied().filter(|node| node.kind == "route") {
            let method = node.details["method"].as_str().unwrap_or("ANY");
            let path = node.details["route_path"].as_str().unwrap_or("");
            entries.push(RouteEntry {
                node,
                method,
                segments: normalize_route_segments(path),
            });
            if let Some(name) = node.details["route_name"]
                .as_str()
                .filter(|name| !name.is_empty())
            {
                names.entry(name).or_default().push(node);
            }
        }
        Self { entries, names }
    }

    pub(super) fn match_route_name(&self, name: &str) -> Result<Option<&'a Node>, ()> {
        single_candidate(self.names.get(name).map(Vec::as_slice).unwrap_or_default())
    }

    pub(super) fn match_route(
        &self,
        client_method: &str,
        client_url: &str,
    ) -> Result<Option<&'a Node>, ()> {
        self.match_route_in_context(client_method, client_url, None, "")
    }

    pub(super) fn match_route_in_context(
        &self,
        client_method: &str,
        client_url: &str,
        context: Option<&PreparedHttpTargetContext>,
        request_owner: &str,
    ) -> Result<Option<&'a Node>, ()> {
        single_candidate(
            &self.matching_routes_in_context(client_method, client_url, context, request_owner),
        )
    }

    pub(super) fn matching_routes(&self, client_method: &str, client_url: &str) -> Vec<&'a Node> {
        self.matching_routes_in_context(client_method, client_url, None, "")
    }

    pub(super) fn matching_routes_in_context(
        &self,
        client_method: &str,
        client_url: &str,
        context: Option<&PreparedHttpTargetContext>,
        request_owner: &str,
    ) -> Vec<&'a Node> {
        let (remote, client_segments) = normalize_client_url(client_url);
        if remote {
            return Vec::new();
        }
        let context = context.filter(|context| context.request_owners.contains(request_owner));
        let mut candidates: Vec<(&'a Node, usize, bool)> = Vec::new();
        for route in &self.entries {
            if context.is_some_and(|context| !context.route_owners.contains(&route.node.path)) {
                continue;
            }
            if route.method != "ANY"
                && client_method != "ANY"
                && !route.method.eq_ignore_ascii_case(client_method)
            {
                continue;
            }
            if let Some(score) = score_route_match(&route.segments, &client_segments) {
                let exact_method =
                    client_method != "ANY" && route.method.eq_ignore_ascii_case(client_method);
                candidates.push((route.node, score, exact_method));
            }
        }
        if candidates.len() > 1 {
            if candidates.iter().any(|(_, _, exact)| *exact) {
                candidates.retain(|(_, _, exact)| *exact);
            }
            if let Some(max_score) = candidates.iter().map(|(_, score, _)| *score).max() {
                candidates.retain(|(_, score, _)| *score == max_score);
            }
        }
        candidates.into_iter().map(|(node, _, _)| node).collect()
    }
}

pub(super) fn is_remote_client_url(url: &str) -> bool {
    url.starts_with("http://") || url.starts_with("https://") || url.starts_with("//")
}

fn is_route_param(segment: &str) -> bool {
    (segment.starts_with('<') && segment.ends_with('>'))
        || segment.starts_with(':')
        || (segment.starts_with('{') && segment.ends_with('}'))
}

fn is_client_param(segment: &str) -> bool {
    segment.chars().all(|character| character.is_ascii_digit())
        || segment.starts_with("${")
        || (segment.starts_with("{{") && segment.ends_with("}}"))
        || (segment.len() == 36 && segment.contains('-'))
}

fn normalize_segment<'a>(segment: &'a str, is_param: fn(&str) -> bool) -> Cow<'a, str> {
    if is_param(segment) {
        Cow::Borrowed("{param}")
    } else if segment.bytes().all(|b| !b.is_ascii_uppercase()) {
        Cow::Borrowed(segment)
    } else {
        Cow::Owned(segment.to_ascii_lowercase())
    }
}

fn parse_segments<'a>(path: &'a str, is_param: fn(&str) -> bool) -> Vec<Cow<'a, str>> {
    let trimmed = path.trim_matches('/');
    if trimmed.is_empty() {
        return Vec::new();
    }
    trimmed
        .split('/')
        .filter(|segment| !segment.is_empty())
        .map(|segment| normalize_segment(segment, is_param))
        .collect()
}

pub(super) fn normalize_route_segments(path: &str) -> Vec<Cow<'_, str>> {
    parse_segments(path.trim_start_matches('^'), is_route_param)
}

pub(super) fn normalize_client_url(url: &str) -> (bool, Vec<Cow<'_, str>>) {
    if is_remote_client_url(url) {
        return (true, Vec::new());
    }
    let path_only = url.split(['?', '#']).next().unwrap_or(url);
    (false, parse_segments(path_only, is_client_param))
}
