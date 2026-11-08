use super::super::*;

#[cfg(feature = "lang-vue")]
#[test]
fn vue_template_unicode_does_not_interrupt_bindings() {
    let source = "<template>\n  <div>⏱ {{ timestamp }}</div>\n  <button @click=\"onSubmit\">Go</button>\n</template>\n";
    let facts = ExtractorHarness::for_lang("vue")
        .at_path("Timer.vue")
        .extract(source);
    let expressions: Vec<&str> = facts
        .references
        .iter()
        .map(|r| r.expression.as_str())
        .collect();

    assert!(expressions.contains(&"timestamp"), "{expressions:?}");
    assert!(expressions.contains(&"onSubmit"), "{expressions:?}");
}

#[cfg(feature = "lang-vue")]
#[test]
fn vue_template_string_literals_do_not_produce_bogus_references() {
    let source = r#"
<template>
  <div :class="['bg-white/5', active && 'text-blue-500', { 'hover:opacity-100': isHovered }]">
    <p>{{ user.name + ' - anonymous' }}</p>
  </div>
</template>
<script setup lang="ts">
const active = true;
const isHovered = false;
const user = { name: "User" };
</script>
"#;
    let facts = ExtractorHarness::for_lang("vue")
        .at_path("Card.vue")
        .extract(source);
    let expressions: Vec<&str> = facts
        .references
        .iter()
        .map(|r| r.expression.as_str())
        .collect();

    for expression in ["active", "isHovered", "user", "user.name"] {
        assert!(
            expressions.contains(&expression),
            "missing {expression}: {expressions:?}"
        );
    }
    for literal in [
        "bg",
        "white",
        "text",
        "blue",
        "hover",
        "opacity",
        "anonymous",
    ] {
        assert!(
            !expressions.contains(&literal),
            "literal {literal}: {expressions:?}"
        );
    }
}

#[cfg(feature = "lang-vue")]
#[test]
fn vue_template_literal_interpolations_preserve_references() {
    let source = r#"
<template>
  <div :title="`User: ${user.name} (${role})`" :class="`prefix ${active ? 'is-active' : ''}`">
    <p>{{ `Hello ${target.title}!` }}</p>
  </div>
</template>
<script setup lang="ts">
const user = { name: "Antigravity" };
const role = "Admin";
const active = true;
const target = { title: "World" };
</script>
"#;
    let facts = ExtractorHarness::for_lang("vue")
        .at_path("TemplateLiteral.vue")
        .extract(source);
    let expressions: Vec<&str> = facts
        .references
        .iter()
        .map(|r| r.expression.as_str())
        .collect();

    for expression in [
        "user",
        "user.name",
        "role",
        "active",
        "target",
        "target.title",
    ] {
        assert!(
            expressions.contains(&expression),
            "missing {expression}: {expressions:?}"
        );
    }
    for literal in ["User", "prefix", "Hello", "World"] {
        assert!(
            !expressions.contains(&literal),
            "literal {literal}: {expressions:?}"
        );
    }
}
