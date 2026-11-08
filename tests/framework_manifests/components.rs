use super::*;

const COMPONENT_FILES: &[(&str, &str)] = &[
    ("z.py", "def endpoint(): return None\n"),
    ("a.py", "def endpoint(): return None\n"),
    ("nested/z.py", "def endpoint(): return None\n"),
    ("nested/a.py", "def endpoint(): return None\n"),
    ("nested/deeper/x.py", "def endpoint(): return None\n"),
];

#[cfg(feature = "lang-python")]
#[test]
fn component_assembly_preserves_root_nested_owners_and_insertion_order() {
    let first = VirtualWorkspace::from_files(COMPONENT_FILES)
        .index()
        .unwrap()
        .conn;
    let reversed: Vec<_> = COMPONENT_FILES.iter().copied().rev().collect();
    let second = VirtualWorkspace::from_files(&reversed)
        .index()
        .unwrap()
        .conn;
    assert_eq!(persisted_graph_rows(&first), persisted_graph_rows(&second));
    assert_eq!(
        query_rows(
            &first,
            "SELECT n.id||'|'||p.path FROM nodes n JOIN path_dictionary p ON p.path_id=n.owner_path_id WHERE n.kind='component' ORDER BY n.id",
        ),
        ["component:.|a.py", "component:nested|nested/a.py"]
    );
    let disk = FrameworkHarness::publish_virtual(VirtualWorkspace::from_files(COMPONENT_FILES));
    commitments::verify(&first).unwrap();
    commitments::verify(&second).unwrap();
    commitments::verify(&disk.open()).unwrap();
    assert_eq!(
        persisted_graph_rows(&first),
        persisted_graph_rows(&disk.open())
    );
}
