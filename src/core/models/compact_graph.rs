use super::{Coverage as PublicCoverage, Edge as PublicEdge, Graph as PublicGraph};
use serde::{Serialize, Serializer};
use std::cmp::Ordering;

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
    pub path: String,
    pub line: usize,
    pub evidence: String,
    pub confidence: CompactTag,
}

impl From<&PublicEdge> for Edge {
    fn from(edge: &PublicEdge) -> Self {
        Self {
            src: edge.src.clone(),
            dst: edge.dst.clone(),
            kind: edge.kind.as_str().into(),
            path: edge.path.clone(),
            line: edge.line,
            evidence: edge.evidence.clone(),
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
            path: edge.path,
            line: edge.line,
            evidence: edge.evidence,
            confidence: edge.confidence.into_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct Coverage {
    pub path: String,
    pub line: usize,
    pub expression: String,
    pub status: CompactTag,
    pub evidence: String,
}

impl From<Coverage> for PublicCoverage {
    fn from(coverage: Coverage) -> Self {
        Self {
            path: coverage.path,
            line: coverage.line,
            expression: coverage.expression,
            status: coverage.status.into_string(),
            evidence: coverage.evidence,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct Graph {
    pub edges: Vec<Edge>,
    pub coverage: Vec<Coverage>,
}

impl From<Graph> for PublicGraph {
    fn from(graph: Graph) -> Self {
        Self {
            edges: graph.edges.into_iter().map(Into::into).collect(),
            coverage: graph.coverage.into_iter().map(Into::into).collect(),
        }
    }
}
