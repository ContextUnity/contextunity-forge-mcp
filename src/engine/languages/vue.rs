use super::*;
#[path = "vue/template.rs"]
mod template;

pub struct Vue;
pub static VUE: Vue = Vue;
impl LanguageProfile for Vue {
    fn id(&self) -> &'static str {
        "vue"
    }
    fn manifest_filenames(&self) -> &'static [&'static str] {
        typescript::TYPESCRIPT.manifest_filenames()
    }
    fn extract_manifest_dependencies(&self, filename: &str, content: &str) -> Vec<String> {
        typescript::TYPESCRIPT.extract_manifest_dependencies(filename, content)
    }
    fn is_stdlib(&self, module: &str) -> bool {
        typescript::TYPESCRIPT.is_stdlib(module)
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
    fn normalize_import_with_root(
        &self,
        root: Option<&std::path::Path>,
        owner: &str,
        module: &str,
    ) -> Option<ImportPath> {
        typescript::TYPESCRIPT.normalize_import_with_root(root, owner, module)
    }
    fn external_import(&self, module: &str) -> Option<&'static str> {
        typescript::TYPESCRIPT.external_import(module)
    }
    fn builtin(&self, name: &str) -> bool {
        typescript::TYPESCRIPT.builtin(name) || compiler_macro(name) || runtime_builtin(name)
    }
    fn builtin_type(&self, name: &str) -> bool {
        typescript::TYPESCRIPT.builtin_type(name)
    }
    fn builtin_member(&self, receiver: &str, member: &str) -> bool {
        typescript::TYPESCRIPT.builtin_member(receiver, member)
    }
    fn builtin_generic(&self, receiver: &str) -> bool {
        typescript::TYPESCRIPT.builtin_generic(receiver)
    }
    fn extract_file(
        &self,
        path: &str,
        source: &str,
        module: &str,
        facts: &mut Facts,
    ) -> Result<()> {
        extract_file_impl(path, source, module, facts, None)
    }

    fn finish(&self, facts: &mut Facts) {
        typescript::TYPESCRIPT.finish(facts);
        if !facts
            .nodes
            .iter()
            .any(|node| node.details["default_export"] == true)
        {
            if let Some(module) = facts.nodes.iter_mut().find(|node| node.kind == "module") {
                module.details["default_export"] = serde_json::json!(true);
            }
        }
    }
}

pub static PROFILES: &[&dyn LanguageProfile] = &[&VUE];

fn extract_file_impl(
    path: &str,
    source: &str,
    module: &str,
    facts: &mut Facts,
    mut flows: Option<&mut crate::core::typed_facts::FlowStore>,
) -> Result<()> {
    let mut rest = source;
    let mut base = 0;
    let mut script_setup_ranges = Vec::new();
    let mut script_setup_ranges_overflowed = false;
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
        let tag = &rest[start..opening];
        let script = &rest[opening..end];
        if is_setup_script_tag(tag) && script_setup_ranges.len() < 128 {
            let absolute_start = base + opening;
            let absolute_end = absolute_start + script.len();
            let start = source_position(source, absolute_start);
            let end = source_position(source, absolute_end);
            script_setup_ranges.push(serde_json::json!({
                "start_line": start.line,
                "start_column": start.column,
                "end_line": end.line,
                "end_column": end.column,
            }));
        } else if is_setup_script_tag(tag) {
            script_setup_ranges_overflowed = true;
        }
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
        let offset = source[..base + opening]
            .bytes()
            .filter(|b| *b == b'\n')
            .count();
        let column = source[..base + opening]
            .rsplit('\n')
            .next()
            .unwrap_or("")
            .len();
        let tree = ast::parse_island(
            &mut profile.create_parser(path)?,
            script,
            base + opening,
            tree_sitter::Point::new(offset, column),
        )
        .context("Tree-sitter parse cancelled")?;
        ast::extract_tree_with_flows(
            profile,
            tree.root_node(),
            path,
            source,
            module,
            &format!("module:{path}"),
            facts,
            flows.as_deref_mut(),
        );
        base += end + 9;
        rest = &source[base..];
    }

    if let Some(module_node) = facts
        .nodes
        .iter_mut()
        .find(|node| node.kind == "module" && node.path == path)
    {
        module_node.details["vue_script_setup"] = serde_json::Value::Array(
            if script_setup_ranges_overflowed {
                Vec::new()
            } else {
                script_setup_ranges
            },
        );
    }

    template::extract(path, source, module, facts)?;
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct Position {
    line: usize,
    column: usize,
}

fn source_position(source: &str, offset: usize) -> Position {
    let prefix = &source[..offset];
    Position {
        line: prefix.bytes().filter(|byte| *byte == b'\n').count() + 1,
        column: prefix.rsplit('\n').next().unwrap_or("").len(),
    }
}

