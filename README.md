# Notiz

Notiz is a native, local-first Markdown knowledge base for Linux. It combines a
focused note editor with backlinks, full-text search and an interactive graph,
rendered directly on Wayland through WGPU/Vulkan.

## Current checkpoint

- Markdown files are the source of truth;
- YAML frontmatter with stable UUID, title, timestamps and tags;
- SQLite/FTS5 is a disposable search and relationship index;
- atomic autosave and recoverable `.notiz-trash` deletion;
- note creation, loading, editing, title-based renaming and tag editing;
- editor, rendered preview and split modes;
- `[[wikilinks]]`, aliases, internal navigation and backlinks;
- full-text search across title, body, tags and path;
- global force-directed knowledge graph;
- GPU-rendered graph with pan, zoom and node navigation;
- external Markdown changes detected and reindexed;
- Wayland-only Iced/WGPU build with Vulkan selected by default.

AI, audio, synchronization and mobile support are intentionally outside this
checkpoint. A future vector index will be derived from Markdown and will never
replace the original files.

## Development

```bash
nix-shell
cargo run -p notiz
```

The default vault follows the XDG data directory. To use a specific folder:

```bash
NOTIZ_VAULT=/path/to/my/vault cargo run -p notiz
```

Run the complete test suite with:

```bash
cargo test --workspace
```

## Vault format

Every note is a regular `.md` file. Notiz adds portable frontmatter when a plain
Markdown file is first indexed:

```markdown
---
id: 95cf72fb-995c-4fb3-970a-89e38a4ff9ad
title: "Reunião semanal"
created_at: 2026-08-14T10:00:00-03:00
updated_at: 2026-08-14T10:30:00-03:00
tags:
  - "trabalho"
  - "1-on-1"
---

Conversei com [[João]] sobre o [[Projeto Atlas|Atlas]].
```

The SQLite index lives in the XDG cache directory, outside the vault. Deleting
it does not delete notes; Notiz rebuilds it from Markdown.

## Architecture

- `notiz-core`: note, link and graph models plus Markdown conventions;
- `notiz-storage`: atomic files, SQLite/FTS5 and graph persistence;
- `notiz-services`: application use cases and force-directed layout;
- `notiz-ui`: modular Iced interface, Markdown preview and WGPU graph canvas;
- `notiz-app`: native executable and dependency composition.

## Shortcuts

- `Ctrl+N`: create a note;
- `Ctrl+S`: save immediately;
- `Ctrl+K` or `Ctrl+P`: focus search.

Changes are also saved automatically after a short idle period.

## License

Licensed under the [MIT License](LICENSE).

