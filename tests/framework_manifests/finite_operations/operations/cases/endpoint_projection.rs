use super::*;

pub(super) fn case() -> Case {
    Case {
                kind: OperationKind::EndpointProjection,
                vector: Some("endpoint_requests"),
                pack: {
                    let mut value = pack(vec![semantic("datasource", json!({"kind":"endpoint_projection","language":"python","callee":{"kind":"imported","module":"tablekit","export":"DataSource"},"attachment":{"callee":{"kind":"imported","module":"tablekit","export":"Table"},"keyword":"sourcefeed"},"url_keyword":"address","method_keyword":"mode","default_method":"GET"}))], vec![syntax("requests", "html", json!({"kind":"html_request_attributes","request_node":"proof_request","context_node":"proof_context","namespace":"proof","context_label":"Request context","prefixes":["zz-"],"methods":{"read":"GET"},"contexts":["target","swap"],"values":"values"}))]);
                    value["route_calls"] = json!([
                        {"callee":"map_get","method":"GET","path_argument":0,"handler_argument":1},
                        {"callee":"map_post","method":"POST","path_argument":0,"handler_argument":1},
                    ]);
                    value
                },
                files: &[
                    ("backend.py", "def get_records(): return None\ndef post_records(): return None\nmap_get('/api/records/', get_records)\nmap_post('/api/records/', post_records)\nmap_get('/api/duplicate/', get_records)\nmap_get('/api/duplicate/', post_records)\nmap_get('/items/{id}/', get_records)\n"),
                    ("spec.py", "from tablekit import Table, DataSource\ndef get_table(): return Table(sourcefeed=DataSource(address='/api/records/'))\ndef post_table(): return Table(sourcefeed=DataSource(address='/api/records/', mode='post'))\ndef unknown_table(): return Table(sourcefeed=DataSource(address=dynamic_url, mode=dynamic_method))\ndef detached(): return DataSource(address='/api/records/')\ndef missing_url(): return Table(sourcefeed=DataSource())\ndef duplicate_route(): return Table(sourcefeed=DataSource(address='/api/duplicate/'))\n"),
                    ("requests.html", "<button zz-read='/api/records/' zz-target='#result' zz-swap='outerHTML'></button>\n<button zz-read='/api/records/?state=ready'></button>\n<button zz-read='/items/${id}/'></button>\n<button zz-read='/items/{{ id }}/'></button>\n<button zz-read='/items/<id>/'></button>\n<button zz-read='javascript:open()'></button>\n<button zz-read='https://example.invalid/api/records/'></button>\n<button zz-read='//example.invalid/api/records/'></button>\n<button zz-read=''></button>\n<button zz-read='#result'></button>\n"),
                ],
                edges: &[
                    ("spec.py", "get_table", "calls_endpoint", "backend.py", "GET /api/records/"),
                    ("spec.py", "post_table", "calls_endpoint", "backend.py", "POST /api/records/"),
                    ("backend.py", "GET /api/records/", "handles", "backend.py", "get_records"),
                    ("backend.py", "POST /api/records/", "handles", "backend.py", "post_records"),
                    ("requests.html", "GET /api/records/", "calls_endpoint", "backend.py", "GET /api/records/"),
                    ("requests.html", "GET /api/records/?state=ready", "calls_endpoint", "backend.py", "GET /api/records/"),
                ],
                negatives: &[
                    ("spec.py", "get_table", "calls_endpoint", "backend.py", "POST /api/records/"), ("spec.py", "post_table", "calls_endpoint", "backend.py", "GET /api/records/"),
                    ("spec.py", "unknown_table", "calls_endpoint", "backend.py", "%"), ("spec.py", "detached", "calls_endpoint", "backend.py", "%"),
                    ("spec.py", "missing_url", "calls_endpoint", "backend.py", "%"), ("spec.py", "duplicate_route", "calls_endpoint", "backend.py", "%"),
                    ("requests.html", "GET /items/${id}/", "calls_endpoint", "backend.py", "%"), ("requests.html", "GET /items/{{ id }}/", "calls_endpoint", "backend.py", "%"),
                    ("requests.html", "GET /items/<id>/", "calls_endpoint", "backend.py", "%"), ("requests.html", "GET javascript:open()", "calls_endpoint", "backend.py", "%"),
                    ("requests.html", "GET https://example.invalid/api/records/", "calls_endpoint", "backend.py", "%"), ("requests.html", "GET //example.invalid/api/records/", "calls_endpoint", "backend.py", "%"),
                    ("requests.html", "GET ", "calls_endpoint", "backend.py", "%"), ("requests.html", "GET #result", "calls_endpoint", "backend.py", "%"),
                ],
                coverage: &[("spec.py", 6, "%dynamic endpoint%", "unresolved"), ("spec.py", 7, "%/api/duplicate/%", "ambiguous"),
                    ("requests.html", 3, "GET /items/${id}/", "unresolved"),
                    ("requests.html", 4, "GET /items/{{ id }}/", "unresolved"),
                    ("requests.html", 5, "GET /items/<id>/", "unresolved"),
                    ("requests.html", 6, "GET javascript:open()", "unresolved"),
                    ("requests.html", 7, "GET https://example.invalid/api/records/", "external"),
                    ("requests.html", 8, "GET //example.invalid/api/records/", "external"),
                    ("requests.html", 9, "GET ", "resolved"),
                    ("requests.html", 10, "GET #result", "resolved")],
                consumer: "spec.py",
                rename: &[("tablekit", "matrixkit"), ("DataSource", "Feed"), ("Table", "Panel"), ("sourcefeed", "feed"), ("address", "location"), ("mode", "verb"), ("zz-", "qq-")],
            }
}

pub(super) fn assert_unknown_endpoint_inputs(payloads: &[Value], phase: &str) {
    assert!(
        payloads.iter().any(|fact| {
            fact["url"]["kind"] == "unknown" && fact["method"]["kind"] == "unknown"
        }),
        "{phase}: explicit uncertain endpoint inputs survive persistence"
    );
}
