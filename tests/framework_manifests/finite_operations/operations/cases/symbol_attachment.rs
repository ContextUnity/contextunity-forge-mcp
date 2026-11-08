use super::*;

pub(super) fn case() -> Case {
    Case {
        kind: OperationKind::SymbolAttachment,
        vector: Some("symbol_attachments"),
        pack: pack(
            vec![
                semantic(
                    "output",
                    json!({"kind":"symbol_attachment","language":"python","callee":server("expose"),"source":{"kind":"decorated_callable"},"target":{"kind":"keyword","name":"output"},"relation":"uses_model"}),
                ),
                semantic(
                    "exception",
                    json!({"kind":"symbol_attachment","language":"python","callee":server("recover"),"source":{"kind":"decorated_callable"},"target":{"kind":"argument","index":0},"relation":"references"}),
                ),
                semantic(
                    "registered-exception",
                    json!({"kind":"symbol_attachment","language":"python","callee":server("add_recovery"),"source":{"kind":"argument","index":1},"target":{"kind":"argument","index":0},"relation":"references"}),
                ),
            ],
            vec![],
        ),
        files: &[
            (
                "models.py",
                "class Record: pass\nclass Alternate: pass\nclass Fault: pass\nclass OtherFault: pass\nclass RepeatedFault: pass\nclass RepeatedFault: pass\n",
            ),
            (
                "app.py",
                "from webkit import Server\nfrom models import Record, Alternate, Fault, OtherFault\napp = Server()\n@app.expose('/records', output=Record)\ndef list_records(): return None\n@app.recover(Fault)\ndef recover(): return None\ndef registered(): return None\ndef install():\n    app.add_recovery(Fault, registered)\n@app.expose('/missing', output=Missing)\ndef missing(): return None\ndef shadow(app):\n    @app.expose('/shadow', output=Record)\n    def blocked(): return None\n    return blocked\n",
            ),
            (
                "attachments.py",
                "from webkit import Server\nfrom models import Record\napp = Server()\n@app.expose('/absent')\ndef absent_model(): return None\n@app.expose('/dynamic', output=choose_model())\ndef dynamic_model(): return None\n@app.expose('/uncertain', **options)\ndef uncertain_model(): return None\n",
            ),
            (
                "registrations.py",
                "from webkit import Server\nfrom models import Fault, RepeatedFault\napp = Server()\ndef unique_handler(): return 1\ndef handler(): return 1\ndef handler(): return 2\napp.add_recovery(Fault, unique_handler)\napp.add_recovery(Fault, absent_handler)\napp.add_recovery(Fault, handler)\napp.add_recovery(MissingFault, unique_handler)\napp.add_recovery(MissingFault, absent_handler)\napp.add_recovery(MissingFault, handler)\napp.add_recovery(RepeatedFault, unique_handler)\napp.add_recovery(RepeatedFault, absent_handler)\napp.add_recovery(RepeatedFault, handler)\n",
            ),
        ],
        edges: &[
            (
                "app.py",
                "list_records",
                "uses_model",
                "models.py",
                "Record",
            ),
            ("app.py", "recover", "references", "models.py", "Fault"),
            ("app.py", "registered", "references", "models.py", "Fault"),
            (
                "registrations.py",
                "unique_handler",
                "references",
                "models.py",
                "Fault",
            ),
        ],
        negatives: &[
            ("app.py", "missing", "uses_model", "models.py", "%"),
            ("app.py", "blocked", "uses_model", "models.py", "%"),
            (
                "registrations.py",
                "handler",
                "references",
                "models.py",
                "%",
            ),
            (
                "registrations.py",
                "unique_handler",
                "references",
                "models.py",
                "RepeatedFault",
            ),
            (
                "registrations.py",
                "registrations",
                "references",
                "models.py",
                "%",
            ),
        ],
        coverage: &[
            ("app.py", 11, "%Missing%", "unresolved"),
            ("attachments.py", 6, "configured attachment", "unresolved"),
            ("attachments.py", 8, "configured attachment", "unresolved"),
            ("registrations.py", 7, "Fault", "resolved"),
            ("registrations.py", 8, "Fault", "unresolved"),
            ("registrations.py", 9, "Fault", "ambiguous"),
            ("registrations.py", 10, "MissingFault", "unresolved"),
            ("registrations.py", 11, "MissingFault", "unresolved"),
            ("registrations.py", 12, "MissingFault", "ambiguous"),
            ("registrations.py", 13, "RepeatedFault", "ambiguous"),
            ("registrations.py", 14, "RepeatedFault", "ambiguous"),
            ("registrations.py", 15, "RepeatedFault", "ambiguous"),
        ],
        consumer: "app.py",
        rename: &[
            ("webkit", "shorekit"),
            ("Server", "Gateway"),
            ("expose", "publish"),
            ("output", "result_type"),
            ("recover", "rescue"),
        ],
    }
}
