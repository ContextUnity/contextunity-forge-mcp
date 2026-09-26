use crate::core::models::*;
use hashbrown::HashMap;
use rayon::prelude::*;
use std::collections::BTreeMap;
fn normalize_module(owner: &str, module: &str) -> String {
    let parent = owner.rsplit_once('/').map_or("", |(p, _)| p);
    if module.starts_with("./") || module.starts_with("../") {
        let mut parts: Vec<&str> = parent.split('/').filter(|p| !p.is_empty()).collect();
        for p in module.split('/') {
            match p {
                "." => {}
                ".." => {
                    parts.pop();
                }
                _ => parts.push(p),
            }
        }
        return parts.join(".");
    }
    if module.starts_with('.') && !module.starts_with("./") {
        let count = module.bytes().take_while(|b| *b == b'.').count();
        let mut parts: Vec<&str> = parent.split('/').filter(|p| !p.is_empty()).collect();
        for _ in 1..count {
            parts.pop();
        }
        parts.push(module.trim_start_matches('.'));
        return parts.join(".");
    }
    if let Some(tail) = module.strip_prefix("crate::") {
        return format!("src.{}", tail.replace("::", "."));
    }
    if let Some(tail) = module.strip_prefix("super::") {
        return format!("{}.{}", parent.replace('/', "."), tail.replace("::", "."));
    }
    if let Some(tail) = module.strip_prefix("self::") {
        return format!("{}.{}", module_name(owner), tail.replace("::", "."));
    }
    module.replace("::", ".").replace('/', ".")
}
pub fn link(all: &BTreeMap<String, Facts>) -> Graph {
    link_owners(all, None)
}
pub fn link_owners(
    all: &BTreeMap<String, Facts>,
    owners: Option<&std::collections::BTreeSet<String>>,
) -> Graph {
    let nodes: Vec<&Node> = all.values().flat_map(|f| f.nodes.iter()).collect();
    let mut by_name: HashMap<&str, Vec<&Node>> = HashMap::new();
    let mut by_qual: HashMap<&str, Vec<&Node>> = HashMap::new();
    let mut by_suffix: HashMap<&str, Vec<&Node>> = HashMap::new();
    let mut by_id: HashMap<&str, &Node> = HashMap::new();
    let mut by_module: HashMap<&str, Vec<&Node>> = HashMap::new();
    for n in &nodes {
        by_name.entry(&n.name).or_default().push(n);
        by_qual.entry(&n.qualname).or_default().push(n);
        by_id.insert(&n.id, n);
        by_module.entry(&n.path).or_default().push(n);
        for (i, c) in n.qualname.char_indices() {
            if c == '.' {
                by_suffix.entry(&n.qualname[i + 1..]).or_default().push(n);
            }
        }
    }
    let lookup = |name: &str| -> Vec<&Node> {
        by_qual
            .get(name)
            .or_else(|| by_suffix.get(name))
            .cloned()
            .unwrap_or_default()
    };
    let parts:Vec<Graph>=all.par_iter().filter(|(path,_)|owners.is_none_or(|o|o.contains(*path))).map(|(path,facts)|{
 let mut graph=Graph{edges:facts.edges.clone(),coverage:Vec::new()};let mut aliases:HashMap<String,Vec<&Node>>=HashMap::new();
 for r in facts.references.iter().filter(|r|r.kind=="imports"){
 let module=normalize_module(path,r.module.as_deref().unwrap_or(&r.expression));
 let symbol=if r.expression=="*"||r.expression=="default"||r.module.as_deref()==Some(&r.expression){module.clone()}else{format!("{module}.{}",r.expression)};
 let mut module_name=module.as_str();let mut modules;loop{modules=lookup(module_name).into_iter().filter(|n|n.kind=="module").collect::<Vec<_>>();if !modules.is_empty(){break;}if let Some((parent,_))=module_name.rsplit_once('.'){module_name=parent;}else{break;}}
 let mut candidates=if r.expression=="default"{modules.iter().flat_map(|m|by_module.get(m.path.as_str()).into_iter().flatten().copied()).filter(|n|n.details["default_export"]==true).collect::<Vec<_>>()}else{lookup(&symbol).into_iter().filter(|n|n.kind!="component").collect::<Vec<_>>()};
 if candidates.is_empty()&&(r.expression=="*"||r.module.as_deref()==Some(&r.expression)){candidates=modules.clone();}
 candidates.sort_by(|a,b|a.id.cmp(&b.id));candidates.dedup_by_key(|n|&n.id);
 if let Some(alias)=&r.alias{aliases.insert(alias.clone(),candidates.clone());}
 let resolved=modules.len()==1&&(r.alias.is_none()||candidates.len()==1);let status=if resolved{"resolved"}else if modules.len()>1||candidates.len()>1{"ambiguous"}else{"unresolved"};
 graph.coverage.push(Coverage{path:path.clone(),line:r.line,expression:r.expression.clone(),status:status.into(),evidence:format!("import {symbol}: {} modules, {} alias targets",modules.len(),candidates.len())});
 if modules.len()==1{graph.edges.push(Edge{src:format!("module:{path}"),dst:modules[0].id.clone(),kind:"imports".into(),path:path.clone(),line:r.line,evidence:r.expression.clone(),confidence:"exact".into()});}
 }
 for r in facts.references.iter().filter(|r|r.kind=="calls"){if r.dynamic {graph.coverage.push(Coverage{path:path.clone(),line:r.line,expression:r.expression.clone(),status:"unresolved".into(),evidence:"computed receiver or dynamic callee; source location retains exact syntax".into()});continue;}
 let expression=r.expression.replace("::",".");let(first,tail)=expression.split_once('.').unwrap_or((&expression,""));
 let owner=by_id.get(r.source.as_str()).copied();
 let mut scope=owner.map(|n|n.qualname.as_str()).unwrap_or("");let mut shadowed=false;let mut lexical=Vec::new();
 while !scope.is_empty(){if let Some(scope_nodes)=by_qual.get(scope){if scope_nodes.iter().any(|n|n.details["bindings"].as_array().is_some_and(|b|b.iter().any(|v|v==first))){shadowed=true;break;}}
 if tail.is_empty(){lexical=by_qual.get(format!("{scope}.{expression}").as_str()).cloned().unwrap_or_default();if !lexical.is_empty(){break;}}
 scope=scope.rsplit_once('.').map_or("",|(p,_)|p);}
 let known_receiver= !tail.is_empty() && (first=="self"||first=="cls") && owner.is_some_and(|n|n.qualname.rsplit_once('.').is_some_and(|(scope,_)| by_qual.get(scope).is_some_and(|nodes|nodes.iter().any(|n|matches!(n.kind.as_str(),"class"|"impl")))));
 if shadowed&&!known_receiver{graph.coverage.push(Coverage{path:path.clone(),line:r.line,expression:r.expression.clone(),status:"unresolved".into(),evidence:"callee is shadowed by a parameter or local binding of unknown callable identity".into()});continue;}
 let mut candidates=lexical;
 if candidates.is_empty(){if let Some(imported)=aliases.get(first){candidates=if tail.is_empty(){imported.clone()}else{imported.iter().flat_map(|n|lookup(&format!("{}.{}",n.qualname,tail))).collect()};}}
 if candidates.is_empty()&&!aliases.contains_key(first){if tail.is_empty(){candidates=by_module.get(path.as_str()).into_iter().flatten().copied().filter(|n|n.name==expression).collect();}else if first=="self"||first=="cls"{if let Some(owner)=owner{let scope=owner.qualname.rsplit_once('.').map_or("",|(p,_)|p);candidates=lookup(&format!("{scope}.{tail}"));}}else{candidates=lookup(&format!("{}.{}",module_name(path),expression));}}
 if candidates.is_empty()&&!aliases.contains_key(first){candidates=by_qual.get(expression.as_str()).cloned().unwrap_or_default();}
 candidates.retain(|n|matches!(n.kind.as_str(),"function"|"method"|"class"|"struct"|"enum"));candidates.sort_by(|a,b|a.id.cmp(&b.id));candidates.dedup_by_key(|n|&n.id);
 let status=if candidates.len()==1{"resolved"}else if candidates.is_empty(){"unresolved"}else{"ambiguous"};graph.coverage.push(Coverage{path:path.clone(),line:r.line,expression:r.expression.clone(),status:status.into(),evidence:format!("{} lexically justified candidates",candidates.len())});
 if candidates.len()==1{graph.edges.push(Edge{src:r.source.clone(),dst:candidates[0].id.clone(),kind:"calls".into(),path:path.clone(),line:r.line,evidence:r.expression.clone(),confidence:"exact".into()});}
 }
 for doc in &facts.docs{for symbol in &doc.referenced_symbols{let normalized=symbol.replace("::",".");let mut candidates=lookup(&normalized);if candidates.is_empty(){candidates=by_name.get(normalized.as_str()).cloned().unwrap_or_default();}for node in candidates{for(src,dst,kind)in [(&doc.doc_id,&node.id,"documents"),(&node.id,&doc.doc_id,"references_doc")]{graph.edges.push(Edge{src:src.clone(),dst:dst.clone(),kind:kind.into(),path:path.clone(),line:1,evidence:symbol.clone(),confidence:"exact".into()});}}}}
 graph
 }).collect();
    let mut result = Graph::default();
    for g in parts {
        result.edges.extend(g.edges);
        result.coverage.extend(g.coverage);
    }
    result.edges.sort_by(|a, b| {
        (&a.path, a.line, &a.src, &a.dst, &a.kind).cmp(&(&b.path, b.line, &b.src, &b.dst, &b.kind))
    });
    result
        .coverage
        .sort_by(|a, b| (&a.path, a.line, &a.expression).cmp(&(&b.path, b.line, &b.expression)));
    result
}
