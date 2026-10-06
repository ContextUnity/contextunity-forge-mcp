use super::text;
use crate::engine::languages;
use anyhow::{bail, Context, Result};
use serde_json::json;
use tree_sitter::{Node as Syntax, Parser, Tree};

/// The search match horizon value.
pub const SEARCH_MATCH_HORIZON: usize = 10_000;

struct SearchBudget {
    deadline: Option<std::time::Instant>,
    remaining: usize,
    exhausted: bool,
}

impl SearchBudget {
    fn unbounded() -> Self {
        Self {
            deadline: None,
            remaining: 0,
            exhausted: false,
        }
    }

    fn bounded(query_deadline: std::time::Instant) -> Self {
        let file_deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        Self {
            deadline: Some(query_deadline.min(file_deadline)),
            remaining: 1_000_000,
            exhausted: false,
        }
    }

    fn step(&mut self) -> bool {
        let Some(deadline) = self.deadline else {
            return true;
        };
        if self.exhausted || self.remaining == 0 || std::time::Instant::now() >= deadline {
            self.exhausted = true;
            return false;
        }
        self.remaining -= 1;
        true
    }

    fn prepare_parser(&mut self, parser: &mut Parser) -> bool {
        if !self.step() {
            return false;
        }
        if let Some(deadline) = self.deadline {
            parser.set_timeout_micros(
                deadline
                    .saturating_duration_since(std::time::Instant::now())
                    .as_micros()
                    .max(1) as u64,
            );
        }
        true
    }
}

