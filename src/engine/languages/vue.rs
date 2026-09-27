use super::*;

fn extract_template_bindings(
    path: &str,
    template: &str,
    template_offset: usize,
    source: &str,
    facts: &mut Facts,
) {
    let owner = format!("module:{path}");
    let bytes = template.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'<' {
            let mut tag_start = i + 1;
            if tag_start < bytes.len() && bytes[tag_start] == b'/' {
                tag_start += 1;
            }
            let mut j = tag_start;
            while j < bytes.len()
                && (bytes[j].is_ascii_alphanumeric() || bytes[j] == b'-' || bytes[j] == b'_')
            {
                j += 1;
            }
            if j > tag_start {
                let tag = &template[tag_start..j];
                if tag.chars().next().is_some_and(|c| c.is_ascii_uppercase()) {
                    let line = source[..template_offset + tag_start]
                        .bytes()
                        .filter(|b| *b == b'\n')
                        .count()
                        + 1;
                    facts.references.push(Reference {
                        source: owner.clone(),
                        dynamic: false,
                        expression: tag.to_string(),
                        kind: "references".into(),
                        line,
                        alias: None,
                        module: None,
                    });
                }
            }
            i = j;
            continue;
        }

        if i + 1 < bytes.len() && bytes[i] == b'{' && bytes[i + 1] == b'{' {
            let expr_start = i + 2;
            if let Some(end_rel) = template[expr_start..].find("}}") {
                let expr = template[expr_start..expr_start + end_rel].trim();
                let line = source[..template_offset + expr_start]
                    .bytes()
                    .filter(|b| *b == b'\n')
                    .count()
                    + 1;
                extract_simple_expressions(&owner, expr, line, facts);
                i = expr_start + end_rel + 2;
                continue;
            }
        }

        if bytes[i] == b'@'
            || (bytes[i] == b':' && i > 0 && bytes[i - 1].is_ascii_whitespace())
            || (i + 2 < bytes.len() && bytes[i] == b'v' && bytes[i + 1] == b'-')
        {
            let attr_start = i;
            if let Some(eq_rel) = template[attr_start..].find('=') {
                let attr_name = template[attr_start..attr_start + eq_rel].trim();
                let is_event = attr_name.starts_with('@') || attr_name.starts_with("v-on:");
                let val_start = attr_start + eq_rel + 1;
                let val_bytes = &bytes[val_start..];
                if let Some(quote) = val_bytes.first().copied().filter(|&b| b == b'"' || b == b'\'') {
                    if let Some(close_rel) = template[val_start + 1..].find(quote as char) {
                        let expr = template[val_start + 1..val_start + 1 + close_rel].trim();
                        let line = source[..template_offset + val_start + 1]
                            .bytes()
                            .filter(|b| *b == b'\n')
                            .count()
                            + 1;
                        if is_event {
                            extract_event_handler(&owner, expr, line, facts);
                        } else {
                            extract_simple_expressions(&owner, expr, line, facts);
                        }
                        i = val_start + 1 + close_rel + 1;
                        continue;
                    }
                }
            }
        }

        i += 1;
    }
}

fn extract_event_handler(owner: &str, expr: &str, line: usize, facts: &mut Facts) {
    let clean = expr.trim();
    let callee = clean.split('(').next().unwrap_or(clean).trim();
    if is_valid_ident(callee) {
        facts.references.push(Reference {
            source: owner.to_string(),
            dynamic: false,
            expression: callee.to_string(),
            kind: "calls".into(),
            line,
            alias: None,
            module: None,
        });
    } else {
        extract_simple_expressions(owner, clean, line, facts);
    }
}

fn extract_simple_expressions(owner: &str, expr: &str, line: usize, facts: &mut Facts) {
    for word in expr.split(|c: char| !c.is_alphanumeric() && c != '_' && c != '$') {
        let word = word.trim();
        if is_valid_ident(word) && !is_js_keyword(word) {
            facts.references.push(Reference {
                source: owner.to_string(),
                dynamic: false,
                expression: word.to_string(),
                kind: "references".into(),
                line,
                alias: None,
                module: None,
            });
        }
    }
}

