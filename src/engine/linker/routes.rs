use crate::core::models::Node;

struct RouteEntry<'a> {
    node: &'a Node,
    method: &'a str,
    segments: Vec<String>,
}

pub(super) struct RouteIndex<'a> {
    entries: Vec<RouteEntry<'a>>,
}

impl<'a> RouteIndex<'a> {
    pub(super) fn build(nodes: &[&'a Node]) -> Self {
        let entries = nodes
            .iter()
            .copied()
            .filter(|node| node.kind == "route")
            .map(|node| {
                let method = node.details["method"].as_str().unwrap_or("ANY");
                let path = node.details["route_path"].as_str().unwrap_or("");
                RouteEntry {
                    node,
                    method,
                    segments: normalize_route_segments(path),
                }
            })
            .collect();
        Self { entries }
    }

    pub(super) fn match_route(
        &self,
        client_method: &str,
        client_url: &str,
    ) -> Result<Option<&'a Node>, ()> {
        let (remote, client_segments) = normalize_client_url(client_url);
        if remote {
            return Ok(None);
        }
        let mut candidates = Vec::new();
        for route in &self.entries {
            if route.method != "ANY"
                && client_method != "ANY"
                && !route.method.eq_ignore_ascii_case(client_method)
            {
                continue;
            }
            if route.segments.len() != client_segments.len() {
                continue;
            }
            let matches = route.segments.iter().zip(&client_segments).all(
                |(route_segment, client_segment)| {
                    route_segment == "{param}"
                        || client_segment == "{param}"
                        || route_segment == client_segment
                },
            );
            if matches {
                candidates.push(route.node);
            }
        }
        if candidates.len() == 1 {
            Ok(Some(candidates[0]))
        } else if candidates.len() > 1 {
            Err(())
        } else {
            Ok(None)
        }
    }
}

pub(super) fn normalize_route_segments(path: &str) -> Vec<String> {
    let trimmed = path.trim_start_matches('^').trim_matches('/');
    if trimmed.is_empty() {
        return Vec::new();
    }
    trimmed
        .split('/')
        .filter(|segment| !segment.is_empty())
        .map(|segment| {
            if (segment.starts_with('<') && segment.ends_with('>'))
                || segment.starts_with(':')
                || (segment.starts_with('{') && segment.ends_with('}'))
            {
                "{param}".to_string()
            } else {
                segment.to_ascii_lowercase()
            }
        })
        .collect()
}

pub(super) fn normalize_client_url(url: &str) -> (bool, Vec<String>) {
    if url.starts_with("http://") || url.starts_with("https://") || url.starts_with("//") {
        return (true, Vec::new());
    }
    let path_only = url.split(['?', '#']).next().unwrap_or(url);
    let trimmed = path_only.trim_matches('/');
    let segments = trimmed
        .split('/')
        .filter(|segment| !segment.is_empty())
        .map(|segment| {
            if segment.chars().all(|character| character.is_ascii_digit())
                || segment.starts_with("${")
                || (segment.len() == 36 && segment.contains('-'))
            {
                "{param}".to_string()
            } else {
                segment.to_ascii_lowercase()
            }
        })
        .collect();
    (false, segments)
}
