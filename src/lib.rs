use zed_extension_api::{self as zed, serde_json, settings::LspSettings};

struct Noteboks {}

impl zed::Extension for Noteboks {
    fn new() -> Self {
        Noteboks {}
    }

    fn language_server_command(
        &mut self,
        _language_server_id: &zed::LanguageServerId,
        worktree: &zed::Worktree,
    ) -> zed::Result<zed::Command> {
        let path = worktree
            .which("noteboks-lsp")
            .ok_or_else(|| "noteboks-lsp must be installed and on PATH")?;

        Ok(zed::Command {
            command: path,
            args: Vec::new(),
            env: worktree.shell_env(),
        })
    }

    fn language_server_initialization_options(
        &mut self,
        _language_server_id: &zed::LanguageServerId,
        worktree: &zed::Worktree,
    ) -> zed::Result<Option<serde_json::Value>> {
        let settings = LspSettings::for_worktree("noteboks-lsp", worktree)
            .ok()
            .and_then(|s| s.initialization_options);
        Ok(settings)
    }
}

zed::register_extension!(Noteboks);
