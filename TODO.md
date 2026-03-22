# Noteboks — Future Features

## LSP: TODO item support

Support finding and surfacing `- [ ]` / `- [x]` checkboxes across the vault.

Ideas:
- New LSP command / code lens to list all open TODOs
- Zed's "tasks" integration to run a vault-wide TODO search
- Diagnostics hint on files with many open TODOs

## LSP: Hashtag support (`#tag`)

Logseq-imported notes preserve `#singletag` inline hashtags. Currently the LSP
ignores them. Planned:
- Parse `#tag` tokens in the inline grammar (similar to `citation`)
- Index hashtags per note in `index.rs`
- Support goto-definition on a hashtag (all notes that use it)
- Support find-references on a hashtag

## Block reference support (`((UUID))`)

Logseq uses `((block-uuid))` to embed the content of a specific block from
another note. These were stripped during import (see `import-summary.md` for
affected files).

To support this properly:
- Assign stable UUIDs to paragraphs/blocks in noteboks notes (front matter list?)
- Parse `((uuid))` syntax in the grammar
- LSP: resolve block refs to their source location
- LSP: hover shows the referenced block's content inline
