use super::{Coverage, Edge, Graph};
use hashbrown::HashMap;
use std::sync::Arc;

pub(super) enum GraphPart {
    Resident(Graph),
    Packed {
        bytes: Vec<u8>,
        edges: usize,
        coverage: usize,
    },
}

impl GraphPart {
    pub(super) fn new(graph: Graph, pack: bool) -> Self {
        if !pack {
            return Self::Resident(graph);
        }
        let mut bytes = Vec::new();
        for edge in &graph.edges {
            for value in [
                edge.src.as_str(),
                &edge.dst,
                edge.kind.as_str(),
                edge.path.as_ref(),
                edge.evidence.as_str(),
                edge.confidence.as_str(),
            ] {
                write_string(&mut bytes, value);
            }
            bytes.extend_from_slice(&(edge.line as u64).to_le_bytes());
        }
        for row in &graph.coverage {
            for value in [
                row.path.as_ref(),
                &row.expression,
                row.status.as_str(),
                row.evidence.as_ref(),
            ] {
                write_string(&mut bytes, value);
            }
            bytes.extend_from_slice(&(row.line as u64).to_le_bytes());
        }
        Self::Packed {
            bytes: zstd::stream::encode_all(bytes.as_slice(), 1)
                .expect("in-memory graph compression"),
            edges: graph.edges.len(),
            coverage: graph.coverage.len(),
        }
    }

    pub(super) fn counts(&self) -> (usize, usize) {
        match self {
            Self::Resident(graph) => (graph.edges.len(), graph.coverage.len()),
            Self::Packed {
                edges, coverage, ..
            } => (*edges, *coverage),
        }
    }

    pub(super) fn into_graph(self) -> Graph {
        let (bytes, edges, coverage) = match self {
            Self::Packed {
                bytes,
                edges,
                coverage,
            } => (bytes, edges, coverage),
            Self::Resident(graph) => return graph,
        };
        let bytes =
            zstd::stream::decode_all(bytes.as_slice()).expect("graph compression is lossless");
        let mut bytes = bytes.as_slice();
        let mut path_pool = HashMap::<&str, Arc<str>>::new();
        let edges = (0..edges)
            .map(|_| {
                let src = read_string(&mut bytes);
                let dst = read_string(&mut bytes);
                let kind = read_str(&mut bytes).into();
                let path = intern_path(&mut path_pool, read_str(&mut bytes));
                let evidence = read_str(&mut bytes).into();
                let confidence = read_str(&mut bytes).into();
                let line = read_number(&mut bytes);
                Edge {
                    src,
                    dst,
                    kind,
                    path,
                    line,
                    evidence,
                    confidence,
                }
            })
            .collect();
        let coverage = (0..coverage)
            .map(|_| {
                let path = intern_path(&mut path_pool, read_str(&mut bytes));
                let expression = read_string(&mut bytes);
                let status = read_str(&mut bytes).into();
                let evidence = read_string(&mut bytes).into();
                let line = read_number(&mut bytes);
                Coverage {
                    path,
                    line,
                    expression,
                    status,
                    evidence,
                }
            })
            .collect();
        let graph = Graph {
            edges,
            coverage,
            path_arc: None,
            path_pool: HashMap::new(),
        };
        debug_assert!(bytes.is_empty());
        graph
    }
}

fn intern_path<'a>(path_pool: &mut HashMap<&'a str, Arc<str>>, path: &'a str) -> Arc<str> {
    path_pool
        .entry(path)
        .or_insert_with(|| Arc::from(path))
        .clone()
}

fn write_string(bytes: &mut Vec<u8>, value: &str) {
    bytes.extend_from_slice(&(value.len() as u64).to_le_bytes());
    bytes.extend_from_slice(value.as_bytes());
}

fn read_number(bytes: &mut &[u8]) -> usize {
    let (number, rest) = bytes.split_at(8);
    *bytes = rest;
    u64::from_le_bytes(number.try_into().expect("eight-byte graph field")) as usize
}

fn read_string(bytes: &mut &[u8]) -> String {
    read_str(bytes).to_owned()
}

fn read_str<'a>(bytes: &mut &'a [u8]) -> &'a str {
    let length = read_number(bytes);
    let (value, rest) = bytes.split_at(length);
    *bytes = rest;
    std::str::from_utf8(value).expect("graph strings were encoded internally")
}
