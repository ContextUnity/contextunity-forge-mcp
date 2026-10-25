use super::*;

#[test]
fn jinja_builtins_follow_imported_filesystem_loader_roots() {
    let workspace = Workspace::new();
    workspace.write(
        "shop/pyproject.toml",
        "[project]\nname = 'shop'\ndependencies = ['jinja2>=3']\n",
    );
    workspace.write(
        "shop/site/environment.py",
        "from pathlib import Path\nfrom jinja2 import Environment, FileSystemLoader\nTEMPLATES = Path(__file__).parent / 'templates'\nenv = Environment(loader=FileSystemLoader(str(TEMPLATES)))\n",
    );
    let source = "{% set rows = [] %}\n{{ rows|selectattr('active')|list }}\n";
    let rooted_source = format!("{source}{{% include 'partials/card.html' %}}\n");
    workspace.write("shop/site/templates/page.html", &rooted_source);
    workspace.write("shop/site/other/templates/page.html", source);
    workspace.write(
        "shop/site/templates/partials/card.html",
        "<article>{{ row.name }}</article>\n",
    );
    writer::build(&workspace.0, &workspace.db(), None).unwrap();
    let conn = reader::open(&workspace.db(), &workspace.0).unwrap();
    let status = |conn: &rusqlite::Connection, path: &str, expression: &str| -> String {
        conn.query_row(
            "SELECT rc.status FROM resolution_coverage rc JOIN path_dictionary pd ON pd.path_id=rc.path_id JOIN coverage_expressions ce ON ce.expression_id=rc.expression_id WHERE pd.path=?1 AND ce.expression=?2",
            [path, expression],
            |row| row.get(0),
        )
        .unwrap()
    };
    let include_targets = |conn: &rusqlite::Connection, path: &str| -> Vec<String> {
        conn.prepare("SELECT (SELECT path FROM path_dictionary WHERE path_id=dst.path_id) FROM edge_occurrences e JOIN nodes dst ON dst.node_hash=e.dst_hash WHERE (SELECT path FROM path_dictionary WHERE path_id=e.owner_id)=?1 AND e.kind='includes' ORDER BY (SELECT path FROM path_dictionary WHERE path_id=dst.path_id)")
            .unwrap()
            .query_map([path], |row| row.get(0))
            .unwrap()
            .map(Result::unwrap)
            .collect()
    };
    let expected_include = ["shop/site/templates/partials/card.html"];
    assert_eq!(
        include_targets(&conn, "shop/site/templates/page.html"),
        expected_include,
        "the declared FileSystemLoader keeps the exact Jinja include target"
    );
    for expression in [
        "template.tag.set",
        "template.filter.selectattr",
        "template.filter.list",
    ] {
        assert_eq!(
            status(&conn, "shop/site/templates/page.html", expression),
            "external",
            "{expression}"
        );
        assert_eq!(
            status(&conn, "shop/site/other/templates/page.html", expression),
            "unresolved",
            "{expression}"
        );
    }
    drop(conn);
    workspace.write(
        "shop/site/templates/page.html",
        &format!("{rooted_source}<p>Changed</p>\n"),
    );
    writer::delta(
        &workspace.0,
        &workspace.db(),
        &[PathBuf::from("shop/site/templates/page.html")],
    )
    .unwrap();
    let cold_db = workspace.0.join("jinja-cold.sqlite");
    writer::build(&workspace.0, &cold_db, None).unwrap();
    let delta = reader::open(&workspace.db(), &workspace.0).unwrap();
    let cold = reader::open(&cold_db, &workspace.0).unwrap();
    assert_eq!(
        include_targets(&delta, "shop/site/templates/page.html"),
        expected_include,
        "the include edge survives incremental persistence"
    );
    assert_eq!(
        include_targets(&delta, "shop/site/templates/page.html"),
        include_targets(&cold, "shop/site/templates/page.html")
    );
    for expression in [
        "template.tag.set",
        "template.filter.selectattr",
        "template.filter.list",
    ] {
        assert_eq!(
            status(&delta, "shop/site/templates/page.html", expression),
            "external",
            "{expression}"
        );
        assert_eq!(
            status(&delta, "shop/site/templates/page.html", expression),
            status(&cold, "shop/site/templates/page.html", expression)
        );
    }
    commitments::verify(&delta).unwrap();
    commitments::verify(&cold).unwrap();
    drop(delta);
    drop(cold);
    workspace.write(
        "shop/site/environment.py",
        "from pathlib import Path\nfrom jinja2 import Environment, FileSystemLoader\nTEMPLATES = Path(__file__).parent / 'other' / 'templates'\nenv = Environment(loader=FileSystemLoader(str(TEMPLATES)))\n",
    );
    writer::delta(
        &workspace.0,
        &workspace.db(),
        &[PathBuf::from("shop/site/environment.py")],
    )
    .unwrap();
    writer::build(&workspace.0, &cold_db, None).unwrap();
    let delta = reader::open(&workspace.db(), &workspace.0).unwrap();
    let cold = reader::open(&cold_db, &workspace.0).unwrap();
    for (path, expected) in [
        ("shop/site/templates/page.html", "unresolved"),
        ("shop/site/other/templates/page.html", "external"),
    ] {
        for expression in [
            "template.tag.set",
            "template.filter.selectattr",
            "template.filter.list",
        ] {
            assert_eq!(
                status(&delta, path, expression),
                expected,
                "{path} {expression}"
            );
            assert_eq!(
                status(&delta, path, expression),
                status(&cold, path, expression)
            );
        }
    }
    commitments::verify(&delta).unwrap();
    commitments::verify(&cold).unwrap();
}

