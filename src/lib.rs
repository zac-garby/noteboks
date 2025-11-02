use zed_extension_api::{self as zed, CodeLabel, CodeLabelSpan, LanguageServerId, Range, lsp::Symbol, settings::LspSettings};

struct Noteboks {

}

impl zed::Extension for Noteboks {
    fn new() -> Self {
        Noteboks {}
    }

    fn language_server_command(
            &mut self,
            _language_server_id: &zed_extension_api::LanguageServerId,
            worktree: &zed_extension_api::Worktree,
    ) -> zed_extension_api::Result<zed_extension_api::Command> {
        let path = worktree
            .which("noteboks-lsp")
            .ok_or_else(|| "noteboks-lsp must be installed")?;

        println!("language server command: {path}");

        Ok(zed::Command{
            command: path,
            args: Vec::new(),
            env: worktree.shell_env(),
        })
    }
}

zed::register_extension!(Noteboks);
