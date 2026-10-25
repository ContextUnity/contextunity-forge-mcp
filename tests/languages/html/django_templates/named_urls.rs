use super::*;

#[test]
fn django_named_url_references_resolve_only_to_unique_routes() {
    let workspace = Workspace::new();
    workspace.write(
        "pyproject.toml",
        "[project]\nname='django-named-routes'\ndependencies=['django']\n",
    );
    workspace.write(
        "urls.py",
        "from django.urls import path\n\ndef product_list(): pass\ndef duplicate_a(): pass\ndef duplicate_b(): pass\nurlpatterns = [\n    path('products/', product_list, name='product-list'),\n    path('duplicate/a/', duplicate_a, name='duplicate'),\n    path('duplicate/b/', duplicate_b, name='duplicate'),\n]\n",
    );
    let consumer_source = "from django.urls import reverse\n\ndef product_url():\n    return reverse('product-list')\n\nimport django.urls as urls\nimport django.urls\ndef module_url():\n    return urls.reverse('product-list')\ndef qualified_url():\n    return django.urls.reverse('product-list')\ndef shadowed(reverse):\n    return reverse('product-list')\ndef dynamic(name):\n    return reverse(name)\nfrom urls import product_list\ndef ordinary(): return product_list()\n";
    workspace.write("consumer.py", consumer_source);
    let template_source = "<a href=\"{% url 'product-list' %}\">Products</a>\n";
    workspace.write("templates/product.html", template_source);
    workspace.write("templates/missing.html", "{% url 'missing' %}\n");
    workspace.write("templates/duplicate.html", "{% url 'duplicate' %}\n");

    let html = &contextunity_forge_mcp::engine::languages::html::HTML;
    let mut html_facts = contextunity_forge_mcp::core::models::Facts::default();
    contextunity_forge_mcp::engine::languages::LanguageProfile::extract_file(
        html,
        "templates/product.html",
        template_source,
        "templates.product",
        &mut html_facts,
    )
    .unwrap();
    assert!(html_facts.references.iter().any(|reference| {
        reference.kind == "references_route"
            && reference.expression == "django.route_name.product-list"
    }));

    let python = &contextunity_forge_mcp::engine::languages::python::PYTHON;
    let mut python_facts = contextunity_forge_mcp::core::models::Facts::default();
    contextunity_forge_mcp::engine::languages::LanguageProfile::extract_file(
        python,
        "consumer.py",
        consumer_source,
        "consumer",
        &mut python_facts,
    )
    .unwrap();
    assert!(python_facts.references.iter().any(|reference| {
        reference.kind == "references_route"
            && reference.expression == "django.route_name.product-list"
    }));

    writer::build(&workspace.0, &workspace.db(), None).unwrap();
    let assert_state = |python_active: bool, template_active: bool| {
        let conn = workspace.open();
        assert_valid(&conn);
        let product_routes = named_route_ids(&conn, "product-list");
        assert_eq!(product_routes.len(), 1, "one named route must be indexed");
        assert_eq!(named_route_ids(&conn, "duplicate").len(), 2);
        assert_eq!(
            edge_occurrence_rows(&conn, "templates/product.html", "references_route"),
            if template_active {
                vec![(
                    "module:templates/product.html".into(),
                    product_routes[0].clone(),
                    1,
                )]
            } else {
                vec![]
            }
        );
        assert_eq!(
            edge_occurrence_rows(&conn, "consumer.py", "references_route"),
            if python_active {
                [("product_url", 4), ("module_url", 9), ("qualified_url", 11)]
                    .into_iter()
                    .map(|(name, line)| {
                        (
                            node_id(&conn, "consumer.py", name),
                            product_routes[0].clone(),
                            line,
                        )
                    })
                    .collect::<Vec<_>>()
            } else {
                vec![]
            }
        );
        if python_active {
            for line in [4, 9, 11] {
                assert_eq!(
                    named_route_coverage(&conn, "consumer.py", line, "product-list"),
                    "resolved"
                );
            }
        }
        if template_active {
            for (path, name, status) in [
                ("templates/product.html", "product-list", "resolved"),
                ("templates/missing.html", "missing", "unresolved"),
                ("templates/duplicate.html", "duplicate", "ambiguous"),
            ] {
                assert_eq!(named_route_coverage(&conn, path, 1, name), status);
            }
        }
        for path in ["templates/missing.html", "templates/duplicate.html"] {
            assert!(
                edge_occurrence_rows(&conn, path, "references_route").is_empty(),
                "non-unique named route must not emit an edge: {path}"
            );
        }
        let references = persisted_facts(&conn)
            .iter()
            .flat_map(|facts| &facts.references)
            .filter(|reference| reference.kind == "references_route")
            .count();
        assert_eq!(
            references,
            3 * usize::from(python_active) + 3 * usize::from(template_active)
        );
        let coverage: usize = conn.query_row(
            "SELECT count(*) FROM resolution_coverage c JOIN coverage_expressions x ON x.expression_id=c.expression_id WHERE x.expression LIKE 'django.route_name.%'",
            [], |row| row.get(0),
        ).unwrap();
        assert_eq!(
            coverage, references,
            "removed rules leave no semantic coverage"
        );
        assert_eq!(
            edge_occurrence_rows(&conn, "consumer.py", "calls"),
            [(
                node_id(&conn, "consumer.py", "ordinary"),
                node_id(&conn, "urls.py", "product_list"),
                17
            )]
        );
        let import_status: String = conn.query_row(
            "SELECT c.status FROM resolution_coverage c JOIN path_dictionary p ON p.path_id=c.path_id JOIN coverage_expressions x ON x.expression_id=c.expression_id WHERE p.path='consumer.py' AND c.line=16 AND x.expression='product_list'",
            [], |row| row.get(0),
        ).unwrap();
        assert_eq!(
            import_status, "resolved",
            "ordinary import survives rule changes"
        );
        drop(conn);
        complete_parity(&workspace);
    };
    assert_state(true, true);

    let manifest = ".forge/frameworks/django.toml";
    let mut pack: toml::Value = toml::from_str(include_str!(
        "../../../../src/engine/languages/manifests/django.toml"
    ))
    .unwrap();
    pack.as_table_mut()
        .unwrap()
        .insert("replace".into(), true.into());
    let mut removed = pack.clone();
    removed["syntax_rules"]
        .as_array_mut()
        .unwrap()
        .retain(|rule| {
            !matches!(
                rule["id"].as_str(),
                Some("named-route-call" | "named-route-tag")
            )
        });
    workspace.write(manifest, &toml::to_string(&removed).unwrap());
    workspace.delta(manifest);
    assert_state(false, false);

    let call = pack["syntax_rules"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|rule| rule["id"].as_str() == Some("named-route-call"))
        .unwrap();
    call["operation"]["module"] = "atelier.links".into();
    call["operation"]["export"] = "locate".into();
    workspace.write(manifest, &toml::to_string(&pack).unwrap());
    workspace.delta(manifest);
    assert_state(false, true);
    workspace.write(
        "consumer.py",
        &consumer_source
            .replace("django.urls", "atelier.links")
            .replace("reverse", "locate"),
    );
    workspace.delta("consumer.py");
    assert_state(true, true);

    fs::remove_file(workspace.0.join(manifest)).unwrap();
    workspace.delta(manifest);
    assert_state(false, true);
    workspace.write("consumer.py", consumer_source);
    workspace.delta("consumer.py");
    assert_state(true, true);
}
