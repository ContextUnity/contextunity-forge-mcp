use super::*;

#[test]
fn psycopg_alias_members_require_declared_dependency_and_finite_api() {
    let source = "from psycopg import AsyncConnection\ntype PgConnection = AsyncConnection[object]\nasync def run(conn: PgConnection):\n    await conn.execute('SELECT 1')\n    conn.cursor()\n    conn.unverified()\n";
    let manifest = "[project]\nname = 'typed-connection'\nversion = '0.1.0'\ndependencies = ['psycopg[binary]>=3.1.0']\n";
    for declared in [true, false] {
        let mut files = vec![("src/storage.py", source)];
        if declared {
            files.push(("pyproject.toml", manifest));
        }
        let (_workspace, db) = build_workspace(&files);
        for expression in ["conn.execute", "conn.cursor"] {
            let coverage = persisted_coverage(&db, "src/storage.py", expression);
            assert_eq!(coverage.len(), 1, "{declared} {expression}: {coverage:?}");
            assert_eq!(
                coverage[0].0,
                if declared { "external" } else { "unresolved" },
                "{declared} {expression}: {coverage:?}"
            );
            if declared {
                assert!(
                    coverage[0].1.contains("finite Python framework member"),
                    "{expression}: {coverage:?}"
                );
                assert!(
                    coverage[0].1.contains("psycopg.AsyncConnection"),
                    "{expression}: {coverage:?}"
                );
                assert!(
                    coverage[0].1.contains("line 1"),
                    "{expression}: {coverage:?}"
                );
            }
        }
        let unsupported = persisted_coverage(&db, "src/storage.py", "conn.unverified");
        assert_eq!(unsupported.len(), 1, "{declared}: {unsupported:?}");
        assert_eq!(
            unsupported[0].0, "unresolved",
            "{declared}: {unsupported:?}"
        );
    }
}