#[test]
fn jinja_loader_roots_fail_closed_without_a_unique_imported_provider() {
    let workspace = Workspace::new();
    let source = "{% set rows = [] %}\n{{ rows|selectattr('active')|list }}\n";
    for project in [
        "ambiguous",
        "rebound",
        "annotated",
        "tuple",
        "local_shadow",
        "src_jinja_shadow",
        "src_path_shadow",
        "missing",
    ] {
        workspace.write(
            format!("{project}/pyproject.toml"),
            &format!("[project]\nname = '{project}'\ndependencies = ['jinja2>=3']\n"),
        );
        workspace.write(format!("{project}/templates/page.html"), source);
    }
    let provider = "from pathlib import Path\nfrom jinja2 import Environment, FileSystemLoader\nTEMPLATES = Path(__file__).parent / 'templates'\nenv = Environment(loader=FileSystemLoader(str(TEMPLATES)))\n";
    workspace.write("ambiguous/first.py", provider);
    workspace.write("ambiguous/second.py", provider);
    workspace.write("rebound/environment.py", "from pathlib import Path\nfrom jinja2 import Environment, FileSystemLoader\nFileSystemLoader = object()\nTEMPLATES = Path(__file__).parent / 'templates'\nenv = Environment(loader=FileSystemLoader(str(TEMPLATES)))\n");
    workspace.write("annotated/environment.py", "from pathlib import Path\nfrom jinja2 import Environment, FileSystemLoader\nFileSystemLoader: object = object()\nTEMPLATES = Path(__file__).parent / 'templates'\nenv = Environment(loader=FileSystemLoader(str(TEMPLATES)))\n");
    workspace.write("tuple/environment.py", "from pathlib import Path\nfrom jinja2 import Environment, FileSystemLoader\nFileSystemLoader, unused = (object(), None)\nTEMPLATES = Path(__file__).parent / 'templates'\nenv = Environment(loader=FileSystemLoader(str(TEMPLATES)))\n");
    workspace.write("local_shadow/jinja2.py", "def Environment(*args, **kwargs):\n    return None\n\ndef FileSystemLoader(*args, **kwargs):\n    return None\n");
    workspace.write("local_shadow/environment.py", provider);
    workspace.write("src_jinja_shadow/src/jinja2.py", "def Environment(*args, **kwargs):\n    return None\n\ndef FileSystemLoader(*args, **kwargs):\n    return None\n");
    workspace.write("src_jinja_shadow/environment.py", provider);
    workspace.write("src_path_shadow/src/pathlib.py", "class Path:\n    pass\n");
    workspace.write("src_path_shadow/environment.py", provider);
    writer::build(&workspace.0, &workspace.db(), None).unwrap();
    let conn = reader::open(&workspace.db(), &workspace.0).unwrap();
    for project in [
        "ambiguous",
        "rebound",
        "annotated",
        "tuple",
        "local_shadow",
        "src_jinja_shadow",
        "src_path_shadow",
        "missing",
    ] {
        let path = format!("{project}/templates/page.html");
        for expression in [
            "template.tag.set",
            "template.filter.selectattr",
            "template.filter.list",
        ] {
            let status: String = conn.query_row(
                "SELECT rc.status FROM resolution_coverage rc JOIN path_dictionary pd ON pd.path_id=rc.path_id JOIN coverage_expressions ce ON ce.expression_id=rc.expression_id WHERE pd.path=?1 AND ce.expression=?2",
                [path.as_str(), expression],
                |row| row.get(0),
            ).unwrap();
            assert_eq!(status, "unresolved", "{project} {expression}");
        }
    }
}

