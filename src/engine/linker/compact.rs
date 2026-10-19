use super::{Coverage, Edge, Graph};

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
                &edge.path,
                &edge.evidence,
                edge.confidence.as_str(),
            ] {
                write_string(&mut bytes, value);
            }
            bytes.extend_from_slice(&(edge.line as u64).to_le_bytes());
        }
        for row in &graph.coverage {
            for value in [
                row.path.as_str(),
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
        let graph = Graph {
            edges: (0..edges)
                .map(|_| Edge {
                    src: read_string(&mut bytes),
                    dst: read_string(&mut bytes),
                    kind: read_str(&mut bytes).into(),
                    path: read_string(&mut bytes),
                    evidence: read_string(&mut bytes),
                    confidence: read_str(&mut bytes).into(),
                    line: read_number(&mut bytes),
                })
                .collect(),
            coverage: (0..coverage)
                .map(|_| Coverage {
                    path: read_string(&mut bytes),
                    expression: read_string(&mut bytes),
                    status: read_str(&mut bytes).into(),
                    evidence: read_string(&mut bytes).into(),
                    line: read_number(&mut bytes),
                })
                .collect(),
        };
        debug_assert!(bytes.is_empty());
        graph
    }
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
