use super::text;
use crate::engine::languages::{self, AstSearchCapabilities, LanguageProfile};
use anyhow::{bail, Context, Result};
use serde::Serialize;
use serde_json::json;
use std::{fmt, ops::Range};
use tree_sitter::{Node as Syntax, Parser, Tree};

/// The search match horizon value.
pub const SEARCH_MATCH_HORIZON: usize = 10_000;

struct SearchBudget {
    deadline: Option<std::time::Instant>,
    remaining: usize,
    exhausted: bool,
}

/// Byte and line coordinates for an unsupported AST pattern.
#[derive(Debug, Serialize)]
pub struct PatternErrorSpan {
    /// Zero-based byte offset in the submitted pattern.
    pub start_byte: usize,
    /// Exclusive zero-based byte offset in the submitted pattern.
    pub end_byte: usize,
    /// One-based starting line in the submitted pattern.
    pub start_line: usize,
    /// One-based starting column in the submitted pattern.
    pub start_column: usize,
    /// One-based ending line in the submitted pattern.
    pub end_line: usize,
    /// One-based ending column in the submitted pattern.
    pub end_column: usize,
}

/// Structured diagnostic returned when a pattern is outside its profile grammar.
#[derive(Debug, Serialize)]
pub struct PatternSearchDiagnostic {
    /// Stable machine-readable diagnostic code.
    pub code: &'static str,
    /// Registered language profile that rejected the pattern.
    pub language: String,
    /// Exact Tree-sitter error range mapped to the submitted pattern.
    pub span: PatternErrorSpan,
    /// Profile-specific next step for repairing or rewriting the pattern.
    pub hint: &'static str,
    /// Valid examples declared by the profile.
    pub examples: Vec<&'static str>,
}

impl fmt::Display for PatternSearchDiagnostic {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "AST pattern is unsupported for {} at bytes {}..{}: {}",
            self.language, self.span.start_byte, self.span.end_byte, self.hint
        )
    }
}

