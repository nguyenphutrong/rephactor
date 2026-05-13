use zed_extension_api as zed;

struct RephactorExtension;

impl zed::Extension for RephactorExtension {
    fn new() -> Self {
        Self
    }

    fn language_server_command(
        &mut self,
        _language_server_id: &zed::LanguageServerId,
        worktree: &zed::Worktree,
    ) -> zed::Result<zed::Command> {
        let command = worktree
            .which("rephactor")
            .ok_or_else(|| "Could not find rephactor on PATH".to_string())?;

        Ok(zed::Command {
            command,
            args: Vec::new(),
            env: worktree.shell_env(),
        })
    }
}

zed::register_extension!(RephactorExtension);
