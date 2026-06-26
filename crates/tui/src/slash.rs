/// One supported slash command exposed by the interactive composer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SlashCommandSpec {
    /// Exact slash command text inserted into the composer.
    pub(crate) name: &'static str,
    /// Short human-readable description shown in the suggestion list.
    pub(crate) description: &'static str,
}

/// Canonical slash commands supported by the interactive TUI.
pub(crate) const SLASH_COMMANDS: [SlashCommandSpec; 18] = [
    SlashCommandSpec {
        name: "/help",
        description: "List the available slash commands",
    },
    SlashCommandSpec {
        name: "/model",
        description: "Show or change the active model",
    },
    SlashCommandSpec {
        name: "/configure",
        description: "Configure provider, model, and API key",
    },
    SlashCommandSpec {
        name: "/config",
        description: "Show the active config.toml (keys masked)",
    },
    SlashCommandSpec {
        name: "/new",
        description: "Create a new session",
    },
    SlashCommandSpec {
        name: "/rename",
        description: "Rename the current session",
    },
    SlashCommandSpec {
        name: "/sessions",
        description: "List sessions and switch between them",
    },
    SlashCommandSpec {
        name: "/skills",
        description: "List available skills",
    },
    SlashCommandSpec {
        name: "/thinking",
        description: "Show or change the thinking mode",
    },
    SlashCommandSpec {
        name: "/reasoning",
        description: "Toggle inline display of model reasoning",
    },
    SlashCommandSpec {
        name: "/status",
        description: "Show current session status",
    },
    SlashCommandSpec {
        name: "/compact",
        description: "Summarize and compact the conversation context",
    },
    SlashCommandSpec {
        name: "/clear",
        description: "Clear the conversation context (keeps the session)",
    },
    SlashCommandSpec {
        name: "/export",
        description: "Export the transcript to a Markdown file",
    },
    SlashCommandSpec {
        name: "/bug",
        description: "Show where to report bugs and feedback",
    },
    SlashCommandSpec {
        name: "/feedback",
        description: "Show where to report bugs and feedback",
    },
    SlashCommandSpec {
        name: "/release-notes",
        description: "Show the version and release notes link",
    },
    SlashCommandSpec {
        name: "/exit",
        description: "Exit the interactive session",
    },
];

/// Computes the visible slash-command suggestions for the current composer text.
pub(crate) fn matching_slash_commands(input: &str) -> Vec<SlashCommandSpec> {
    let trimmed = input.trim_start();
    // Suggestions only appear for a bare command prefix; once the user types a
    // space, the input is treated as a normal prompt or a fully formed command.
    if !trimmed.starts_with('/') || trimmed.contains(char::is_whitespace) {
        return Vec::new();
    }

    SLASH_COMMANDS
        .into_iter()
        .filter(|command| command.name.starts_with(trimmed))
        .collect()
}