impl std::error::Error for PatternSearchDiagnostic {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PatternMode {
    Complete,
    Declaration,
    Fragment,
    Attribute,
}

struct NormalizedPattern {
    source: String,
    original_ranges: Vec<Range<usize>>,
    original_len: usize,
}

struct PreparedPattern {
    source: String,
    tree: Tree,
    mode: PatternMode,
    fragment_range: Option<Range<usize>>,
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

struct StructuralMatchOptions<'a> {
    capabilities: &'a AstSearchCapabilities,
    declaration_root: bool,
}

fn structural_match(
    pattern: Syntax<'_>,
    target: Syntax<'_>,
    ps: &str,
    source: &str,
    captures: &mut serde_json::Map<String, serde_json::Value>,
    budget: &mut SearchBudget,
    options: StructuralMatchOptions<'_>,
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
    let omit_declaration_body = options
        .capabilities
        .declaration_kinds
        .contains(&pattern.kind())
        && (options.declaration_root
            || pattern.named_children(&mut pattern.walk()).any(|child| {
                options
                    .capabilities
                    .declaration_body_kinds
                    .contains(&child.kind())
                    && child
                        .named_children(&mut child.walk())
                        .all(|nested| nested.kind().contains("comment"))
            }));
    let mut pc = pattern.walk();
    let p: Vec<_> = pattern
        .children(&mut pc)
        .filter(|n| {
            !n.kind().contains("comment")
                && !(omit_declaration_body
                    && options
                        .capabilities
                        .declaration_body_kinds
                        .contains(&n.kind()))
        })
        .collect();
    let mut tc = target.walk();
    let t: Vec<_> = target
        .children(&mut tc)
        .filter(|n| {
            !n.kind().contains("comment")
                && !(omit_declaration_body
                    && options
                        .capabilities
                        .declaration_body_kinds
                        .contains(&n.kind()))
        })
        .collect();
    fn sequence(
        p: &[Syntax<'_>],
        t: &[Syntax<'_>],
        ps: &str,
        ts: &str,
        caps: &mut serde_json::Map<String, serde_json::Value>,
        budget: &mut SearchBudget,
        capabilities: &AstSearchCapabilities,
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
                if sequence(
                    &p[1..],
                    &t[count..],
                    ps,
                    ts,
                    &mut branch,
                    budget,
                    capabilities,
                ) {
                    *caps = branch;
                    return true;
                }
            }
            return false;
        }
        if t.is_empty()
            || !structural_match(
                p[0],
                t[0],
                ps,
                ts,
                caps,
                budget,
                StructuralMatchOptions {
                    capabilities,
                    declaration_root: false,
                },
            )
        {
            return false;
        }
        sequence(&p[1..], &t[1..], ps, ts, caps, budget, capabilities)
    }
    sequence(&p, &t, ps, source, captures, budget, options.capabilities)
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

fn normalize_search_pattern(pattern: &str) -> NormalizedPattern {
    let mut normalized = NormalizedPattern {
        source: String::new(),
        original_ranges: Vec::new(),
        original_len: pattern.len(),
    };
    let mut chars = pattern.char_indices().peekable();
    while let Some((start, character)) = chars.next() {
        if character == '$' {
            let mut count = 1;
            let mut token_end = start + character.len_utf8();
            while chars.peek().is_some_and(|(_, next)| *next == '$') {
                let (offset, dollar) = chars.next().expect("peeked dollar");
                token_end = offset + dollar.len_utf8();
                count += 1;
            }
            let name_start = token_end;
            for (offset, next) in pattern[token_end..].char_indices() {
                if !(next.is_ascii_alphanumeric() || next == '_') {
                    break;
                }
                token_end = name_start + offset + next.len_utf8();
            }
            let marker = if count > 1 {
                "__FORGE_MANY_"
            } else {
                "__FORGE_META_"
            };
            normalized.source.push_str(marker);
            normalized
                .original_ranges
                .extend((0..marker.len()).map(|_| start..token_end));
        } else {
            normalized.source.push(character);
            normalized
                .original_ranges
                .extend((start..start + character.len_utf8()).map(|byte| byte..byte + 1));
        }
    }
    normalized
}

impl NormalizedPattern {
    fn original_range(&self, range: Range<usize>) -> Range<usize> {
        if range.start >= range.end {
            let offset = self
                .original_ranges
                .get(range.start)
                .map_or(self.original_len, |origin| origin.start);
            return offset..offset;
        }
        let start = self
            .original_ranges
            .get(range.start)
            .map_or(self.original_len, |origin| origin.start);
        let end = self
            .original_ranges
            .get(range.end - 1)
            .map_or(self.original_len, |origin| origin.end);
        start..end
    }
}

fn source_position(source: &str, byte: usize) -> (usize, usize) {
    let mut line = 1;
    let mut column = 1;
    for byte in source.as_bytes().iter().take(byte.min(source.len())) {
        if *byte == b'\n' {
            line += 1;
            column = 1;
        } else {
            column += 1;
        }
    }
    (line, column)
}

fn pattern_error_span(
    tree: &Tree,
    normalized: &NormalizedPattern,
    original: &str,
) -> PatternErrorSpan {
    let mut stack = vec![tree.root_node()];
    let mut selected: Option<Range<usize>> = None;
    while let Some(node) = stack.pop() {
        if node.is_error() && node.start_byte() < node.end_byte() {
            let range = node.start_byte()..node.end_byte();
            if selected
                .as_ref()
                .is_none_or(|current| range.len() < current.len())
            {
                selected = Some(range);
            }
        }
        let mut cursor = node.walk();
        stack.extend(node.children(&mut cursor));
    }
    let normalized_range = selected.unwrap_or(tree.root_node().byte_range());
    let original_range = normalized.original_range(normalized_range);
    let start_byte = original_range.start.min(original.len());
    let end_byte = original_range.end.min(original.len()).max(start_byte);
    let (start_line, start_column) = source_position(original, start_byte);
    let (end_line, end_column) = source_position(original, end_byte);
    PatternErrorSpan {
        start_byte,
        end_byte,
        start_line,
        start_column,
        end_line,
        end_column,
    }
}

fn pattern_diagnostic(
    profile: &dyn LanguageProfile,
    tree: &Tree,
    normalized: &NormalizedPattern,
    original: &str,
) -> PatternSearchDiagnostic {
    let capabilities = profile.ast_search_capabilities();
    PatternSearchDiagnostic {
        code: "unsupported_ast_pattern",
        language: profile.id().to_owned(),
        span: pattern_error_span(tree, normalized, original),
        hint: capabilities.diagnostic_hint,
        examples: capabilities.valid_examples.to_vec(),
    }
}

fn parse_pattern_candidate(
    parser: &mut Parser,
    source: &str,
    budget: &mut SearchBudget,
) -> Result<Option<Tree>> {
    if !budget.prepare_parser(parser) {
        return Ok(None);
    }
    Ok(parser.parse(source, None))
}

fn unwrap_pattern_node<'tree>(
    mut node: Syntax<'tree>,
    profile: &dyn LanguageProfile,
) -> Option<Syntax<'tree>> {
    while node.named_child_count() == 1 && profile.pattern_wrapper(node.kind()) {
        node = node.named_child(0)?;
    }
    Some(node)
}

