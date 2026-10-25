use super::*;

#[test]
fn rendered_template_context_keys_follow_exact_targets_and_includes() {
    let workspace = Workspace::new();
    for project in ["shop", "other"] {
        workspace.write(
            format!("{project}/pyproject.toml"),
            &format!("[project]\nname = '{project}'\ndependencies = ['django>=5']\n"),
        );
    }
    workspace.write(
        "shop/views.py",
        "from django.shortcuts import render\n\ndef page(request):\n    return render(request, 'templates/page.html', {'title': 'Welcome'})\n\ndef later(request):\n    return render(request, 'templates/later.html', {'late': 'Ready'})\n",
    );
    workspace.write(
        "shop/templates/page.html",
        "<h1>{{ title }}</h1>\n{% include 'partials/card.html' %}\n",
    );
    workspace.write(
        "shop/templates/partials/card.html",
        "<strong>{{ title }}</strong>\n",
    );
    workspace.write(
        "shop/templates/next.html",
        "<h1>{{ title }}</h1><h2>{{ headline }}</h2>\n",
    );
    workspace.write("other/templates/page.html", "<h1>{{ title }}</h1>\n");
    writer::build(&workspace.0, &workspace.db(), None).unwrap();
    let conn = reader::open(&workspace.db(), &workspace.0).unwrap();
    for (path, expected) in [
        ("shop/templates/page.html", "resolved"),
        ("shop/templates/partials/card.html", "resolved"),
        ("other/templates/page.html", "unresolved"),
    ] {
        let (status, evidence): (String, String) = conn.query_row(
            "SELECT rc.status,ev.evidence FROM resolution_coverage rc JOIN path_dictionary pd ON pd.path_id=rc.path_id JOIN coverage_expressions ce ON ce.expression_id=rc.expression_id JOIN coverage_evidence ev ON ev.evidence_id=rc.evidence_id WHERE pd.path=?1 AND ce.expression='template.variable.title'",
            [path], |row| Ok((row.get(0)?, row.get(1)?)),
        ).unwrap();
        assert_eq!(status, expected, "{path}: {evidence}");
        if status == "resolved" {
            assert!(evidence.contains("shop/views.py"), "{path}: {evidence}");
        }
    }
    drop(conn);

    let rows = |conn: &rusqlite::Connection| {
        conn.prepare("SELECT pd.path,ce.expression,rc.status,ev.evidence FROM resolution_coverage rc JOIN path_dictionary pd ON pd.path_id=rc.path_id JOIN coverage_expressions ce ON ce.expression_id=rc.expression_id JOIN coverage_evidence ev ON ev.evidence_id=rc.evidence_id WHERE ce.expression LIKE 'template.variable.%' ORDER BY pd.path,ce.expression,rc.status,ev.evidence")
            .unwrap()
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?, row.get::<_, String>(3)?))
            })
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap()
    };
    workspace.write("shop/templates/later.html", "<p>{{ late }}</p>\n");
    writer::delta(
        &workspace.0,
        &workspace.db(),
        &[PathBuf::from("shop/templates/later.html")],
    )
    .unwrap();
    let added_cold = workspace.0.join("render-added-cold.sqlite");
    writer::build(&workspace.0, &added_cold, None).unwrap();
    let added_delta = reader::open(&workspace.db(), &workspace.0).unwrap();
    let added_cold = reader::open(&added_cold, &workspace.0).unwrap();
    assert_eq!(
        rows(&added_delta),
        rows(&added_cold),
        "new exact target must hydrate unchanged provider"
    );
    let later = rows(&added_delta)
        .into_iter()
        .find(|row| row.0 == "shop/templates/later.html" && row.1 == "template.variable.late")
        .unwrap();
    assert_eq!(later.2, "resolved", "{later:?}");
    commitments::verify(&added_delta).unwrap();
    commitments::verify(&added_cold).unwrap();
    drop(added_delta);
    drop(added_cold);

    workspace.write(
        "shop/templates/page.html",
        "<h1>{{ title }}</h1><p>Changed</p>\n{% include 'partials/card.html' %}\n",
    );
    writer::delta(
        &workspace.0,
        &workspace.db(),
        &[PathBuf::from("shop/templates/page.html")],
    )
    .unwrap();
    let html_cold = workspace.0.join("render-html-cold.sqlite");
    writer::build(&workspace.0, &html_cold, None).unwrap();
    let delta = reader::open(&workspace.db(), &workspace.0).unwrap();
    let cold = reader::open(&html_cold, &workspace.0).unwrap();
    assert_eq!(
        rows(&delta),
        rows(&cold),
        "HTML-only delta must retain unchanged render provider facts"
    );
    commitments::verify(&delta).unwrap();
    commitments::verify(&cold).unwrap();
    drop(delta);
    drop(cold);

    workspace.write(
        "shop/views.py",
        "from django.shortcuts import render\n\ndef page(request):\n    return render(request, 'templates/next.html', {'title': 'Welcome'})\n\ndef later(request):\n    return render(request, 'templates/later.html', {'late': 'Ready'})\n",
    );
    writer::delta(
        &workspace.0,
        &workspace.db(),
        &[PathBuf::from("shop/views.py")],
    )
    .unwrap();
    let target_cold = workspace.0.join("render-target-cold.sqlite");
    writer::build(&workspace.0, &target_cold, None).unwrap();
    let delta = reader::open(&workspace.db(), &workspace.0).unwrap();
    let cold = reader::open(&target_cold, &workspace.0).unwrap();
    assert_eq!(
        rows(&delta),
        rows(&cold),
        "old and new literal targets must be invalidated"
    );
    for path in [
        "shop/templates/page.html",
        "shop/templates/partials/card.html",
    ] {
        let old = rows(&delta)
            .into_iter()
            .find(|row| row.0 == path && row.1 == "template.variable.title")
            .unwrap();
        assert_eq!(old.2, "unresolved", "{path}: {old:?}");
    }
    let next = rows(&delta)
        .into_iter()
        .find(|row| row.0 == "shop/templates/next.html" && row.1 == "template.variable.title")
        .unwrap();
    assert_eq!(next.2, "resolved", "{next:?}");
    commitments::verify(&delta).unwrap();
    commitments::verify(&cold).unwrap();
    drop(delta);
    drop(cold);

    workspace.write(
        "shop/views.py",
        "from django.shortcuts import render\n\ndef page(request):\n    return render(request, 'templates/next.html', {'headline': 'Welcome'})\n\ndef later(request):\n    return render(request, 'templates/later.html', {'late': 'Ready'})\n",
    );
    writer::delta(
        &workspace.0,
        &workspace.db(),
        &[PathBuf::from("shop/views.py")],
    )
    .unwrap();
    let key_cold = workspace.0.join("render-key-cold.sqlite");
    writer::build(&workspace.0, &key_cold, None).unwrap();
    let delta = reader::open(&workspace.db(), &workspace.0).unwrap();
    let cold = reader::open(&key_cold, &workspace.0).unwrap();
    assert_eq!(
        rows(&delta),
        rows(&cold),
        "changed literal context keys must be invalidated"
    );
    for (expression, expected) in [
        ("template.variable.title", "unresolved"),
        ("template.variable.headline", "resolved"),
    ] {
        let row = rows(&delta)
            .into_iter()
            .find(|row| row.0 == "shop/templates/next.html" && row.1 == expression)
            .unwrap();
        assert_eq!(row.2, expected, "{row:?}");
    }
    commitments::verify(&delta).unwrap();
    commitments::verify(&cold).unwrap();
}

