use super::{Coverage as PublicCoverage, Edge as PublicEdge, Graph as PublicGraph};
use serde::{Serialize, Serializer};
use std::borrow::Cow;
use std::cmp::Ordering;
use std::sync::Arc;

#[derive(Debug, Clone)]
pub(crate) enum CompactTag {
    Static(&'static str),
    Owned(Box<str>),
}

impl CompactTag {
    pub(crate) fn as_str(&self) -> &str {
        match self {
            Self::Static(value) => value,
            Self::Owned(value) => value,
        }
    }

    fn known(value: &str) -> Option<&'static str> {
        Some(match value {
            "calls" => "calls",
            "possible_calls" => "possible_calls",
            "imports" => "imports",
            "contains" => "contains",
            "references" => "references",
            "inherits" => "inherits",
            "implements" => "implements",
            "bases" => "bases",
            "decorates" => "decorates",
            "handles" => "handles",
            "mutates" => "mutates",
            "registers" => "registers",
            "exact" => "exact",
            "inferred" => "inferred",
            "possible" => "possible",
            "heuristic" => "heuristic",
            "resolved" => "resolved",
            "external" => "external",
            "ambiguous" => "ambiguous",
            "unresolved" => "unresolved",
            _ => return None,
        })
    }

    fn into_string(self) -> String {
        match self {
            Self::Static(value) => value.to_owned(),
            Self::Owned(value) => value.into_string(),
        }
    }
}

impl From<&str> for CompactTag {
    fn from(value: &str) -> Self {
        Self::known(value).map_or_else(|| Self::Owned(value.into()), Self::Static)
    }
}

impl From<String> for CompactTag {
    fn from(value: String) -> Self {
        Self::known(&value).map_or_else(|| Self::Owned(value.into_boxed_str()), Self::Static)
    }
}

impl Serialize for CompactTag {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl PartialEq for CompactTag {
    fn eq(&self, other: &Self) -> bool {
        self.as_str() == other.as_str()
    }
}

impl Eq for CompactTag {}

impl PartialOrd for CompactTag {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for CompactTag {
    fn cmp(&self, other: &Self) -> Ordering {
        self.as_str().cmp(other.as_str())
    }
}

impl PartialEq<&str> for CompactTag {
    fn eq(&self, other: &&str) -> bool {
        self.as_str() == *other
    }
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct Edge {
    pub src: String,
    pub dst: String,
    pub kind: CompactTag,
    pub path: Arc<str>,
    pub line: usize,
    pub evidence: CompactTag,
    pub confidence: CompactTag,
}

impl From<&PublicEdge> for Edge {
    fn from(edge: &PublicEdge) -> Self {
        let path = Arc::from(edge.path.as_str());
        Self::with_path(edge, &path)
    }
}

impl Edge {
    pub(crate) fn with_path(edge: &PublicEdge, path: &Arc<str>) -> Self {
        Self {
            src: edge.src.clone(),
            dst: edge.dst.clone(),
            kind: edge.kind.as_str().into(),
            path: Arc::clone(path),
            line: edge.line,
            evidence: edge.evidence.as_str().into(),
            confidence: edge.confidence.as_str().into(),
        }
    }
}

impl From<Edge> for PublicEdge {
    fn from(edge: Edge) -> Self {
        Self {
            src: edge.src,
            dst: edge.dst,
            kind: edge.kind.into_string(),
            path: edge.path.as_ref().to_owned(),
            line: edge.line,
            evidence: edge.evidence.into_string(),
            confidence: edge.confidence.into_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct Coverage {
    pub path: Arc<str>,
    pub line: usize,
    pub expression: String,
    pub status: CompactTag,
    pub evidence: Cow<'static, str>,
}

impl From<Coverage> for PublicCoverage {
    fn from(coverage: Coverage) -> Self {
        Self {
            path: coverage.path.as_ref().to_owned(),
            line: coverage.line,
            expression: coverage.expression,
            status: coverage.status.into_string(),
            evidence: coverage.evidence.into_owned(),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct Graph {
    pub edges: Vec<Edge>,
    pub coverage: Vec<Coverage>,
    pub(crate) path_arc: Option<Arc<str>>,
    pub(crate) path_pool: hashbrown::HashMap<String, Arc<str>>,
}

impl Graph {
    fn shared_path(&mut self, path: &str) -> Arc<str> {
        if let Some(shared) = self.path_arc.as_ref().filter(|shared| shared.as_ref() == path) {
            return Arc::clone(shared);
        }
        if let Some(shared) = self.path_pool.get(path) {
            return Arc::clone(shared);
        }
        let shared = Arc::from(path);
        self.path_pool
            .insert(path.to_owned(), Arc::clone(&shared));
        shared
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn push_edge<S, D, K, E, C>(
        &mut self,
        path: &str,
        src: S,
        dst: D,
        kind: K,
        line: usize,
        evidence: E,
        confidence: C,
    ) where
        S: Into<String>,
        D: Into<String>,
        K: Into<CompactTag>,
        E: Into<CompactTag>,
        C: Into<CompactTag>,
    {
        let path = self.shared_path(path);
        self.edges.push(Edge {
            src: src.into(),
            dst: dst.into(),
            kind: kind.into(),
            path,
            line,
            evidence: evidence.into(),
            confidence: confidence.into(),
        });
    }

    pub(crate) fn push_coverage<P, S, E>(
        &mut self,
        path: &str,
        line: usize,
        expression: P,
        status: S,
        evidence: E,
    ) where
        P: Into<String>,
        S: Into<CompactTag>,
        E: Into<Cow<'static, str>>,
    {
        let path = self.shared_path(path);
        self.coverage.push(Coverage {
            path,
            line,
            expression: expression.into(),
            status: status.into(),
            evidence: evidence.into(),
        });
    }

    pub(crate) fn reference_edge(
        &mut self,
        path: &str,
        reference: &super::Reference,
        target: String,
        kind: &str,
        confidence: &str,
    ) {
        self.push_edge(
            path,
            reference.source.clone(),
            target,
            kind,
            reference.line,
            reference.expression.as_str(),
            confidence,
        );
    }

    pub(crate) fn reference_coverage(
        &mut self,
        path: &str,
        reference: &super::Reference,
        status: &str,
        evidence: Cow<'static, str>,
    ) {
        self.push_coverage(
            path,
            reference.line,
            reference.expression.clone(),
            status,
            evidence,
        );
    }
}

impl From<Graph> for PublicGraph {
    fn from(graph: Graph) -> Self {
        Self {
            edges: graph.edges.into_iter().map(Into::into).collect(),
            coverage: graph.coverage.into_iter().map(Into::into).collect(),
        }
    }
}
