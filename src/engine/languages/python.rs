use super::*;
use crate::engine::ast::{relations, routes};
#[path = "python/fastmcp.rs"]
mod fastmcp;
pub fn language() -> tree_sitter::Language {
    tree_sitter_python::language()
}
pub fn kind(kind: &str) -> Option<&'static str> {
    match kind {
        "function_definition" | "lambda" => Some("function"),
        "class_definition" => Some("class"),
        _ => None,
    }
}

pub struct Python;
pub static PYTHON: Python = Python;
impl LanguageProfile for Python {
    fn id(&self) -> &'static str {
        "python"
    }
    fn extensions(&self) -> &'static [&'static str] {
        &["py", "pyi"]
    }
    fn grammar(&self, _path: &str) -> tree_sitter::Language {
        language()
    }
    fn family(&self) -> LanguageFamily {
        LanguageFamily("python")
    }
    fn symbol_kind(&self, k: &str) -> Option<&'static str> {
        kind(k)
    }
    fn node_prefix(&self, kind: &str) -> &'static str {
        if kind == "class" {
            "class"
        } else {
            "py"
        }
    }
    fn module_name(&self, path: &str) -> String {
        module_stem(path)
            .trim_end_matches("/__init__")
            .replace('/', ".")
    }
    fn extract_file(
        &self,
        path: &str,
        source: &str,
        module: &str,
        facts: &mut Facts,
    ) -> Result<()> {
        parse_file(self, path, source, module, facts)
    }
    fn extract_imports(&self, ctx: &SyntaxContext<'_, '_>, facts: &mut Facts) {
        let (node, source) = (ctx.node, ctx.source);
        let statement = text(node, source);
        let mut add = |expression, alias, module| ctx.import(facts, expression, alias, module);
        match node.kind() {
            "import_from_statement" => {
                let module = field(node, source, "module_name").unwrap_or("").to_owned();
                let mut c = node.walk();
                for child in node.children_by_field_name("name", &mut c) {
                    let name = field(child, source, "name").unwrap_or_else(|| text(child, source));
                    add(
                        name.into(),
                        field(child, source, "alias")
                            .map(str::to_owned)
                            .or_else(|| Some(name.into())),
                        Some(module.clone()),
                    );
                }
                if statement.contains('*') {
                    add("*".into(), None, Some(module));
                }
            }
            "import_statement"
                if statement.starts_with("import ")
                    && !statement.contains("from ")
                    && !statement.contains('"')
                    && !statement.contains('\'') =>
            {
                let mut c = node.walk();
                for child in node.children_by_field_name("name", &mut c) {
                    let name = field(child, source, "name").unwrap_or_else(|| text(child, source));
                    add(
                        name.into(),
                        field(child, source, "alias")
                            .map(str::to_owned)
                            .or_else(|| Some(name.split('.').next().unwrap_or(name).into())),
                        Some(name.into()),
                    );
                }
            }
            _ => {}
        }
    }
    fn extract_calls(&self, ctx: &SyntaxContext<'_, '_>, facts: &mut Facts) {
        if matches!(ctx.node.kind(), "call") {
            call(ctx, facts, false);
        }
    }
    fn external_import(&self, module: &str) -> Option<&'static str> {
        let root = module.split('.').next().unwrap_or(module);
        if is_python_stdlib(root) {
            Some("Python standard library")
        } else if is_python_external_package(root) {
            Some("Python external dependency")
        } else {
            None
        }
    }
    fn builtin(&self, symbol: &str) -> bool {
        matches!(
            symbol,
            "print"
                | "len"
                | "isinstance"
                | "issubclass"
                | "str"
                | "int"
                | "float"
                | "bool"
                | "list"
                | "dict"
                | "set"
                | "tuple"
                | "super"
                | "getattr"
                | "setattr"
                | "hasattr"
                | "delattr"
                | "type"
                | "repr"
                | "open"
                | "iter"
                | "next"
                | "any"
                | "all"
                | "min"
                | "max"
                | "sum"
                | "enumerate"
                | "zip"
                | "sorted"
                | "reversed"
                | "abs"
                | "round"
                | "id"
                | "hash"
                | "callable"
                | "dir"
                | "vars"
                | "help"
                | "range"
                | "frozenset"
                | "bytes"
                | "bytearray"
                | "memoryview"
                | "complex"
                | "slice"
                | "object"
                | "classmethod"
                | "staticmethod"
                | "property"
                | "filter"
                | "map"
                | "format"
                | "pow"
                | "divmod"
                | "bin"
                | "hex"
                | "oct"
                | "ord"
                | "chr"
                | "breakpoint"
                | "compile"
                | "eval"
                | "exec"
                | "BaseException"
                | "Exception"
                | "ArithmeticError"
                | "BufferError"
                | "LookupError"
                | "AssertionError"
                | "AttributeError"
                | "EOFError"
                | "FloatingPointError"
                | "GeneratorExit"
                | "ImportError"
                | "ModuleNotFoundError"
                | "IndexError"
                | "KeyError"
                | "KeyboardInterrupt"
                | "MemoryError"
                | "NameError"
                | "NotImplementedError"
                | "OSError"
                | "OverflowError"
                | "RecursionError"
                | "ReferenceError"
                | "RuntimeError"
                | "StopIteration"
                | "StopAsyncIteration"
                | "SyntaxError"
                | "IndentationError"
                | "TabError"
                | "SystemError"
                | "SystemExit"
                | "TypeError"
                | "UnboundLocalError"
                | "UnicodeError"
                | "UnicodeEncodeError"
                | "UnicodeDecodeError"
                | "UnicodeTranslateError"
                | "ValueError"
                | "ZeroDivisionError"
                | "EnvironmentError"
                | "IOError"
                | "BlockingIOError"
                | "ChildProcessError"
                | "ConnectionError"
                | "BrokenPipeError"
                | "ConnectionAbortedError"
                | "ConnectionRefusedError"
                | "ConnectionResetError"
                | "FileExistsError"
                | "FileNotFoundError"
                | "InterruptedError"
                | "IsADirectoryError"
                | "NotADirectoryError"
                | "PermissionError"
                | "ProcessLookupError"
                | "TimeoutError"
                | "Warning"
                | "UserWarning"
                | "DeprecationWarning"
                | "PendingDeprecationWarning"
                | "SyntaxWarning"
                | "RuntimeWarning"
                | "FutureWarning"
                | "ImportWarning"
                | "UnicodeWarning"
                | "BytesWarning"
                | "ResourceWarning"
        )
    }

    fn normalize_import(&self, owner: &str, module: &str) -> Option<ImportPath> {
        if module.starts_with('.') {
            let count = module.bytes().take_while(|b| *b == b'.').count();
            let path = format!(
                "{}{}",
                "../".repeat(count - 1),
                module[count..].replace('.', "/")
            );
            Some(ImportPath {
                namespace: relative_namespace(owner, &path)?,
                relative: true,
                symbol_path: false,
            })
        } else {
            Some(ImportPath::absolute(module.into()))
        }
    }
    fn doc_comment(&self, node: Syntax<'_>, source: &str) -> String {
        if let Some(first) = node
            .child_by_field_name("body")
            .and_then(|b| b.named_child(0))
        {
            if first.kind() == "expression_statement"
                && first.named_child(0).is_some_and(|n| n.kind() == "string")
            {
                return text(first, source).into();
            }
        }
        ast::doc_comment(node, source)
    }
    fn receiver(&self, name: &str, owner: &Node) -> bool {
        matches!(name, "self" | "cls") && owner.details["receiver_name"] == name
    }
    fn metadata(
        &self,
        node: Syntax<'_>,
        source: &str,
        _name: &str,
        _file: &FileContext,
    ) -> SymbolMetadata {
        let decorators = node
            .parent()
            .filter(|p| p.kind() == "decorated_definition")
            .map(|p| {
                let mut c = p.walk();
                p.named_children(&mut c)
                    .filter(|n| n.kind() == "decorator")
                    .map(|n| text(n, source).to_owned())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let receiver_name = if !decorators.iter().any(|d| d.contains("staticmethod")) {
            node.child_by_field_name("parameters")
                .and_then(|p| p.named_child(0))
                .and_then(|p| {
                    if p.kind() == "identifier" {
                        Some(text(p, source))
                    } else {
                        p.named_child(0)
                            .filter(|n| n.kind() == "identifier")
                            .map(|n| text(n, source))
                    }
                })
                .map(str::to_owned)
        } else {
            None
        };
        SymbolMetadata {
            receiver_name,
            decorators,
            bases: field(node, source, "superclasses").map(str::to_owned),
            is_async: text(node, source).trim_start().starts_with("async "),
            ..Default::default()
        }
    }
    fn extract_relations(&self, ctx: &SyntaxContext<'_, '_>, facts: &mut Facts) {
        if let Some(bases) = ctx.node.child_by_field_name("superclasses") {
            let mut c = bases.walk();
            for base in bases
                .named_children(&mut c)
                .filter(|n| n.kind() != "keyword_argument")
            {
                relations::reference(
                    facts,
                    ctx.owner,
                    relations::type_name(base, ctx.source),
                    "inherits",
                    ctx.line(),
                );
            }
        }
        relations::decorator_references(ctx, facts);
        fastmcp::decorator_candidates(ctx, facts);
        routes::declaration(ctx.node, ctx.source, ctx.owner, facts, ctx.offset);
    }
    fn extract_mutations(&self, ctx: &SyntaxContext<'_, '_>, facts: &mut Facts) {
        relations::mutation(
            ctx,
            facts,
            &[],
            &["assignment", "augmented_assignment"],
            &["attribute"],
        );
    }
    fn extract_routes(
        &self,
        ctx: &SyntaxContext<'_, '_>,
        facts: &mut Facts,
        symbols: &HashMap<usize, String>,
    ) {
        if ctx.node.kind() == "call" {
            routes::registration(ctx.node, ctx.source, ctx.owner, facts, ctx.offset, symbols);
        }
        fastmcp::module_assignment(ctx, facts);
    }
    fn finish(&self, facts: &mut Facts) {
        relations::implicit_fields(facts);
        fastmcp::registrations(facts);
    }
    fn prepare_pattern(&self, pattern: &mut String) -> bool {
        let partial = pattern.trim_end().ends_with(':');
        if partial {
            pattern.push_str("\n    __FORGE_META_BODY\n");
        }
        partial
    }
}

fn is_python_stdlib(pkg: &str) -> bool {
    matches!(
        pkg,
        "__future__"
            | "abc"
            | "aifc"
            | "antigravity"
            | "argparse"
            | "array"
            | "ast"
            | "asynchat"
            | "asyncio"
            | "asyncore"
            | "atexit"
            | "audioop"
            | "base64"
            | "bdb"
            | "binascii"
            | "binhex"
            | "bisect"
            | "builtins"
            | "bz2"
            | "calendar"
            | "cgi"
            | "cgitb"
            | "chunk"
            | "cmath"
            | "cmd"
            | "code"
            | "codecs"
            | "codeop"
            | "collections"
            | "colorsys"
            | "compileall"
            | "concurrent"
            | "configparser"
            | "contextlib"
            | "contextvars"
            | "copy"
            | "copyreg"
            | "cProfile"
            | "crypt"
            | "csv"
            | "ctypes"
            | "curses"
            | "dataclasses"
            | "datetime"
            | "dbm"
            | "decimal"
            | "difflib"
            | "dis"
            | "distutils"
            | "doctest"
            | "email"
            | "encodings"
            | "enum"
            | "errno"
            | "faulthandler"
            | "fcntl"
            | "filecmp"
            | "fileinput"
            | "fnmatch"
            | "fractions"
            | "ftplib"
            | "functools"
            | "gc"
            | "getopt"
            | "getpass"
            | "gettext"
            | "glob"
            | "graphlib"
            | "grp"
            | "gzip"
            | "hashlib"
            | "heapq"
            | "hmac"
            | "html"
            | "http"
            | "idlelib"
            | "imaplib"
            | "imghdr"
            | "imp"
            | "importlib"
            | "inspect"
            | "io"
            | "ipaddress"
            | "itertools"
            | "json"
            | "keyword"
            | "lib2to3"
            | "linecache"
            | "locale"
            | "logging"
            | "lzma"
            | "mailbox"
            | "mailcap"
            | "marshal"
            | "math"
            | "mimetypes"
            | "mmap"
            | "modulefinder"
            | "msilib"
            | "msvcrt"
            | "multiprocessing"
            | "netrc"
            | "nis"
            | "nntplib"
            | "numbers"
            | "operator"
            | "optparse"
            | "os"
            | "ossaudiodev"
            | "parser"
            | "pathlib"
            | "pdb"
            | "pickle"
            | "pickletools"
            | "pipes"
            | "pkgutil"
            | "platform"
            | "plistlib"
            | "poplib"
            | "posix"
            | "posixpath"
            | "pprint"
            | "profile"
            | "pstats"
            | "pty"
            | "pwd"
            | "py_compile"
            | "pyclbr"
            | "pydoc"
            | "queue"
            | "quopri"
            | "random"
            | "re"
            | "readline"
            | "reprlib"
            | "resource"
            | "rlcompleter"
            | "runpy"
            | "sched"
            | "secrets"
            | "select"
            | "selectors"
            | "shelve"
            | "shlex"
            | "shutil"
            | "signal"
            | "site"
            | "smtpd"
            | "smtplib"
            | "sndhdr"
            | "socket"
            | "socketserver"
            | "spwd"
            | "sqlite3"
            | "sre_compile"
            | "sre_constants"
            | "sre_parse"
            | "ssl"
            | "stat"
            | "statistics"
            | "string"
            | "stringprep"
            | "struct"
            | "subprocess"
            | "sunau"
            | "symbol"
            | "symtable"
            | "sys"
            | "sysconfig"
            | "syslog"
            | "tabnanny"
            | "tarfile"
            | "telnetlib"
            | "tempfile"
            | "termios"
            | "test"
            | "textwrap"
            | "threading"
            | "time"
            | "timeit"
            | "tkinter"
            | "token"
            | "tokenize"
            | "tomllib"
            | "trace"
            | "traceback"
            | "tracemalloc"
            | "tty"
            | "turtle"
            | "turtledemo"
            | "types"
            | "typing"
            | "typing_extensions"
            | "unicodedata"
            | "unittest"
            | "urllib"
            | "uu"
            | "uuid"
            | "venv"
            | "warnings"
            | "wave"
            | "weakref"
            | "webbrowser"
            | "winreg"
            | "winsound"
            | "wsgiref"
            | "xdrlib"
            | "xml"
            | "xmlrpc"
            | "zipapp"
            | "zipfile"
            | "zipimport"
            | "zlib"
            | "_thread"
    )
}

fn is_python_external_package(pkg: &str) -> bool {
    matches!(
        pkg,
        "aiofiles"
            | "aiohttp"
            | "aiosignal"
            | "alembic"
            | "anthropic"
            | "anyio"
            | "attrs"
            | "authlib"
            | "babel"
            | "bcrypt"
            | "beautifulsoup4"
            | "boto3"
            | "botocore"
            | "bs4"
            | "celery"
            | "certifi"
            | "cffi"
            | "charset_normalizer"
            | "click"
            | "colorama"
            | "corsheaders"
            | "coverage"
            | "cryptography"
            | "dateutil"
            | "diff_match_patch"
            | "django"
            | "django_filters"
            | "django_redis"
            | "djangorestframework"
            | "dotenv"
            | "drf_spectacular"
            | "elastic_transport"
            | "elasticsearch"
            | "email_validator"
            | "factory"
            | "faker"
            | "fastapi"
            | "flask"
            | "freezegun"
            | "google"
            | "googleapis_common_protos"
            | "greenlet"
            | "grpc"
            | "gunicorn"
            | "h11"
            | "httpcore"
            | "httpx"
            | "huggingface_hub"
            | "idna"
            | "iniconfig"
            | "jinja2"
            | "jose"
            | "jsonschema"
            | "jwt"
            | "kombu"
            | "langchain"
            | "litellm"
            | "lxml"
            | "mako"
            | "markupsafe"
            | "marshmallow"
            | "matplotlib"
            | "mock"
            | "more_itertools"
            | "msgpack"
            | "multidict"
            | "mypy"
            | "mypy_extensions"
            | "numpy"
            | "openai"
            | "openpyxl"
            | "opentelemetry"
            | "packaging"
            | "pandas"
            | "passlib"
            | "PIL"
            | "pillow"
            | "pip"
            | "pkg_resources"
            | "pluggy"
            | "polars"
            | "prometheus_client"
            | "prompt_toolkit"
            | "proto"
            | "protobuf"
            | "psutil"
            | "psycopg"
            | "psycopg2"
            | "py"
            | "pyarrow"
            | "pyasn1"
            | "pycparser"
            | "pydantic"
            | "pydantic_core"
            | "pydantic_settings"
            | "pygments"
            | "pyjwt"
            | "pyopenssl"
            | "pyparsing"
            | "pytest"
            | "pytest_asyncio"
            | "pytest_django"
            | "pytest_mock"
            | "python_dateutil"
            | "pytz"
            | "pyyaml"
            | "redis"
            | "requests"
            | "responses"
            | "rest_framework"
            | "rich"
            | "rsa"
            | "scipy"
            | "sentry_sdk"
            | "setuptools"
            | "six"
            | "sniffio"
            | "snowflake"
            | "soupsieve"
            | "sqlalchemy"
            | "starlette"
            | "stripe"
            | "structlog"
            | "temporalio"
            | "tenacity"
            | "tensorboard"
            | "tensorflow"
            | "tiktoken"
            | "tokenizers"
            | "tomli"
            | "tomli_w"
            | "torch"
            | "tornado"
            | "tqdm"
            | "traitlets"
            | "transformers"
            | "typing_inspect"
            | "tzdata"
            | "urllib3"
            | "uvicorn"
            | "watchfiles"
            | "webencodings"
            | "webtest"
            | "werkzeug"
            | "wheel"
            | "wrapt"
            | "wtforms"
            | "xlrd"
            | "yaml"
            | "yarl"
            | "zstandard"
    )
}

pub static PROFILES: &[&dyn LanguageProfile] = &[&PYTHON];