#[test]
fn rendered_template_with_multiple_views_stays_ambiguous() {
    let workspace = Workspace::new();
    workspace.write(
        "shop/pyproject.toml",
        "[project]\nname = 'shop'\ndependencies = ['django>=5']\n",
    );
    workspace.write(
        "shop/views.py",
        "from django.shortcuts import render\n\ndef page(request):\n    return render(request, 'templates/page.html', {'title': 'Welcome'})\n\ndef other(request):\n    return render(request, 'templates/page.html', {'headline': 'Other'})\n",
    );
    workspace.write("shop/templates/page.html", "<h1>{{ title }}</h1>\n");
    writer::build(&workspace.0, &workspace.db(), None).unwrap();
    let conn = reader::open(&workspace.db(), &workspace.0).unwrap();
    let (status, evidence): (String, String) = conn
        .query_row(
            "SELECT rc.status,ev.evidence FROM resolution_coverage rc JOIN path_dictionary pd ON pd.path_id=rc.path_id JOIN coverage_expressions ce ON ce.expression_id=rc.expression_id JOIN coverage_evidence ev ON ev.evidence_id=rc.evidence_id WHERE pd.path='shop/templates/page.html' AND ce.expression='template.variable.title'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(status, "ambiguous", "{evidence}");
    let edges: i64 = conn
        .query_row(
            "SELECT count(*) FROM edges e JOIN path_dictionary pd ON pd.path_id=e.path_id WHERE e.kind='context_provider' AND pd.path='shop/templates/page.html'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        edges, 0,
        "a shared template must not emit a context_provider edge"
    );
}
