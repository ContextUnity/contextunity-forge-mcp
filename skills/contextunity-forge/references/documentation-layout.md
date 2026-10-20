# Repository documentation layout

Use these page roles in an indexed repository. Create a directory only when it has pages, and state current behavior or commands rather than history.

| Path | Owns |
| --- | --- |
| `docs/AGENTS.md` | Authoring rules for the documentation tree |
| `docs/README.md` | Navigation |
| `docs/roadmap.md` | One or two paragraphs of strategic context |
| `docs/architecture/` | Current system topology |
| `docs/adr/` | Accepted decisions |
| `docs/reference/` | Interfaces, configuration, CLI, tools, schemas |
| `docs/runbooks/` | Repository commands and operational sequence |
| `docs/testing/` | Repository verification |
| `docs/milestones/` | Admitted commitments; `archive/` keeps receipts |
| `docs/plans/` | Proposals before admission |
| `docs/archive/` | Retained history outside the documentation index |

The repository root `AGENTS.md` owns local verification commands and Git permissions. Forge schemas belong in the Forge repository's `docs/reference/`; markup for pages indexed by Forge belongs in its `docs/AGENTS.md`.

When creating a docs tree in another repository, adapt the [README example](docs-readme.md.example)
and [documentation-rules example](docs-agents.md.example). Copy their contents to `docs/README.md`
and `docs/AGENTS.md`, then resolve relative links from those destination files. Replace or remove
every example path that the target repository does not maintain.