#[test]
fn jinja_loader_roots_follow_literal_parent_chains_and_local_loader_flow() {
    let workspace = Workspace::new();
    let template = "{% set rows = [] %}\n{{ rows|e }}\n";
    for project in [
        "proven",
        "dynamic_parent",
        "bare_file_parent",
        "string_resolve",
        "rebound_loader",
        "shadowed_str",
        "shadowed_path",
        "shadowed_root",
        "conditional_return",
    ] {
        workspace.write(
            format!("{project}/pyproject.toml"),
            &format!("[project]\nname = '{project}'\ndependencies = ['jinja2>=3']\n"),
        );
        workspace.write(
            format!("{project}/src/pkg/templates/spec/page.html"),
            template,
        );
    }
    workspace.write("proven/src/pkg/templates/other/page.html", template);
    let provider = "from functools import lru_cache\nfrom pathlib import Path\nfrom jinja2 import Environment, FileSystemLoader\n_PACKAGE_ROOT = Path(__file__).resolve().parents[1]\n_TEMPLATE_DIR = _PACKAGE_ROOT / 'templates' / 'spec'\n\n@lru_cache(maxsize=1)\ndef environment():\n    loader = FileSystemLoader(str(_TEMPLATE_DIR))\n    return Environment(loader=loader)\n";
    workspace.write("proven/src/pkg/render/jinja.py", provider);
    workspace.write(
        "dynamic_parent/src/pkg/render/jinja.py",
        &provider
            .replace("parents[1]", "parents[INDEX]")
            .replace("_PACKAGE_ROOT =", "INDEX = 1\n_PACKAGE_ROOT ="),
    );
    workspace.write(
        "bare_file_parent/src/pkg/render/jinja.py",
        &provider.replace(
            "Path(__file__).resolve().parents[1]",
            "__file__.resolve().parents[1]",
        ),
    );
    workspace.write(
        "string_resolve/src/pkg/render/jinja.py",
        &provider.replace(
            "Path(__file__).resolve().parents[1]",
            "str(Path(__file__)).resolve().parents[1]",
        ),
    );
    workspace.write(
        "rebound_loader/src/pkg/render/jinja.py",
        &provider.replace(
            "    return Environment(loader=loader)",
            "    loader = unknown\n    return Environment(loader=loader)",
        ),
    );
    workspace.write(
        "shadowed_str/src/pkg/render/jinja.py",
        &provider.replace(
            "    loader = FileSystemLoader",
            "    str = lambda value: unknown\n    loader = FileSystemLoader",
        ),
    );
    workspace.write("shadowed_path/src/pkg/render/jinja.py", &provider.replace("    loader = FileSystemLoader(str(_TEMPLATE_DIR))", "    Path = lambda value: unknown\n    loader = FileSystemLoader(str(Path(__file__).resolve().parents[1] / 'templates' / 'spec'))"));
    workspace.write(
        "shadowed_root/src/pkg/render/jinja.py",
        &provider.replace(
            "    loader = FileSystemLoader",
            "    _TEMPLATE_DIR = unknown\n    loader = FileSystemLoader",
        ),
    );
    workspace.write(
        "conditional_return/src/pkg/render/jinja.py",
        &provider.replace(
            "    return Environment(loader=loader)",
            "    if False:\n        return Environment(loader=loader)",
        ),
    );
    writer::build(&workspace.0, &workspace.db(), None).unwrap();
    let status = |conn: &rusqlite::Connection, path: &str, expression: &str| -> String {
        conn.query_row(
            "SELECT rc.status FROM resolution_coverage rc JOIN path_dictionary pd ON pd.path_id=rc.path_id JOIN coverage_expressions ce ON ce.expression_id=rc.expression_id WHERE pd.path=?1 AND ce.expression=?2",
            [path, expression],
            |row| row.get(0),
        ).unwrap()
    };
    let conn = reader::open(&workspace.db(), &workspace.0).unwrap();
    for expression in ["template.tag.set", "template.filter.e"] {
        assert_eq!(
            status(&conn, "proven/src/pkg/templates/spec/page.html", expression),
            "external",
            "{expression}"
        );
        for path in [
            "proven/src/pkg/templates/other/page.html",
            "dynamic_parent/src/pkg/templates/spec/page.html",
            "bare_file_parent/src/pkg/templates/spec/page.html",
            "string_resolve/src/pkg/templates/spec/page.html",
            "rebound_loader/src/pkg/templates/spec/page.html",
            "shadowed_str/src/pkg/templates/spec/page.html",
            "shadowed_path/src/pkg/templates/spec/page.html",
            "shadowed_root/src/pkg/templates/spec/page.html",
            "conditional_return/src/pkg/templates/spec/page.html",
        ] {
            assert_eq!(
                status(&conn, path, expression),
                "unresolved",
                "{path} {expression}"
            );
        }
    }
    drop(conn);
    workspace.write(
        "proven/src/pkg/templates/spec/page.html",
        "{% set rows = [] %}\n{{ rows|e }}\n<p>Changed</p>\n",
    );
    writer::delta(
        &workspace.0,
        &workspace.db(),
        &[PathBuf::from("proven/src/pkg/templates/spec/page.html")],
    )
    .unwrap();
    let cold_db = workspace.0.join("parent-chain-cold.sqlite");
    writer::build(&workspace.0, &cold_db, None).unwrap();
    let delta = reader::open(&workspace.db(), &workspace.0).unwrap();
    let cold = reader::open(&cold_db, &workspace.0).unwrap();
    for expression in ["template.tag.set", "template.filter.e"] {
        let path = "proven/src/pkg/templates/spec/page.html";
        assert_eq!(status(&delta, path, expression), "external", "{expression}");
        assert_eq!(
            status(&delta, path, expression),
            status(&cold, path, expression)
        );
    }
    commitments::verify(&delta).unwrap();
    commitments::verify(&cold).unwrap();
}
