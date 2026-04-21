use super::*;

mod model;
mod onboarding;
mod panel_accept;
mod rollout_files;
mod slash_commands;

impl TuiApp {
    pub(crate) fn dismiss_aux_panel(&mut self) {
        self.aux_panel = None;
        self.aux_panel_selection = 0;
    }

    pub(crate) fn dismiss_slash_popup(&mut self) {
        self.input.clear();
        self.reset_slash_selection();
    }

    fn emit_inline_command_echo(&mut self, command: &str) {
        if self.inline_mode {
            self.pending_inline_history
                .push(crate::transcript::format_shell_command_echo(command));
        }
    }

    pub(crate) fn show_aux_panel(&mut self, title: impl Into<String>, body: impl Into<String>) {
        self.aux_panel = Some(AuxPanel {
            title: title.into(),
            content: AuxPanelContent::Text(body.into()),
        });
        self.aux_panel_selection = 0;
    }

    pub(crate) fn show_session_panel(&mut self, sessions: Vec<SessionListEntry>) {
        self.aux_panel_selection = sessions
            .iter()
            .position(|session| session.is_active)
            .unwrap_or(0);
        self.aux_panel = Some(AuxPanel {
            title: "Sessions".to_string(),
            content: AuxPanelContent::SessionList(sessions),
        });
    }

    pub(crate) fn show_model_switch_panel(&mut self) {
        let entries = self.model_switch_entries();
        self.aux_panel_selection = entries
            .iter()
            .position(|entry| entry.is_current)
            .unwrap_or(0);
        self.aux_panel = Some(AuxPanel {
            title: "Models".to_string(),
            content: AuxPanelContent::ModelList(entries),
        });
    }

    pub(crate) fn show_thinking_panel(&mut self) {
        let entries = self.thinking_entries();
        self.aux_panel_selection = entries
            .iter()
            .position(|entry| entry.is_current)
            .unwrap_or(0);
        self.aux_panel = Some(AuxPanel {
            title: "Thinking".to_string(),
            content: AuxPanelContent::ThinkingList(entries),
        });
    }

    #[cfg(test)]
    pub(crate) fn show_model_panel(&mut self) {
        self.show_onboarding_model_panel();
    }

    pub(crate) fn show_onboarding_model_panel(&mut self) {
        let entries = self.onboarding_model_picker_entries();
        self.aux_panel_selection = entries
            .iter()
            .position(|entry| entry.is_current)
            .unwrap_or(0);
        self.aux_panel = Some(AuxPanel {
            title: String::new(),
            content: AuxPanelContent::ModelList(entries),
        });
    }

    pub(crate) fn reset_slash_selection(&mut self) {
        self.slash_selection = 0;
    }

    pub(crate) fn move_slash_selection(&mut self, delta: isize) {
        let suggestions = self.slash_suggestions();
        if suggestions.is_empty() {
            self.slash_selection = 0;
            return;
        }
        let len = suggestions.len() as isize;
        let next = (self.slash_selection as isize + delta).rem_euclid(len);
        self.slash_selection = next as usize;
    }

    pub(crate) fn try_apply_slash_suggestion(&mut self) -> bool {
        let suggestions = self.slash_suggestions();
        if suggestions.is_empty() {
            return false;
        }
        let selected = suggestions[self.slash_selection.min(suggestions.len() - 1)];
        self.input.replace(selected.name);
        self.reset_slash_selection();
        true
    }

    pub(crate) fn move_aux_panel_selection(&mut self, delta: isize) {
        let len = self
            .aux_panel
            .as_ref()
            .map(|panel| match &panel.content {
                AuxPanelContent::SessionList(sessions) => sessions.len(),
                AuxPanelContent::ModelList(models) => models.len(),
                AuxPanelContent::ThinkingList(thinking) => thinking.len(),
                AuxPanelContent::PresetList(presets) => presets.len(),
                AuxPanelContent::Text(_) => 0,
            })
            .unwrap_or(0);
        if len == 0 {
            self.aux_panel_selection = 0;
            return;
        }

        let len = len as isize;
        let next = (self.aux_panel_selection as isize + delta).rem_euclid(len);
        self.aux_panel_selection = next as usize;
    }
}