fn exact_fragment_node<'tree>(tree: &'tree Tree, range: &Range<usize>) -> Option<Syntax<'tree>> {
    let mut stack = vec![tree.root_node()];
    while let Some(node) = stack.pop() {
        if node.start_byte() == range.start && node.end_byte() == range.end {
            return Some(node);
        }
        let mut cursor = node.walk();
        stack.extend(node.children(&mut cursor));
    }
    None
}

impl PreparedPattern {
    fn node(&self, profile: &dyn LanguageProfile) -> Option<Syntax<'_>> {
        let node = match &self.fragment_range {
            Some(range) => exact_fragment_node(&self.tree, range)?,
            None => self.tree.root_node(),
        };
        unwrap_pattern_node(node, profile)
    }
}

fn prepare_search_pattern(
    parser: &mut Parser,
    profile: &dyn LanguageProfile,
    pattern: &str,
    budget: &mut SearchBudget,
) -> Result<Option<PreparedPattern>> {
    let normalized = normalize_search_pattern(pattern);
    let Some(tree) = parse_pattern_candidate(parser, &normalized.source, budget)? else {
        return Ok(None);
    };
    let capabilities = profile.ast_search_capabilities();
    if !tree.root_node().has_error() {
        let root = unwrap_pattern_node(tree.root_node(), profile);
        if root
            .filter(|node| {
                capabilities.declaration_kinds.contains(&node.kind())
                    && !node
                        .named_children(&mut node.walk())
                        .any(|child| capabilities.declaration_body_kinds.contains(&child.kind()))
            })
            .is_some()
        {
            for suffix in capabilities.declaration_completions {
                let candidate_source = format!("{}{suffix}", normalized.source);
                let Some(candidate_tree) =
                    parse_pattern_candidate(parser, &candidate_source, budget)?
                else {
                    return Ok(None);
                };
                if candidate_tree.root_node().has_error() {
                    continue;
                }
                let Some(candidate_root) = unwrap_pattern_node(candidate_tree.root_node(), profile)
                else {
                    continue;
                };
                if capabilities
                    .declaration_kinds
                    .contains(&candidate_root.kind())
                {
                    return Ok(Some(PreparedPattern {
                        source: candidate_source,
                        tree: candidate_tree,
                        mode: PatternMode::Declaration,
                        fragment_range: None,
                    }));
                }
            }
        }
        let mode = root.map_or(PatternMode::Complete, |node| {
            if capabilities.attribute_kinds.contains(&node.kind()) {
                PatternMode::Attribute
            } else if capabilities.declaration_kinds.contains(&node.kind()) {
                PatternMode::Declaration
            } else {
                PatternMode::Complete
            }
        });
        return Ok(Some(PreparedPattern {
            source: normalized.source,
            tree,
            mode,
            fragment_range: None,
        }));
    }

    for suffix in capabilities.declaration_completions {
        let candidate_source = format!("{}{suffix}", normalized.source);
        let Some(candidate_tree) = parse_pattern_candidate(parser, &candidate_source, budget)?
        else {
            return Ok(None);
        };
        if candidate_tree.root_node().has_error() {
            continue;
        }
        let Some(node) = unwrap_pattern_node(candidate_tree.root_node(), profile) else {
            continue;
        };
        if capabilities.declaration_kinds.contains(&node.kind()) {
            return Ok(Some(PreparedPattern {
                source: candidate_source,
                tree: candidate_tree,
                mode: PatternMode::Declaration,
                fragment_range: None,
            }));
        }
    }

    if let Some(probe) = capabilities.fragment_probe {
        let mut candidate_source = String::with_capacity(
            probe.prefix.len() + normalized.source.len() + probe.suffix.len(),
        );
        candidate_source.push_str(probe.prefix);
        let fragment_start =
            candidate_source.len() + normalized.source.len() - normalized.source.trim_start().len();
        candidate_source.push_str(&normalized.source);
        let fragment_end =
            candidate_source.len() - (normalized.source.len() - normalized.source.trim_end().len());
        candidate_source.push_str(probe.suffix);
        let Some(candidate_tree) = parse_pattern_candidate(parser, &candidate_source, budget)?
        else {
            return Ok(None);
        };
        if !candidate_tree.root_node().has_error() {
            let range = fragment_start..fragment_end;
            if exact_fragment_node(&candidate_tree, &range).is_some() {
                return Ok(Some(PreparedPattern {
                    source: candidate_source,
                    tree: candidate_tree,
                    mode: PatternMode::Fragment,
                    fragment_range: Some(range),
                }));
            }
        }
    }

    Err(anyhow::Error::new(pattern_diagnostic(
        profile,
        &tree,
        &normalized,
        pattern,
    )))
}