fn is_setup_script_tag(tag: &str) -> bool {
    let bytes = tag.as_bytes();
    let prefix = b"<script";
    if !bytes.starts_with(prefix) || bytes.len() <= prefix.len() {
        return false;
    }
    let boundary = bytes[prefix.len()];
    if !boundary.is_ascii_whitespace() && boundary != b'>' {
        return false;
    }
    let Some(attributes) = bytes.get(prefix.len()..bytes.len() - 1) else {
        return false;
    };
    if bytes.last() != Some(&b'>') {
        return false;
    }

    let mut setup = false;
    let mut source = false;
    let mut index = 0;
    while index < attributes.len() {
        while index < attributes.len()
            && (attributes[index].is_ascii_whitespace() || attributes[index] == b'/')
        {
            index += 1;
        }
        let name_start = index;
        while index < attributes.len()
            && !attributes[index].is_ascii_whitespace()
            && !matches!(attributes[index], b'=' | b'/')
        {
            index += 1;
        }
        if name_start == index {
            index += 1;
            continue;
        }
        let name = &attributes[name_start..index];
        setup |= name.eq_ignore_ascii_case(b"setup");
        source |= name.eq_ignore_ascii_case(b"src");

        while index < attributes.len() && attributes[index].is_ascii_whitespace() {
            index += 1;
        }
        if attributes.get(index) == Some(&b'=') {
            index += 1;
            while index < attributes.len() && attributes[index].is_ascii_whitespace() {
                index += 1;
            }
            if let Some(quote @ (b'\'' | b'"')) = attributes.get(index).copied() {
                index += 1;
                while index < attributes.len() && attributes[index] != quote {
                    index += 1;
                }
                if index == attributes.len() {
                    return false;
                }
                index += 1;
            } else {
                while index < attributes.len() && !attributes[index].is_ascii_whitespace() {
                    index += 1;
                }
            }
        }
    }
    setup && !source
}

pub(crate) fn compiler_macro(name: &str) -> bool {
    matches!(
        name,
        "defineProps"
            | "defineEmits"
            | "defineExpose"
            | "withDefaults"
            | "defineOptions"
            | "defineSlots"
            | "defineModel"
    )
}

fn runtime_builtin(name: &str) -> bool {
    matches!(
        name,
        "ref"
            | "computed"
            | "reactive"
            | "shallowRef"
            | "shallowReactive"
            | "toRef"
            | "toRefs"
            | "unref"
            | "isRef"
            | "watch"
            | "watchEffect"
            | "onMounted"
            | "onUnmounted"
            | "onUpdated"
            | "onBeforeMount"
            | "onBeforeUnmount"
            | "nextTick"
            | "inject"
            | "provide"
            | "useSlots"
            | "useAttrs"
            | "useI18n"
            | "t"
    )
}

pub(crate) fn admits_macro(
    module: &crate::core::models::Node,
    at: crate::core::semantic::SourcePosition,
) -> bool {
    if module.kind != "module" || module.language != "vue" {
        return false;
    }
    let Some(ranges) = module.details.get("vue_script_setup").and_then(|value| value.as_array())
    else {
        return false;
    };
    if ranges.len() > 128 {
        return false;
    }

    let at = Position {
        line: at.line,
        column: at.column,
    };
    let mut admitted = false;
    for range in ranges {
        let Some(object) = range.as_object() else {
            return false;
        };
        if object.len() != 4
            || !["start_line", "start_column", "end_line", "end_column"]
                .iter()
                .all(|key| object.contains_key(*key))
        {
            return false;
        }
        let coordinate = |key: &str| {
            object
                .get(key)
                .and_then(serde_json::Value::as_u64)
                .and_then(|value| usize::try_from(value).ok())
        };
        let (Some(start_line), Some(start_column), Some(end_line), Some(end_column)) = (
            coordinate("start_line"),
            coordinate("start_column"),
            coordinate("end_line"),
            coordinate("end_column"),
        ) else {
            return false;
        };
        let start = Position {
            line: start_line,
            column: start_column,
        };
        let end = Position {
            line: end_line,
            column: end_column,
        };
        if start >= end {
            return false;
        }
        admitted |= at >= start && at < end;
    }
    admitted
}

pub(crate) fn extract_typed(
    path: &str,
    source: &str,
    module: &str,
    typed: &mut crate::core::typed_facts::TypedFacts,
) -> Result<()> {
    extract_file_impl(
        path,
        source,
        module,
        &mut typed.facts,
        Some(&mut typed.flows),
    )?;
    VUE.finish(&mut typed.facts);
    Ok(())
}