fn structural_match(
    pattern: Syntax<'_>,
    target: Syntax<'_>,
    ps: &str,
    source: &str,
    captures: &mut serde_json::Map<String, serde_json::Value>,
    budget: &mut SearchBudget,
) -> bool {
    if !budget.step() {
        return false;
    }
    let token = text(pattern, ps);
    if token.starts_with("__FORGE_META_") && pattern.named_child_count() == 0 {
        let name = token.trim_start_matches("__FORGE_META_");
        let value = json!(text(target, source));
        return match captures.get(name) {
            Some(old) => *old == value,
            None => {
                captures.insert(name.into(), value);
                true
            }
        };
    }
    if pattern.kind() != target.kind() {
        return false;
    }
    if pattern.child_count() == 0 {
        return token == text(target, source);
    }
    let mut pc = pattern.walk();
    let p: Vec<_> = pattern
        .children(&mut pc)
        .filter(|n| !n.kind().contains("comment"))
        .collect();
    let mut tc = target.walk();
    let t: Vec<_> = target
        .children(&mut tc)
        .filter(|n| !n.kind().contains("comment"))
        .collect();
    fn sequence(
        p: &[Syntax<'_>],
        t: &[Syntax<'_>],
        ps: &str,
        ts: &str,
        caps: &mut serde_json::Map<String, serde_json::Value>,
        budget: &mut SearchBudget,
    ) -> bool {
        if !budget.step() {
            return false;
        }
        if p.is_empty() {
            return t.is_empty();
        }
        let token = text(p[0], ps);
        if token.starts_with("__FORGE_MANY_") {
            for count in 0..=t.len() {
                if !budget.step() {
                    return false;
                }
                let mut branch = caps.clone();
                let name = token.trim_start_matches("__FORGE_MANY_");
                let value = if count == 0 {
                    ""
                } else {
                    &ts[t[0].start_byte()..t[count - 1].end_byte()]
                };
                if branch.get(name).is_some_and(|v| v != value) {
                    continue;
                }
                branch.insert(name.into(), json!(value));
                if sequence(&p[1..], &t[count..], ps, ts, &mut branch, budget) {
                    *caps = branch;
                    return true;
                }
            }
            return false;
        }
        if t.is_empty() || !structural_match(p[0], t[0], ps, ts, caps, budget) {
            return false;
        }
        sequence(&p[1..], &t[1..], ps, ts, caps, budget)
    }
    sequence(&p, &t, ps, source, captures, budget)
}

/// Performs search.
pub fn search(
    source: &str,
    path: &str,
    language: &str,
    pattern: &str,
    limit: usize,
) -> Result<Vec<serde_json::Value>> {
    Ok(search_range(
        source,
        path,
        language,
        pattern,
        0,
        limit,
        SearchBudget::unbounded(),
    )?
    .items)
}

/// Represents search page data.
pub struct SearchPage {
    /// The items value.
    pub items: Vec<serde_json::Value>,
    /// The matched value.
    pub matched: usize,
    /// Whether complete applies.
    pub complete: bool,
    /// Whether work limited applies.
    pub work_limited: bool,
}

impl SearchPage {
    fn limited(items: Vec<serde_json::Value>, matched: usize) -> Self {
        Self {
            items,
            matched,
            complete: false,
            work_limited: true,
        }
    }
}

/// Performs search page.
pub fn search_page(
    source: &str,
    path: &str,
    language: &str,
    pattern: &str,
    offset: usize,
    limit: usize,
    deadline: std::time::Instant,
) -> Result<SearchPage> {
    anyhow::ensure!(
        limit > 0
            && offset
                .checked_add(limit)
                .is_some_and(|end| end <= SEARCH_MATCH_HORIZON),
        "AST search offset + limit must be 1..=10000; narrow path or pattern"
    );
    search_range(
        source,
        path,
        language,
        pattern,
        offset,
        limit,
        SearchBudget::bounded(deadline),
    )
}

fn normalize_search_pattern(pattern: &str) -> String {
    let mut normalized = String::new();
    let mut chars = pattern.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '$' {
            let mut count = 1;
            while chars.peek() == Some(&'$') {
                chars.next();
                count += 1;
            }
            normalized.push_str(if count > 1 {
                "__FORGE_MANY_"
            } else {
                "__FORGE_META_"
            });
        } else {
            normalized.push(c);
        }
    }
    normalized
}

pub(crate) fn pattern_symbol_name(pattern: &str, language: &str) -> Result<Option<String>> {
    let profile = languages::require(language)?;
    let mut normalized = normalize_search_pattern(pattern);
    profile.prepare_pattern(&mut normalized);
    let mut parser = profile.create_parser("__forge_pattern__")?;
    let tree = parser.parse(&normalized, None).context("invalid pattern")?;
    anyhow::ensure!(
        !tree.root_node().has_error(),
        "pattern is not valid {language} syntax"
    );
    let mut node = tree.root_node();
    while node.named_child_count() == 1 && profile.pattern_wrapper(node.kind()) {
        node = node.named_child(0).context("empty pattern")?;
    }
    if profile.symbol_with_source(node, &normalized).is_none() {
        return Ok(None);
    }
    Ok(profile
        .symbol_name(node, &normalized)
        .filter(|name| !name.contains("__FORGE_"))
        .map(str::to_owned))
}

fn search_range(
    source: &str,
    path: &str,
    language: &str,
    pattern: &str,
    offset: usize,
    limit: usize,
    mut budget: SearchBudget,
) -> Result<SearchPage> {
    let bounded = budget.deadline.is_some();
    let mut normalized = normalize_search_pattern(pattern);
    let profile = languages::require(language)?;
    let partial_body = profile.prepare_pattern(&mut normalized);
    let mut parser = profile.create_parser(path)?;
    if !budget.prepare_parser(&mut parser) {
        return Ok(SearchPage::limited(Vec::new(), 0));
    }
    let pt: Tree = match parser.parse(&normalized, None) {
        Some(tree) => tree,
        None if bounded => return Ok(SearchPage::limited(Vec::new(), 0)),
        None => bail!("invalid pattern"),
    };
    if pt.root_node().has_error() {
        bail!("pattern is not valid {language} syntax");
    }
    let mut pn = pt.root_node();
    while pn.named_child_count() == 1 && profile.pattern_wrapper(pn.kind()) {
        pn = pn.named_child(0).context("empty pattern")?;
    }
    if !budget.prepare_parser(&mut parser) {
        return Ok(SearchPage::limited(Vec::new(), 0));
    }
    let tree = match parser.parse(source, None) {
        Some(tree) => tree,
        None if bounded => return Ok(SearchPage::limited(Vec::new(), 0)),
        None => bail!("parse cancelled"),
    };
    let mut stack = vec![tree.root_node()];
    let mut matches = Vec::new();
    let mut matched_count = 0;
    let mut visited = 0;
    while let Some(n) = stack.pop() {
        visited += 1;
        if !budget.step() || (bounded && visited > 100_000) {
            return Ok(SearchPage::limited(matches, matched_count));
        }
        let mut captures = serde_json::Map::new();
        let matched = if partial_body && n.kind() == pn.kind() {
            let mut pc = pn.walk();
            let mut nc = n.walk();
            let a: Vec<_> = pn
                .children(&mut pc)
                .filter(|x| x.kind() != "block")
                .collect();
            let b: Vec<_> = n
                .children(&mut nc)
                .filter(|x| x.kind() != "block")
                .collect();
            a.len() == b.len()
                && a.iter().zip(b).all(|(a, b)| {
                    structural_match(*a, b, &normalized, source, &mut captures, &mut budget)
                })
        } else {
            structural_match(pn, n, &normalized, source, &mut captures, &mut budget)
        };
        if budget.exhausted {
            return Ok(SearchPage::limited(matches, matched_count));
        }
        if matched {
            matched_count += 1;
            if matched_count > offset {
                if matches.len() == limit {
                    return Ok(SearchPage {
                        items: matches,
                        matched: matched_count,
                        complete: false,
                        work_limited: false,
                    });
                }
                matches.push(json!({"path":path,"line":n.start_position().row+1,"end_line":n.end_position().row+1,"text":text(n,source),"captures":captures}));
                if !bounded && matches.len() >= limit {
                    return Ok(SearchPage {
                        items: matches,
                        matched: matched_count,
                        complete: false,
                        work_limited: false,
                    });
                }
            }
        }
        let mut c = n.walk();
        let children: Vec<_> = n.named_children(&mut c).collect();
        stack.extend(children.into_iter().rev());
    }
    Ok(SearchPage {
        items: matches,
        matched: matched_count,
        complete: true,
        work_limited: false,
    })
}
