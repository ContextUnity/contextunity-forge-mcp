#![cfg(feature = "lang-python")]
use contextunity_forge_mcp::engine::ast;
use serde_json::json;

#[test]
fn literal_export_sets_and_verified_import_returns_describe_lazy_members() {
    let source="_EXPORTS=frozenset({'Codec','decode'})\ndef __getattr__(name: str):\n    if name in _EXPORTS:\n        from . import codecs\n        return getattr(codecs,name)\n    if name == 'Model':\n        from .models import Model\n        return Model\n    raise AttributeError(name)\n";
    let facts = ast::extract("package/api.py", "python", source).unwrap();
    assert!(facts.errors.is_empty(), "{:?}", facts.errors);
    assert_eq!(
        facts.nodes[0].details["lazy_exports"],
        json!([
            {"name":"Codec","module":".codecs","member":"Codec","relative_to_module":false},
            {"name":"decode","module":".codecs","member":"decode","relative_to_module":false},
            {"name":"Model","module":".models","member":"Model","relative_to_module":false},
        ])
    );
}

#[test]
fn finite_fstring_import_module_targets_are_expanded_without_dynamic_names() {
    let source="import importlib\ndef __getattr__(name):\n    if name in {'config','env'}:\n        return importlib.import_module(f'.{name}',__name__)\n    raise AttributeError(name)\n";
    let facts = ast::extract("package/__init__.py", "python", source).unwrap();
    assert_eq!(
        facts.nodes[0].details["lazy_exports"],
        json!([
            {"name":"config","module":".config","member":null,"relative_to_module":true},
            {"name":"env","module":".env","member":null,"relative_to_module":true},
        ])
    );
}

#[test]
fn rebound_parameter_dynamic_export_sets_and_shadowed_getattr_are_not_admitted() {
    for source in [
        "_EXPORTS=compute()\ndef __getattr__(name):\n    if name in _EXPORTS:\n        from . import codecs\n        return getattr(codecs,name)\n",
        "def __getattr__(name):\n    name='Codec'\n    if name == 'Codec':\n        from . import codecs\n        return getattr(codecs,name)\n",
        "getattr=custom\ndef __getattr__(name):\n    if name == 'Codec':\n        from . import codecs\n        return getattr(codecs,name)\n",
        "def __getattr__(name):\n    if name == 'Codec':\n        from fake import getattr\n        from . import codecs\n        return getattr(codecs,name)\n",
        "def __getattr__(name):\n    return None\n    if name == 'Codec':\n        from . import codecs\n        return getattr(codecs,name)\n",
        "def __getattr__(name):\n    arbitrary_call()\n    if name == 'Codec':\n        from . import codecs\n        return getattr(codecs,name)\n",
        "def __getattr__(name):\n    if arbitrary_guard(): return None\n    if name == 'Codec':\n        from . import codecs\n        return getattr(codecs,name)\n",
        "def __getattr__(name):\n    if name == 'Codec':\n        from . import codecs\n        return getattr(codecs,name)\n__getattr__=custom\n",
        "def __getattr__(name):\n    if name == 'Codec':\n        from . import codecs\n        return getattr(codecs,name)\nif flag:\n    __getattr__=custom\n",
        "import importlib\ndef __getattr__(name):\n    if name == 'Dynamic':\n        return importlib.import_module(compute(name))\n",
        "_EXPORTS={'Codec'}\n_EXPORTS.clear()\ndef __getattr__(name):\n    if name in _EXPORTS:\n        from . import codecs\n        return getattr(codecs,name)\n",
        "_EXPORTS=frozenset({'Codec'})\n_EXPORTS -= {'Codec'}\ndef __getattr__(name):\n    if name in _EXPORTS:\n        from . import codecs\n        return getattr(codecs,name)\n",
        "_EXPORTS=frozenset({'Codec'})\nif flag:\n    _EXPORTS=frozenset()\ndef __getattr__(name):\n    if name in _EXPORTS:\n        from . import codecs\n        return getattr(codecs,name)\n",
        "def __getattr__(name):\n    if name == 'Codec':\n        from . import first\n        return getattr(first,name)\n    if name == 'Codec':\n        from . import second\n        return getattr(second,name)\n",
    ] {
        let facts=ast::extract("package/api.py","python",source).unwrap();
        assert!(facts.nodes[0].details.get("lazy_exports").is_none(),"{source}");
    }
}
