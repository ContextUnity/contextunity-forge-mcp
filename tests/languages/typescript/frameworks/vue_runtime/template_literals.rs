use super::super::*;

#[cfg(feature = "lang-vue")]
#[test]
fn vue_template_unicode_does_not_interrupt_bindings() {
    let source = "<template>\n  <div>⏱ {{ timestamp }}</div>\n  <button @click=\"onSubmit\">Go</button>\n</template>\n";
    let facts = ast::extract("Timer.vue", "vue", source).unwrap();
    let expressions: Vec<&str> = facts
        .references
        .iter()
        .map(|r| r.expression.as_str())
        .collect();

    assert!(expressions.contains(&"timestamp"), "{expressions:?}");
    assert!(expressions.contains(&"onSubmit"), "{expressions:?}");
}

#[cfg(feature = "lang-vue")]
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
    let facts = ast::extract("Card.vue", "vue", source).unwrap();
    let expressions: Vec<&str> = facts
        .references
        .iter()
        .map(|r| r.expression.as_str())
        .collect();

    assert!(
        expressions.contains(&"active"),
        "must contain active: {expressions:?}"
    );
    assert!(
        expressions.contains(&"isHovered"),
        "must contain isHovered: {expressions:?}"
    );
    assert!(
        expressions.contains(&"user"),
        "must contain user: {expressions:?}"
    );
    assert!(
        expressions.contains(&"user.name"),
        "must contain the qualified receiver member user.name: {expressions:?}"
    );

    // CSS class string tokens MUST NOT be extracted as references
    assert!(
        !expressions.contains(&"bg"),
        "must NOT contain 'bg': {expressions:?}"
    );
    assert!(
        !expressions.contains(&"white"),
        "must NOT contain 'white': {expressions:?}"
    );
    assert!(
        !expressions.contains(&"text"),
        "must NOT contain 'text': {expressions:?}"
    );
    assert!(
        !expressions.contains(&"blue"),
        "must NOT contain 'blue': {expressions:?}"
    );
    assert!(
        !expressions.contains(&"hover"),
        "must NOT contain 'hover': {expressions:?}"
    );
    assert!(
        !expressions.contains(&"opacity"),
        "must NOT contain 'opacity': {expressions:?}"
    );
    assert!(
        !expressions.contains(&"anonymous"),
        "must NOT contain 'anonymous': {expressions:?}"
    );
}

#[cfg(feature = "lang-vue")]
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
    let facts = ast::extract("TemplateLiteral.vue", "vue", source).unwrap();
    let expressions: Vec<&str> = facts
        .references
        .iter()
        .map(|r| r.expression.as_str())
        .collect();

    assert!(
        expressions.contains(&"user"),
        "must contain user: {expressions:?}"
    );
    assert!(
        expressions.contains(&"user.name"),
        "must contain the qualified receiver member user.name: {expressions:?}"
    );
    assert!(
        expressions.contains(&"role"),
        "must contain role: {expressions:?}"
    );
    assert!(
        expressions.contains(&"active"),
        "must contain active: {expressions:?}"
    );
    assert!(
        expressions.contains(&"target"),
        "must contain target: {expressions:?}"
    );
    assert!(
        expressions.contains(&"target.title"),
        "must contain the qualified receiver member target.title: {expressions:?}"
    );

    // Literal string parts must NOT be extracted
    assert!(
        !expressions.contains(&"User"),
        "must NOT contain 'User': {expressions:?}"
    );
    assert!(
        !expressions.contains(&"prefix"),
        "must NOT contain 'prefix': {expressions:?}"
    );
    assert!(
        !expressions.contains(&"Hello"),
        "must NOT contain 'Hello': {expressions:?}"
    );
    assert!(
        !expressions.contains(&"World"),
        "must NOT contain 'World': {expressions:?}"
    );
}