fn is_valid_ident(s: &str) -> bool {
    !s.is_empty()
        && (s.chars().next().unwrap().is_alphabetic() || s.starts_with('_') || s.starts_with('$'))
        && s.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '$')
}

fn is_js_keyword(s: &str) -> bool {
    matches!(
        s,
        "true"
            | "false"
            | "null"
            | "undefined"
            | "NaN"
            | "Infinity"
            | "typeof"
            | "instanceof"
            | "in"
            | "new"
            | "return"
            | "if"
            | "else"
            | "for"
            | "while"
            | "do"
            | "break"
            | "continue"
            | "switch"
            | "case"
            | "default"
            | "try"
            | "catch"
            | "finally"
            | "throw"
            | "function"
            | "var"
            | "let"
            | "const"
            | "this"
            | "class"
            | "extends"
            | "import"
            | "export"
            | "super"
            | "async"
            | "await"
            | "yield"
            | "void"
            | "delete"
            | "$event"
    )
}

pub struct Vue;
pub static VUE: Vue = Vue;
impl LanguageProfile for Vue {
    fn id(&self) -> &'static str {
        "vue"
    }
    fn family(&self) -> LanguageFamily {
        LanguageFamily("javascript")
    }
    fn extensions(&self) -> &'static [&'static str] {
        &["vue"]
    }
    fn grammar(&self, path: &str) -> tree_sitter::Language {
        typescript::TYPESCRIPT.grammar(path)
    }
    fn symbol_kind(&self, kind: &str) -> Option<&'static str> {
        typescript::TYPESCRIPT.symbol_kind(kind)
    }
    fn node_prefix(&self, kind: &str) -> &'static str {
        typescript::TYPESCRIPT.node_prefix(kind)
    }
    fn module_name(&self, path: &str) -> String {
        typescript::TYPESCRIPT.module_name(path)
    }
    fn normalize_import(&self, owner: &str, module: &str) -> Option<ImportPath> {
        typescript::TYPESCRIPT.normalize_import(owner, module)
    }
    fn external_import(&self, module: &str) -> Option<&'static str> {
        typescript::TYPESCRIPT.external_import(module)
    }
    fn builtin(&self, name: &str) -> bool {
        typescript::TYPESCRIPT.builtin(name)
    }
    fn extract_file(
        &self,
        path: &str,
        source: &str,
        module: &str,
        facts: &mut Facts,
    ) -> Result<()> {
        let mut rest = source;
        let mut base = 0;
        while let Some(start) = rest.find("<script") {
            let opening = start
                + rest[start..]
                    .find('>')
                    .context("unterminated Vue script tag")?
                + 1;
            let end = opening
                + rest[opening..]
                    .find("</script>")
                    .context("unterminated Vue script block")?;
            let script = &rest[opening..end];
            let lang = if rest[start..opening].contains("lang=\"ts\"")
                || rest[start..opening].contains("lang='ts'")
            {
                "typescript"
            } else {
                "javascript"
            };
            let profile: &dyn LanguageProfile = if lang == "typescript" {
                &typescript::TYPESCRIPT
            } else {
                &typescript::JAVASCRIPT
            };
            let tree = profile
                .create_parser(path)?
                .parse(script, None)
                .context("Tree-sitter parse cancelled")?;
            let offset = source[..base + opening]
                .bytes()
                .filter(|b| *b == b'\n')
                .count();
            let column = source[..base + opening]
                .rsplit('\n')
                .next()
                .unwrap_or("")
                .len();
            ast::extract_tree(
                profile,
                tree.root_node_with_offset(base + opening, tree_sitter::Point::new(offset, column)),
                path,
                source,
                module,
                facts,
            );
            base += end + 9;
            rest = &source[base..];
        }

        if let Some(start) = source.find("<template") {
            if let Some(opening_rel) = source[start..].find('>') {
                let opening = start + opening_rel + 1;
                if let Some(end_rel) = source[opening..].find("</template>") {
                    let end = opening + end_rel;
                    let template = &source[opening..end];
                    extract_template_bindings(path, template, opening, source, facts);
                }
            }
        }
        Ok(())
    }
    fn finish(&self, facts: &mut Facts) {
        typescript::TYPESCRIPT.finish(facts);
    }
}

pub static PROFILES: &[&dyn LanguageProfile] = &[&VUE];