pub(crate) fn pattern_symbol_name(pattern: &str, language: &str) -> Result<Option<String>> {
    let profile = languages::require(language)?;
    let mut parser = profile.create_parser("__forge_pattern__")?;
    let mut budget = SearchBudget::unbounded();
    let prepared = match prepare_search_pattern(&mut parser, profile, pattern, &mut budget) {
        Ok(Some(prepared)) => prepared,
        Ok(None) => return Ok(None),
        Err(error) if error.downcast_ref::<PatternSearchDiagnostic>().is_some() => {
            return Ok(None);
        }
        Err(error) => return Err(error),
    };
    let Some(node) = prepared.node(profile) else {
        return Ok(None);
    };
    if profile.symbol_with_source(node, &prepared.source).is_none() {
        return Ok(None);
    }
    Ok(profile
        .symbol_name(node, &prepared.source)
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
    let profile = languages::require(language)?;
    let mut parser = profile.create_parser(path)?;
    let Some(prepared) = prepare_search_pattern(&mut parser, profile, pattern, &mut budget)? else {
        return Ok(SearchPage::limited(Vec::new(), 0));
    };
    let pn = prepared
        .node(profile)
        .context("search pattern root is empty")?;
    let capabilities = profile.ast_search_capabilities();
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
        let matched = structural_match(
            pn,
            n,
            &prepared.source,
            source,
            &mut captures,
            &mut budget,
            StructuralMatchOptions {
                capabilities: &capabilities,
                declaration_root: prepared.mode == PatternMode::Declaration,
            },
        );
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
