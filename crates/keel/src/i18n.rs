//! Keel's interface texts in Italian and English.
//!
//! Every text lives in one table per language, so the compiler checks that
//! none is missing in either.

/// The language of the interface.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UiLanguage {
    Italian,
    English,
}

impl UiLanguage {
    /// The language from the `keel.language` setting: `it`, `en`, or `auto`
    /// to follow the Mac. Languages other than Italian fall back to English.
    pub fn from_setting(setting: Option<&str>, system_locale: Option<&str>) -> Self {
        match setting {
            Some("it") => UiLanguage::Italian,
            Some("en") => UiLanguage::English,
            _ if system_locale
                .is_some_and(|locale| locale.to_ascii_lowercase().starts_with("it")) =>
            {
                UiLanguage::Italian
            }
            _ => UiLanguage::English,
        }
    }

    /// The Mac's preferred language, such as `it-IT`.
    pub fn system_locale() -> Option<String> {
        sys_locale::get_locale()
    }

    /// The code extensions see as `vscode.env.language`.
    pub fn code(self) -> &'static str {
        match self {
            UiLanguage::Italian => "it",
            UiLanguage::English => "en",
        }
    }

    pub fn strings(self) -> &'static Strings {
        match self {
            UiLanguage::Italian => &ITALIAN,
            UiLanguage::English => &ENGLISH,
        }
    }
}

/// The texts of the interface.
pub struct Strings {
    pub save: &'static str,
    pub dont_save: &'static str,
    pub cancel: &'static str,
    pub ok: &'static str,
    pub untitled: &'static str,
    pub untitled_file_name: &'static str,
    pub changes_lost: &'static str,
    pub type_a_command: &'static str,
    pub choose_language: &'static str,
    pub command_save: &'static str,
    pub command_close_window: &'static str,
    pub command_language: &'static str,
    pub command_theme: &'static str,
    pub command_quit: &'static str,
    pub menu_file: &'static str,
    pub menu_edit: &'static str,
    pub menu_view: &'static str,
    pub menu_services: &'static str,
    pub menu_theme: &'static str,
    pub menu_language: &'static str,
    pub menu_close_window: &'static str,
    pub menu_undo: &'static str,
    pub menu_redo: &'static str,
    pub menu_cut: &'static str,
    pub menu_copy: &'static str,
    pub menu_paste: &'static str,
    pub menu_select_all: &'static str,
    pub menu_find: &'static str,
    pub menu_replace: &'static str,
    pub menu_command_palette: &'static str,
    pub choose_theme: &'static str,
    pub settings_unavailable: &'static str,
    pub search_commands: &'static str,
    pub toggle_sidebar: &'static str,
    pub toggle_panel: &'static str,
    pub toggle_agent: &'static str,
    pub view_explorer: &'static str,
    pub view_search: &'static str,
    pub view_source_control: &'static str,
    pub view_extensions: &'static str,
    pub open_editors: &'static str,
    pub installed_extensions: &'static str,
    pub no_extensions: &'static str,
    pub panel_problems: &'static str,
    pub panel_output: &'static str,
    pub panel_terminal: &'static str,
    terminal_failed: &'static str,
    pub no_terminal: &'static str,
    pub panel_ports: &'static str,
    pub no_output: &'static str,
    pub plain_text: &'static str,
    pub close_editor: &'static str,
    pub welcome_open: &'static str,
    pub welcome_new_file: &'static str,
    pub menu_new_window: &'static str,
    pub menu_open_recent: &'static str,
    pub recent_projects: &'static str,
    pub open_recent_title: &'static str,
    pub no_recent_projects: &'static str,
    pub command_open: &'static str,
    pub command_new_file: &'static str,
    pub command_new_window: &'static str,
    pub command_open_recent: &'static str,
    pub no_folder: &'static str,
    pub explorer_new_file: &'static str,
    pub explorer_new_folder: &'static str,
    pub explorer_refresh: &'static str,
    pub explorer_collapse: &'static str,
    pub explorer_rename: &'static str,
    pub explorer_trash: &'static str,
    pub explorer_reveal: &'static str,
    pub explorer_copy_path: &'static str,
    pub explorer_copy_relative: &'static str,
    pub explorer_move: &'static str,
    pub trash_detail: &'static str,
    pub command_explorer_new_file: &'static str,
    pub command_explorer_new_folder: &'static str,
    pub command_explorer_refresh: &'static str,
    pub command_explorer_collapse: &'static str,
    pub tab_pin: &'static str,
    pub tab_unpin: &'static str,
    pub tab_close_others: &'static str,
    pub tab_close_right: &'static str,
    pub tab_close_all: &'static str,
    pub tab_split: &'static str,
    pub menu_close_editor: &'static str,
    pub menu_reopen_editor: &'static str,
    pub menu_split_editor: &'static str,
    pub command_close_editor: &'static str,
    pub command_close_others: &'static str,
    pub command_close_group: &'static str,
    pub command_close_all: &'static str,
    pub command_reopen_editor: &'static str,
    pub command_pin_editor: &'static str,
    pub command_keep_editor: &'static str,
    pub command_next_editor: &'static str,
    pub command_previous_editor: &'static str,
    pub command_split_editor: &'static str,
    pub command_focus_group_1: &'static str,
    pub command_focus_group_2: &'static str,
    pub command_focus_group_3: &'static str,
    pub search_files: &'static str,
    pub quick_open_placeholder: &'static str,
    pub menu_go: &'static str,
    pub menu_go_to_file: &'static str,
    pub menu_go_to_line: &'static str,
    pub menu_rename_terminal: &'static str,
    pub command_quick_open: &'static str,
    pub command_go_to_line: &'static str,
    pub command_comment_line: &'static str,
    pub command_block_comment: &'static str,
    pub command_move_up: &'static str,
    pub command_move_down: &'static str,
    pub command_copy_up: &'static str,
    pub command_copy_down: &'static str,
    pub command_delete_lines: &'static str,
    pub command_line_after: &'static str,
    pub command_line_before: &'static str,
    pub command_word_wrap: &'static str,
    pub command_zoom_in: &'static str,
    pub command_zoom_out: &'static str,
    pub command_zoom_reset: &'static str,
    pub menu_selection: &'static str,
    pub menu_word_wrap: &'static str,
    pub menu_zoom_in: &'static str,
    pub menu_zoom_out: &'static str,
    pub menu_zoom_reset: &'static str,
    pub overwrite: &'static str,
    pub use_disk_version: &'static str,
    pub large_file_detail: &'static str,
    pub open_anyway: &'static str,
    pub search_placeholder: &'static str,
    pub include_placeholder: &'static str,
    pub exclude_placeholder: &'static str,
    pub match_case: &'static str,
    pub whole_word: &'static str,
    pub use_regex: &'static str,
    pub folders_toggle: &'static str,
    pub searching: &'static str,
    pub no_results: &'static str,
    pub results_limited: &'static str,
    pub replace_placeholder: &'static str,
    pub toggle_replace: &'static str,
    pub replace_all: &'static str,
    pub replace_in_file: &'static str,
    pub replace_detail: &'static str,
    pub overwrite_detail: &'static str,
    pub command_toggle_sidebar: &'static str,
    pub command_toggle_panel: &'static str,
    pub command_toggle_agent: &'static str,
    pub command_explorer: &'static str,
    pub command_search: &'static str,
    pub command_source_control: &'static str,
    pub command_extensions: &'static str,
    pub command_problems: &'static str,
    pub command_output: &'static str,
    pub command_terminal: &'static str,
    pub terminal_name: &'static str,
    pub hunk_changed: &'static str,
    pub hunk_preview: &'static str,
    pub hunk_stage: &'static str,
    pub hunk_revert: &'static str,
    pub command_stage_change: &'static str,
    pub command_revert_change: &'static str,
    pub command_next_change: &'static str,
    pub command_previous_change: &'static str,
    pub diff_working_tree: &'static str,
    pub diff_index: &'static str,
    pub diff_conflict: &'static str,
    pub diff_side_by_side: &'static str,
    pub diff_inline: &'static str,
    pub diff_save_resolution: &'static str,
    pub scm_message: &'static str,
    pub scm_commit: &'static str,
    pub scm_commit_amend: &'static str,
    pub scm_stage: &'static str,
    pub scm_unstage: &'static str,
    pub scm_stage_all: &'static str,
    pub scm_unstage_all: &'static str,
    pub scm_discard: &'static str,
    pub scm_discard_all: &'static str,
    pub scm_discard_detail: &'static str,
    pub scm_discard_button: &'static str,
    pub scm_no_message: &'static str,
    pub scm_stage_and_commit_question: &'static str,
    pub scm_stage_and_commit: &'static str,
    pub scm_push: &'static str,
    pub scm_pull: &'static str,
    pub scm_fetch: &'static str,
    pub scm_sync: &'static str,
    pub scm_publish: &'static str,
    pub scm_stash: &'static str,
    pub scm_stash_pop: &'static str,
    pub scm_refresh: &'static str,
    pub scm_checkout: &'static str,
    pub scm_create_branch: &'static str,
    pub scm_new_branch: &'static str,
    pub scm_remote_branch: &'static str,
    pub scm_choose_branch: &'static str,
    pub scm_branch_name: &'static str,
    pub scm_no_repository: &'static str,
    pub scm_clean: &'static str,
    pub scm_staged: &'static str,
    pub scm_changes: &'static str,
    pub scm_conflicts: &'static str,
    pub scm_open_file: &'static str,
    scm_commit_on: &'static str,
    scm_discard_question: &'static str,
    scm_discard_many_question: &'static str,
    pub command_git_commit: &'static str,
    pub command_git_amend: &'static str,
    pub command_git_stage_all: &'static str,
    pub command_git_unstage_all: &'static str,
    pub command_git_discard_all: &'static str,
    pub command_git_push: &'static str,
    pub command_git_pull: &'static str,
    pub command_git_fetch: &'static str,
    pub command_git_sync: &'static str,
    pub command_git_stash: &'static str,
    pub command_git_stash_pop: &'static str,
    pub command_git_refresh: &'static str,
    pub command_git_checkout: &'static str,
    pub command_git_branch: &'static str,
    pub output_extensions: &'static str,
    pub output_clear: &'static str,
    pub no_ports: &'static str,
    pub port: &'static str,
    pub port_address: &'static str,
    pub port_process: &'static str,
    pub open_in_browser: &'static str,
    port_opened: &'static str,
    pub npm_scripts: &'static str,
    pub no_npm_scripts: &'static str,
    pub run_npm_script: &'static str,
    pub command_run_npm_script: &'static str,
    pub find_in_terminal: &'static str,
    pub find_older: &'static str,
    pub find_newer: &'static str,
    pub close_find: &'static str,
    pub terminal_new: &'static str,
    pub terminal_split: &'static str,
    pub terminal_rename: &'static str,
    pub terminal_kill: &'static str,
    pub maximize_panel: &'static str,
    pub restore_panel: &'static str,
    pub open_claude_code: &'static str,
    pub claude_code_hint: &'static str,
    pub close_claude_code: &'static str,
    pub command_new_terminal: &'static str,
    pub command_split_terminal: &'static str,
    pub command_kill_terminal: &'static str,
    pub command_rename_terminal: &'static str,
    pub command_next_terminal: &'static str,
    pub command_previous_terminal: &'static str,
    pub command_maximize_panel: &'static str,
    pub command_claude_code: &'static str,
    pub command_claude_code_panel: &'static str,
    save_changes: &'static str,
    could_not_save: &'static str,
    could_not_use: &'static str,
    could_not_start: &'static str,
    could_not_open: &'static str,
    could_not_change: &'static str,
    go_to_line: &'static str,
    changed_on_disk: &'static str,
    not_text: &'static str,
    replace_question: &'static str,
    replaced_summary: &'static str,
    results: [&'static str; 4],
    large_file: &'static str,
    overwrite_question: &'static str,
    move_question: &'static str,
    trash_question: &'static str,
    /// Unused while every part of the window is built; kept for the next.
    #[allow(dead_code)]
    coming_in_phase: &'static str,
    line_column: &'static str,
    spaces: &'static str,
    tab_size: &'static str,
    pub output_languages: &'static str,
    pub lsp_started: &'static str,
    pub lsp_installing_text: &'static str,
    pub lsp_failed_text: &'static str,
    pub prerequisite_missing_text: &'static str,
    pub prerequisite_detail_text: &'static str,
    pub install_in_terminal: &'static str,
    pub install_terminal_name_text: &'static str,
    pub lsp_stopped_text: &'static str,
    pub lsp_restarting: &'static str,
    pub lsp_restarted: &'static str,
    pub command_restart_language_servers: &'static str,
    pub problems_none: &'static str,
    pub command_next_problem: &'static str,
    pub command_previous_problem: &'static str,
    pub command_trigger_parameter_hints: &'static str,
    pub command_trigger_suggest: &'static str,
    pub lsp_not_supported: &'static str,
    pub lsp_no_definition: &'static str,
    pub lsp_no_references: &'static str,
    pub lsp_references_text: &'static str,
    pub lsp_definitions_text: &'static str,
    pub lsp_cannot_rename: &'static str,
    pub lsp_rename_title: &'static str,
    pub lsp_renamed_text: &'static str,
    pub lsp_symbols_in_file: &'static str,
    pub lsp_symbols_in_workspace: &'static str,
    pub command_reveal_definition: &'static str,
    pub command_go_to_implementation: &'static str,
    pub command_go_to_type_definition: &'static str,
    pub command_go_to_references: &'static str,
    pub command_rename_symbol: &'static str,
    pub command_symbol_in_file: &'static str,
    pub command_symbol_in_workspace: &'static str,
    pub format_timed_out: &'static str,
    pub command_format_document: &'static str,
    pub command_organize_imports: &'static str,
    pub preview_title_text: &'static str,
    pub command_markdown_preview: &'static str,
    pub command_markdown_preview_side: &'static str,
    pub image_fit: &'static str,
    pub pdf_search_placeholder: &'static str,
    pub pdf_thumbnails: &'static str,
    pub pdf_page_text: &'static str,
    pub find_previous: &'static str,
    pub find_next: &'static str,
    pub command_git_graph: &'static str,
    pub graph_no_repository: &'static str,
    pub graph_search_placeholder: &'static str,
    pub graph_uncommitted: &'static str,
    pub graph_all_branches: &'static str,
    pub graph_branches: &'static str,
    pub graph_column_graph: &'static str,
    pub graph_column_description: &'static str,
    pub graph_column_date: &'static str,
    pub graph_column_author: &'static str,
    pub graph_column_commit: &'static str,
    pub graph_load_more_text: &'static str,
    pub graph_parents: &'static str,
    pub graph_loading: &'static str,
    pub graph_checkout: &'static str,
    pub graph_create_branch: &'static str,
    pub graph_create_tag: &'static str,
    pub graph_cherry_pick: &'static str,
    pub graph_revert: &'static str,
    pub graph_merge: &'static str,
    pub graph_delete: &'static str,
    pub graph_copy_hash: &'static str,
    pub graph_copy_subject: &'static str,
    pub graph_tag_name: &'static str,
    pub graph_checkout_branch_text: &'static str,
    pub graph_merge_branch_text: &'static str,
    pub graph_delete_branch_text: &'static str,
    pub graph_delete_tag_text: &'static str,
    pub graph_checkout_question_text: &'static str,
    pub graph_cherry_pick_question_text: &'static str,
    pub graph_revert_question_text: &'static str,
    pub graph_merge_question_text: &'static str,
    pub graph_delete_branch_question_text: &'static str,
    pub graph_delete_tag_question_text: &'static str,
    pub command_toggle_blame: &'static str,
    pub command_file_history: &'static str,
    pub blame_not_in_git: &'static str,
    pub blame_uncommitted: &'static str,
    pub file_history_title_text: &'static str,
    pub extensions_install_vsix: &'static str,
    pub extensions_restart: &'static str,
    pub extensions_failed: &'static str,
    pub extension_installed_text: &'static str,
    pub extension_uninstall_question_text: &'static str,
    pub extension_tab_title_text: &'static str,
    pub extension_enable: &'static str,
    pub extension_disable: &'static str,
    pub extension_disable_project: &'static str,
    pub extension_uninstall: &'static str,
    pub extension_disabled_badge: &'static str,
    pub extension_missing: &'static str,
    pub extension_commands: &'static str,
    pub extension_keybindings: &'static str,
    pub extension_themes: &'static str,
    pub extension_icon_themes: &'static str,
    pub extension_snippets: &'static str,
    pub extension_languages: &'static str,
    pub extensions_search_placeholder: &'static str,
    pub extensions_searching: &'static str,
    pub extensions_none_found: &'static str,
    pub extensions_check_updates: &'static str,
    pub extensions_update_all: &'static str,
    pub extensions_up_to_date: &'static str,
    pub extension_updates_found_text: &'static str,
    pub extension_install: &'static str,
    pub extension_update: &'static str,
    pub extension_installing: &'static str,
    pub extension_installed_badge: &'static str,
    pub command_icon_theme: &'static str,
    pub choose_icon_theme: &'static str,
    pub icon_theme_none: &'static str,
    pub extensions_restarted_text: &'static str,
    pub extensions_gave_up: &'static str,
    pub extensions_import_cursor: &'static str,
    pub command_import_cursor: &'static str,
    pub cursor_heading: &'static str,
    pub cursor_close: &'static str,
    pub cursor_loading: &'static str,
    pub cursor_none: &'static str,
    pub cursor_install_all_text: &'static str,
    pub cursor_group_install: &'static str,
    pub cursor_group_keel: &'static str,
    pub cursor_group_blocked: &'static str,
    pub cursor_installed: &'static str,
    pub cursor_local: &'static str,
    pub cursor_partial_text: &'static str,
    pub cursor_blocked_text: &'static str,
    pub cursor_needs_text: &'static str,
    pub cursor_not_on_open_vsx: &'static str,
    pub cursor_builtin_prettier: &'static str,
    pub cursor_builtin_git_graph: &'static str,
    pub cursor_builtin_claude_code: &'static str,
    pub cursor_builtin_pdf: &'static str,
    pub cursor_builtin_markdown_preview: &'static str,
    pub cursor_builtin_yaml: &'static str,
    pub cursor_builtin_typescript: &'static str,
    pub cursor_blocker_cursor_only: &'static str,
    pub cursor_blocker_debugger: &'static str,
    pub cursor_blocker_notebooks: &'static str,
    pub cursor_blocker_custom_editors: &'static str,
    pub cursor_blocker_views: &'static str,
    pub cursor_blocker_webviews: &'static str,
    pub cursor_blocker_language_server: &'static str,
    pub command_insert_at_mention: &'static str,
    pub claude_not_connected: &'static str,
    pub command_accept_proposed: &'static str,
    pub command_reject_proposed: &'static str,
    pub claude_proposed_edit: &'static str,
    pub claude_accept: &'static str,
    pub claude_reject: &'static str,
    pub agent_view_terminal: &'static str,
    pub agent_view_agent: &'static str,
    pub agent_placeholder: &'static str,
    pub agent_no_data: &'static str,
    pub agent_stopped: &'static str,
    pub agent_stopped_text: &'static str,
    pub agent_reason_cancelled: &'static str,
    pub agent_reason_limit: &'static str,
    pub agent_reason_refusal: &'static str,
    pub agent_history: &'static str,
    pub agent_new: &'static str,
    pub agent_installing: &'static str,
    pub agent_starting: &'static str,
    pub agent_empty: &'static str,
    pub agent_start: &'static str,
    pub agent_working: &'static str,
    pub agent_thinking: &'static str,
    pub agent_plan_text: &'static str,
    pub agent_permission_text: &'static str,
    pub agent_mode: &'static str,
    pub agent_stop: &'static str,
    pub agent_send: &'static str,
    pub agent_add_file: &'static str,
    pub command_open_agent: &'static str,
    pub command_new_agent: &'static str,
    pub command_add_to_agent: &'static str,
    pub agent_changes_text: &'static str,
    pub agent_accept_all: &'static str,
    pub agent_reject_all: &'static str,
    pub agent_new_file: &'static str,
    pub agent_change_of_claude: &'static str,
}

impl Strings {
    /// The palette title of a Keel command, or `None` for commands the
    /// palette does not list, such as the palette itself.
    pub fn command_title(&self, id: &str) -> Option<&'static str> {
        Some(match id {
            "workbench.action.files.save" => self.command_save,
            "workbench.action.closeWindow" => self.command_close_window,
            "workbench.action.quit" => self.command_quit,
            "workbench.action.selectTheme" => self.command_theme,
            "workbench.action.selectIconTheme" => self.command_icon_theme,
            "keel.extensions.importFromCursor" => self.command_import_cursor,
            "claude-code.insertAtMentioned" => self.command_insert_at_mention,
            "claude-code.acceptProposedDiff" => self.command_accept_proposed,
            "keel.agent.open" => self.command_open_agent,
            "keel.agent.newConversation" => self.command_new_agent,
            "keel.agent.addToConversation" => self.command_add_to_agent,
            "claude-code.rejectProposedDiff" => self.command_reject_proposed,
            "keel.selectLanguage" => self.command_language,
            "workbench.action.toggleSidebarVisibility" => self.command_toggle_sidebar,
            "workbench.action.togglePanel" => self.command_toggle_panel,
            "workbench.action.toggleAuxiliaryBar" => self.command_toggle_agent,
            "workbench.view.explorer" => self.command_explorer,
            "workbench.view.search" => self.command_search,
            "workbench.view.scm" => self.command_source_control,
            "workbench.view.extensions" => self.command_extensions,
            "workbench.actions.view.problems" => self.command_problems,
            "workbench.action.output.toggleOutput" => self.command_output,
            "workbench.action.terminal.toggleTerminal" => self.command_terminal,
            "workbench.action.terminal.new" => self.command_new_terminal,
            "workbench.action.terminal.split" => self.command_split_terminal,
            "workbench.action.terminal.kill" => self.command_kill_terminal,
            "workbench.action.terminal.rename" => self.command_rename_terminal,
            "workbench.action.terminal.focusNext" => self.command_next_terminal,
            "workbench.action.terminal.focusPrevious" => self.command_previous_terminal,
            "workbench.action.toggleMaximizedPanel" => self.command_maximize_panel,
            "keel.claudeCode.open" => self.command_claude_code,
            "keel.claudeCode.openInPanel" => self.command_claude_code_panel,
            "keel.npm.runScript" => self.command_run_npm_script,
            "git.stageSelectedRanges" => self.command_stage_change,
            "git.revertSelectedRanges" => self.command_revert_change,
            "workbench.action.editor.nextChange" => self.command_next_change,
            "workbench.action.editor.previousChange" => self.command_previous_change,
            "git.commit" => self.command_git_commit,
            "git.commitStagedAmend" => self.command_git_amend,
            "git.stageAll" => self.command_git_stage_all,
            "git.unstageAll" => self.command_git_unstage_all,
            "git.cleanAll" => self.command_git_discard_all,
            "git.push" => self.command_git_push,
            "keel.restartLanguageServers" => self.command_restart_language_servers,
            "editor.action.marker.nextInFiles" => self.command_next_problem,
            "editor.action.triggerParameterHints" => self.command_trigger_parameter_hints,
            "editor.action.revealDefinition" => self.command_reveal_definition,
            "editor.action.goToImplementation" => self.command_go_to_implementation,
            "editor.action.goToTypeDefinition" => self.command_go_to_type_definition,
            "editor.action.goToReferences" => self.command_go_to_references,
            "editor.action.rename" => self.command_rename_symbol,
            "editor.action.formatDocument" => self.command_format_document,
            "markdown.showPreview" => self.command_markdown_preview,
            "git-graph.view" => self.command_git_graph,
            "gitlens.toggleFileBlame" => self.command_toggle_blame,
            "workbench.extensions.action.installVSIX" => self.extensions_install_vsix,
            "workbench.action.restartExtensionHost" => self.extensions_restart,
            "workbench.extensions.action.checkForUpdates" => self.extensions_check_updates,
            "workbench.extensions.action.updateAllExtensions" => self.extensions_update_all,
            "keel.showFileHistory" => self.command_file_history,
            "markdown.showPreviewToSide" => self.command_markdown_preview_side,
            "editor.action.organizeImports" => self.command_organize_imports,
            "workbench.action.gotoSymbol" => self.command_symbol_in_file,
            "workbench.action.showAllSymbols" => self.command_symbol_in_workspace,
            "editor.action.triggerSuggest" => self.command_trigger_suggest,
            "editor.action.marker.prevInFiles" => self.command_previous_problem,
            "git.pull" => self.command_git_pull,
            "git.fetch" => self.command_git_fetch,
            "git.sync" => self.command_git_sync,
            "git.stash" => self.command_git_stash,
            "git.stashPop" => self.command_git_stash_pop,
            "git.refresh" => self.command_git_refresh,
            "git.checkout" => self.command_git_checkout,
            "git.branch" => self.command_git_branch,
            "workbench.action.files.openFileFolder" => self.command_open,
            "workbench.action.files.newUntitledFile" => self.command_new_file,
            "workbench.action.newWindow" => self.command_new_window,
            "workbench.action.openRecent" => self.command_open_recent,
            "explorer.newFile" => self.command_explorer_new_file,
            "explorer.newFolder" => self.command_explorer_new_folder,
            "workbench.files.action.refreshFilesExplorer" => self.command_explorer_refresh,
            "workbench.files.action.collapseExplorerFolders" => self.command_explorer_collapse,
            "workbench.action.closeActiveEditor" => self.command_close_editor,
            "workbench.action.closeOtherEditors" => self.command_close_others,
            "workbench.action.closeEditorsInGroup" => self.command_close_group,
            "workbench.action.closeAllEditors" => self.command_close_all,
            "workbench.action.reopenClosedEditor" => self.command_reopen_editor,
            "workbench.action.pinEditor" => self.command_pin_editor,
            "workbench.action.keepEditor" => self.command_keep_editor,
            "workbench.action.nextEditor" => self.command_next_editor,
            "workbench.action.previousEditor" => self.command_previous_editor,
            "workbench.action.splitEditor" => self.command_split_editor,
            "workbench.action.focusFirstEditorGroup" => self.command_focus_group_1,
            "workbench.action.focusSecondEditorGroup" => self.command_focus_group_2,
            "workbench.action.focusThirdEditorGroup" => self.command_focus_group_3,
            "workbench.action.quickOpen" => self.command_quick_open,
            "workbench.action.gotoLine" => self.command_go_to_line,
            "editor.action.commentLine" => self.command_comment_line,
            "editor.action.blockComment" => self.command_block_comment,
            "editor.action.moveLinesUpAction" => self.command_move_up,
            "editor.action.moveLinesDownAction" => self.command_move_down,
            "editor.action.copyLinesUpAction" => self.command_copy_up,
            "editor.action.copyLinesDownAction" => self.command_copy_down,
            "editor.action.deleteLines" => self.command_delete_lines,
            "editor.action.insertLineAfter" => self.command_line_after,
            "editor.action.insertLineBefore" => self.command_line_before,
            "editor.action.toggleWordWrap" => self.command_word_wrap,
            "workbench.action.zoomIn" => self.command_zoom_in,
            "workbench.action.zoomOut" => self.command_zoom_out,
            "workbench.action.zoomReset" => self.command_zoom_reset,
            _ => return None,
        })
    }

    /// "Do you want to save the changes you made to {name}?"
    pub fn save_changes(&self, name: &str) -> String {
        self.save_changes.replace("{name}", name)
    }

    pub fn could_not_save(&self, name: &str) -> String {
        self.could_not_save.replace("{name}", name)
    }

    pub fn could_not_use(&self, path: &str) -> String {
        self.could_not_use.replace("{path}", path)
    }

    pub fn could_not_start(&self, extension: &str, error: &str) -> String {
        self.could_not_start
            .replace("{extension}", extension)
            .replace("{error}", error)
    }

    pub fn could_not_open(&self, path: &str, error: &str) -> String {
        self.could_not_open
            .replace("{path}", path)
            .replace("{error}", error)
    }

    pub fn port_opened(&self, port: u16, process: &str) -> String {
        self.port_opened
            .replace("{port}", &port.to_string())
            .replace("{process}", process)
    }

    pub fn scm_commit_on(&self, branch: &str) -> String {
        if branch.is_empty() {
            return self.scm_commit.to_owned();
        }
        self.scm_commit_on.replace("{branch}", branch)
    }

    pub fn scm_discard_question(&self, name: &str) -> String {
        self.scm_discard_question.replace("{name}", name)
    }

    pub fn scm_discard_many_question(&self, count: usize) -> String {
        self.scm_discard_many_question
            .replace("{count}", &count.to_string())
    }

    pub fn terminal_failed(&self, error: &str) -> String {
        self.terminal_failed.replace("{error}", error)
    }

    pub fn could_not_change(&self, name: &str, error: &str) -> String {
        self.could_not_change
            .replace("{name}", name)
            .replace("{error}", error)
    }

    pub fn go_to_line_placeholder(&self, current: usize, total: usize) -> String {
        self.go_to_line
            .replace("{current}", &current.to_string())
            .replace("{total}", &total.to_string())
    }

    /// "7.310 risultati in 327 file", with the right singulars.
    pub fn results_summary(
        &self,
        count: &str,
        files: &str,
        one_result: bool,
        one_file: bool,
    ) -> String {
        let text = match (one_result, one_file) {
            (true, _) => self.results[0].to_owned(),
            (false, true) => self.results[1].replace(" files", " file"),
            (false, false) => self.results[1].to_owned(),
        };
        text.replace("{count}", count).replace("{files}", files)
    }

    pub fn replace_question(&self, count: &str, files: &str, text: &str) -> String {
        self.replace_question
            .replace("{count}", count)
            .replace("{files}", files)
            .replace("{text}", text)
    }

    pub fn replaced_summary(&self, count: &str, files: &str) -> String {
        self.replaced_summary
            .replace("{count}", count)
            .replace("{files}", files)
    }

    pub fn not_text(&self, name: &str) -> String {
        self.not_text.replace("{name}", name)
    }

    pub fn large_file_question(&self, name: &str, size: &str) -> String {
        self.large_file
            .replace("{name}", name)
            .replace("{size}", size)
    }

    pub fn changed_on_disk(&self, name: &str) -> String {
        self.changed_on_disk.replace("{name}", name)
    }

    pub fn overwrite_question(&self, name: &str) -> String {
        self.overwrite_question.replace("{name}", name)
    }

    pub fn move_question(&self, name: &str, folder: &str) -> String {
        self.move_question
            .replace("{name}", name)
            .replace("{folder}", folder)
    }

    pub fn trash_question(&self, name: &str) -> String {
        self.trash_question.replace("{name}", name)
    }

    pub fn lsp_references_title(&self, count: usize) -> String {
        self.lsp_references_text
            .replace("{count}", &count.to_string())
    }

    pub fn lsp_definitions_title(&self, count: usize) -> String {
        self.lsp_definitions_text
            .replace("{count}", &count.to_string())
    }

    pub fn lsp_renamed(&self, edits: usize, files: usize) -> String {
        self.lsp_renamed_text
            .replace("{edits}", &edits.to_string())
            .replace("{files}", &files.to_string())
    }

    pub fn pdf_page_of(&self, page: usize, count: usize) -> String {
        self.pdf_page_text
            .replace("{page}", &page.to_string())
            .replace("{count}", &count.to_string())
    }

    pub fn extension_updates_found(&self, count: usize) -> String {
        self.extension_updates_found_text
            .replace("{count}", &count.to_string())
    }

    pub fn extensions_restarted(&self, what: &str) -> String {
        self.extensions_restarted_text.replace("{what}", what)
    }

    pub fn cursor_install_all(&self, count: usize) -> String {
        self.cursor_install_all_text
            .replace("{count}", &count.to_string())
    }

    pub fn cursor_partial(&self, reasons: &str) -> String {
        self.cursor_partial_text.replace("{reasons}", reasons)
    }

    pub fn cursor_blocked(&self, reasons: &str) -> String {
        self.cursor_blocked_text.replace("{reasons}", reasons)
    }

    pub fn cursor_needs(&self, name: &str) -> String {
        self.cursor_needs_text.replace("{name}", name)
    }

    pub fn agent_stop_reason(&self, reason: &str) -> String {
        let reason = match reason {
            "cancelled" => self.agent_reason_cancelled,
            "max_tokens" | "max_turn_requests" => self.agent_reason_limit,
            "refusal" => self.agent_reason_refusal,
            other => other,
        };
        self.agent_stopped_text.replace("{reason}", reason)
    }

    pub fn agent_changes(&self, count: usize) -> String {
        self.agent_changes_text
            .replace("{count}", &count.to_string())
    }

    pub fn agent_plan(&self, done: usize, total: usize) -> String {
        self.agent_plan_text
            .replace("{done}", &done.to_string())
            .replace("{total}", &total.to_string())
    }

    pub fn agent_permission(&self, title: &str) -> String {
        self.agent_permission_text.replace("{title}", title)
    }

    pub fn extension_installed(&self, name: &str) -> String {
        self.extension_installed_text.replace("{name}", name)
    }

    pub fn extension_uninstall_question(&self, name: &str) -> String {
        self.extension_uninstall_question_text
            .replace("{name}", name)
    }

    pub fn extension_tab_title(&self, name: &str) -> String {
        self.extension_tab_title_text.replace("{name}", name)
    }

    pub fn file_history_title(&self, name: &str) -> String {
        self.file_history_title_text.replace("{name}", name)
    }

    pub fn graph_load_more(&self, count: usize) -> String {
        self.graph_load_more_text
            .replace("{count}", &count.to_string())
    }

    pub fn graph_checkout_branch(&self, name: &str) -> String {
        self.graph_checkout_branch_text.replace("{name}", name)
    }

    pub fn graph_merge_branch(&self, name: &str) -> String {
        self.graph_merge_branch_text.replace("{name}", name)
    }

    pub fn graph_delete_branch(&self, name: &str) -> String {
        self.graph_delete_branch_text.replace("{name}", name)
    }

    pub fn graph_delete_tag(&self, name: &str) -> String {
        self.graph_delete_tag_text.replace("{name}", name)
    }

    pub fn graph_checkout_question(&self, hash: &str) -> String {
        self.graph_checkout_question_text.replace("{hash}", hash)
    }

    pub fn graph_cherry_pick_question(&self, hash: &str) -> String {
        self.graph_cherry_pick_question_text.replace("{hash}", hash)
    }

    pub fn graph_revert_question(&self, hash: &str) -> String {
        self.graph_revert_question_text.replace("{hash}", hash)
    }

    pub fn graph_merge_question(&self, name: &str) -> String {
        self.graph_merge_question_text.replace("{name}", name)
    }

    pub fn graph_delete_branch_question(&self, name: &str) -> String {
        self.graph_delete_branch_question_text
            .replace("{name}", name)
    }

    pub fn graph_delete_tag_question(&self, name: &str) -> String {
        self.graph_delete_tag_question_text.replace("{name}", name)
    }

    pub fn preview_title(&self, name: &str) -> String {
        self.preview_title_text.replace("{name}", name)
    }

    pub fn lsp_installing(&self, name: &str) -> String {
        self.lsp_installing_text.replace("{name}", name)
    }

    pub fn prerequisite_missing(&self, server: &str, what: &str) -> String {
        self.prerequisite_missing_text
            .replace("{server}", server)
            .replace("{what}", what)
    }

    pub fn prerequisite_detail(&self, command: &str) -> String {
        self.prerequisite_detail_text.replace("{command}", command)
    }

    pub fn install_terminal_name(&self, what: &str) -> String {
        self.install_terminal_name_text.replace("{what}", what)
    }

    pub fn lsp_failed(&self, name: &str, error: &str) -> String {
        self.lsp_failed_text
            .replace("{name}", name)
            .replace("{error}", error)
    }

    pub fn lsp_stopped(&self, name: &str) -> String {
        self.lsp_stopped_text.replace("{name}", name)
    }

    /// "Coming in phase 3", for the parts of the window not built yet.
    #[allow(dead_code)]
    pub fn coming_in_phase(&self, phase: u8) -> String {
        self.coming_in_phase.replace("{phase}", &phase.to_string())
    }

    /// The cursor position in the status bar, counted from 1.
    pub fn line_column(&self, line: usize, column: usize) -> String {
        self.line_column
            .replace("{line}", &line.to_string())
            .replace("{column}", &column.to_string())
    }

    /// The indentation in the status bar.
    pub fn indentation(&self, size: usize, hard_tabs: bool) -> String {
        let text = if hard_tabs {
            self.tab_size
        } else {
            self.spaces
        };
        text.replace("{size}", &size.to_string())
    }
}

pub const ITALIAN: Strings = Strings {
    save: "Salva",
    dont_save: "Non salvare",
    cancel: "Annulla",
    ok: "OK",
    untitled: "Senza titolo",
    untitled_file_name: "Senza titolo.txt",
    changes_lost: "Se non le salvi, le modifiche andranno perse.",
    type_a_command: "Scrivi un comando",
    choose_language: "Lingua dell'interfaccia",
    command_save: "File: Salva",
    command_close_window: "Vista: Chiudi finestra",
    command_language: "Preferenze: Lingua",
    command_theme: "Preferenze: Tema colori",
    command_quit: "Esci da Keel",
    menu_file: "File",
    menu_edit: "Modifica",
    menu_view: "Vista",
    menu_services: "Servizi",
    menu_theme: "Tema colori…",
    menu_language: "Lingua…",
    menu_close_window: "Chiudi finestra",
    menu_undo: "Annulla",
    menu_redo: "Ripeti",
    menu_cut: "Taglia",
    menu_copy: "Copia",
    menu_paste: "Incolla",
    menu_select_all: "Seleziona tutto",
    menu_find: "Trova…",
    menu_replace: "Sostituisci…",
    menu_command_palette: "Palette dei comandi…",
    choose_theme: "Tema colori",
    settings_unavailable: "Le impostazioni non sono ancora disponibili",
    search_commands: "Cerca comandi",
    toggle_sidebar: "Mostra o nascondi la barra laterale",
    toggle_panel: "Mostra o nascondi il pannello",
    toggle_agent: "Mostra o nascondi Claude Code",
    view_explorer: "Esplora risorse",
    view_search: "Cerca",
    view_source_control: "Controllo del codice sorgente",
    view_extensions: "Estensioni",
    open_editors: "Editor aperti",
    installed_extensions: "Installate",
    no_extensions: "Nessuna estensione installata",
    panel_problems: "Problemi",
    panel_output: "Output",
    panel_terminal: "Terminale",
    terminal_failed: "Il terminale non è partito: {error}",
    no_terminal: "Nessun terminale aperto",
    panel_ports: "Porte",
    no_output: "Nessun messaggio",
    plain_text: "Testo normale",
    close_editor: "Chiudi",
    welcome_open: "Apri…",
    welcome_new_file: "Nuovo file",
    menu_new_window: "Nuova finestra",
    menu_open_recent: "Apri recente",
    recent_projects: "Recenti",
    open_recent_title: "Apri un progetto recente",
    no_recent_projects: "Nessun progetto recente",
    command_open: "File: Apri…",
    command_new_file: "File: Nuovo file",
    command_new_window: "File: Nuova finestra",
    command_open_recent: "File: Apri recente…",
    no_folder: "Apri una cartella per vederne i file.",
    explorer_new_file: "Nuovo file…",
    explorer_new_folder: "Nuova cartella…",
    explorer_refresh: "Aggiorna",
    explorer_collapse: "Comprimi tutto",
    explorer_rename: "Rinomina…",
    explorer_trash: "Sposta nel Cestino",
    explorer_reveal: "Mostra nel Finder",
    explorer_copy_path: "Copia percorso",
    explorer_copy_relative: "Copia percorso relativo",
    explorer_move: "Sposta",
    trash_detail: "Puoi recuperarlo dal Cestino.",
    command_explorer_new_file: "Esplora risorse: Nuovo file",
    command_explorer_new_folder: "Esplora risorse: Nuova cartella",
    command_explorer_refresh: "Esplora risorse: Aggiorna",
    command_explorer_collapse: "Esplora risorse: Comprimi tutto",
    tab_pin: "Fissa",
    tab_unpin: "Sblocca",
    tab_close_others: "Chiudi gli altri",
    tab_close_right: "Chiudi quelli a destra",
    tab_close_all: "Chiudi tutti",
    tab_split: "Dividi a destra",
    menu_close_editor: "Chiudi editor",
    menu_reopen_editor: "Riapri editor chiuso",
    menu_split_editor: "Dividi editor",
    command_close_editor: "Vista: Chiudi editor",
    command_close_others: "Vista: Chiudi gli altri editor del gruppo",
    command_close_group: "Vista: Chiudi tutti gli editor del gruppo",
    command_close_all: "Vista: Chiudi tutti gli editor",
    command_reopen_editor: "Vista: Riapri l'editor chiuso",
    command_pin_editor: "Vista: Fissa o sblocca l'editor",
    command_keep_editor: "Vista: Mantieni l'editor aperto",
    command_next_editor: "Vista: Editor successivo",
    command_previous_editor: "Vista: Editor precedente",
    command_split_editor: "Vista: Dividi l'editor",
    command_focus_group_1: "Vista: Vai al primo gruppo",
    command_focus_group_2: "Vista: Vai al secondo gruppo",
    command_focus_group_3: "Vista: Vai al terzo gruppo",
    search_files: "Cerca file",
    quick_open_placeholder: "Cerca un file per nome; aggiungi :riga per andarci",
    menu_go: "Vai",
    menu_go_to_file: "Vai al file…",
    menu_go_to_line: "Vai alla riga…",
    menu_rename_terminal: "Rinomina il terminale…",
    command_quick_open: "Vai al file…",
    command_go_to_line: "Vai alla riga o colonna…",
    command_comment_line: "Attiva/disattiva commento riga",
    command_block_comment: "Attiva/disattiva commento in blocco",
    command_move_up: "Sposta riga in alto",
    command_move_down: "Sposta riga in basso",
    command_copy_up: "Copia riga in alto",
    command_copy_down: "Copia riga in basso",
    command_delete_lines: "Elimina riga",
    command_line_after: "Inserisci riga sotto",
    command_line_before: "Inserisci riga sopra",
    command_word_wrap: "Vista: A capo automatico",
    command_zoom_in: "Vista: Ingrandisci",
    command_zoom_out: "Vista: Riduci",
    command_zoom_reset: "Vista: Dimensioni originali",
    menu_selection: "Selezione",
    menu_word_wrap: "A capo automatico",
    menu_zoom_in: "Ingrandisci",
    menu_zoom_out: "Riduci",
    menu_zoom_reset: "Dimensioni originali",
    overwrite: "Sovrascrivi",
    use_disk_version: "Usa la versione sul disco",
    large_file_detail: "Aprirlo può rallentare Keel per qualche momento.",
    open_anyway: "Apri comunque",
    search_placeholder: "Cerca",
    include_placeholder: "File da includere (es. src, *.ts)",
    exclude_placeholder: "File da escludere",
    match_case: "Maiuscole/minuscole",
    whole_word: "Parola intera",
    use_regex: "Espressione regolare",
    folders_toggle: "File da includere ed escludere",
    searching: "Ricerca…",
    no_results: "Nessun risultato",
    results_limited: " · mostrati i primi 20.000: restringi la ricerca",
    replace_placeholder: "Sostituisci",
    toggle_replace: "Mostra o nascondi Sostituisci",
    replace_all: "Sostituisci tutto",
    replace_in_file: "Sostituisci nel file",
    replace_detail: "I file aperti cambiano nell'editor e restano da salvare; gli altri vengono scritti subito.",
    overwrite_detail: "Se usi la versione sul disco, le modifiche fatte in Keel si perdono.",
    command_toggle_sidebar: "Vista: Mostra o nascondi la barra laterale",
    command_toggle_panel: "Vista: Mostra o nascondi il pannello",
    command_toggle_agent: "Vista: Mostra o nascondi Claude Code",
    command_explorer: "Vista: Esplora risorse",
    command_search: "Vista: Cerca",
    command_source_control: "Vista: Controllo del codice sorgente",
    command_extensions: "Vista: Estensioni",
    command_problems: "Vista: Problemi",
    command_output: "Vista: Output",
    command_terminal: "Vista: Terminale",
    terminal_name: "Nome del terminale",
    hunk_changed: "Blocco cambiato",
    hunk_preview: "Differenze",
    hunk_stage: "Metti in stage",
    hunk_revert: "Annulla",
    command_stage_change: "Git: Metti in stage il blocco",
    command_revert_change: "Git: Annulla il blocco",
    command_next_change: "Vai alla modifica successiva",
    command_previous_change: "Vai alla modifica precedente",
    diff_working_tree: "modifiche",
    diff_index: "in stage",
    diff_conflict: "conflitto",
    diff_side_by_side: "Affiancate",
    diff_inline: "In linea",
    diff_save_resolution: "Salva e metti in stage",
    scm_message: "Messaggio (⌘↵ per il commit)",
    scm_commit: "Commit",
    scm_commit_amend: "Commit (amend)",
    scm_stage: "Metti in stage",
    scm_unstage: "Togli dallo stage",
    scm_stage_all: "Metti tutto in stage",
    scm_unstage_all: "Togli tutto dallo stage",
    scm_discard: "Scarta le modifiche",
    scm_discard_all: "Scarta tutte le modifiche",
    scm_discard_detail: "Le modifiche non si possono recuperare; i file nuovi vanno nel Cestino.",
    scm_discard_button: "Scarta",
    scm_no_message: "Scrivi il messaggio del commit",
    scm_stage_and_commit_question: "Non c'è niente in stage. Metto in stage tutte le modifiche e faccio il commit?",
    scm_stage_and_commit: "Metti tutto in stage e fai il commit",
    scm_push: "Push",
    scm_pull: "Pull",
    scm_fetch: "Fetch",
    scm_sync: "Sincronizza",
    scm_publish: "Pubblica il branch",
    scm_stash: "Metti da parte le modifiche",
    scm_stash_pop: "Riprendi le modifiche messe da parte",
    scm_refresh: "Aggiorna",
    scm_checkout: "Cambia branch…",
    scm_create_branch: "Crea un branch…",
    scm_new_branch: "+ Crea un nuovo branch…",
    scm_remote_branch: "remoto",
    scm_choose_branch: "Scegli il branch",
    scm_branch_name: "Nome del nuovo branch",
    scm_no_repository: "Il progetto non è un repository git",
    scm_clean: "Nessuna modifica",
    scm_staged: "Modifiche in stage",
    scm_changes: "Modifiche",
    scm_conflicts: "Conflitti di merge",
    scm_open_file: "Apri il file",
    scm_commit_on: "Commit su «{branch}»",
    scm_discard_question: "Vuoi scartare le modifiche di «{name}»?",
    scm_discard_many_question: "Vuoi scartare le modifiche di {count} file?",
    command_git_commit: "Git: Commit",
    command_git_amend: "Git: Commit (amend)",
    command_git_stage_all: "Git: Metti tutto in stage",
    command_git_unstage_all: "Git: Togli tutto dallo stage",
    command_git_discard_all: "Git: Scarta tutte le modifiche",
    command_git_push: "Git: Push",
    command_git_pull: "Git: Pull",
    command_git_fetch: "Git: Fetch",
    command_git_sync: "Git: Sincronizza",
    command_git_stash: "Git: Metti da parte le modifiche (stash)",
    command_git_stash_pop: "Git: Riprendi le modifiche messe da parte",
    command_git_refresh: "Git: Aggiorna",
    command_git_checkout: "Git: Cambia branch…",
    command_git_branch: "Git: Crea un branch…",
    output_extensions: "Estensioni",
    output_clear: "Pulisci",
    no_ports: "Nessuna porta aperta dai programmi dei terminali",
    port: "Porta",
    port_address: "Indirizzo",
    port_process: "Processo",
    open_in_browser: "Apri nel browser",
    port_opened: "La porta {port} è aperta ({process}): la trovi in Porte",
    npm_scripts: "Script npm",
    no_npm_scripts: "Il progetto non ha script in package.json",
    run_npm_script: "Scegli lo script da eseguire",
    command_run_npm_script: "npm: Esegui uno script…",
    find_in_terminal: "Cerca nel terminale",
    find_older: "Più indietro (Invio)",
    find_newer: "Più avanti (Maiusc+Invio)",
    close_find: "Chiudi (Esc)",
    terminal_new: "Nuovo terminale",
    terminal_split: "Dividi il terminale",
    terminal_rename: "Rinomina…",
    terminal_kill: "Chiudi il terminale",
    maximize_panel: "Ingrandisci il pannello",
    restore_panel: "Riduci il pannello",
    open_claude_code: "Apri Claude Code",
    claude_code_hint: "Claude Code gira in un terminale qui accanto (⌘Esc)",
    close_claude_code: "Chiudi Claude Code",
    command_new_terminal: "Terminale: Nuovo terminale",
    command_split_terminal: "Terminale: Dividi il terminale",
    command_kill_terminal: "Terminale: Chiudi il terminale",
    command_rename_terminal: "Terminale: Rinomina…",
    command_next_terminal: "Terminale: Scheda successiva",
    command_previous_terminal: "Terminale: Scheda precedente",
    command_maximize_panel: "Vista: Ingrandisci il pannello",
    command_claude_code: "Claude Code: Apri a lato",
    command_claude_code_panel: "Claude Code: Apri nel pannello in basso",
    save_changes: "Vuoi salvare le modifiche a {name}?",
    could_not_save: "Keel non è riuscito a salvare {name}",
    could_not_use: "Keel non può usare {path}",
    could_not_start: "{extension} non è partita: {error}",
    could_not_open: "Keel non riesce ad aprire {path}: {error}",
    could_not_change: "Operazione non riuscita su {name}: {error}",
    go_to_line: "Riga attuale {current} di {total}. Scrivi la riga, anche con :colonna",
    changed_on_disk: "«{name}» è cambiato sul disco; quando salvi, Keel ti chiede quale versione tenere",
    not_text: "«{name}» non è un file di testo: Keel non lo apre",
    replace_question: "Sostituire {count} occorrenze in {files} file con «{text}»?",
    replaced_summary: "Sostituite {count} occorrenze in {files} file",
    results: [
        "{count} risultato in {files} file",
        "{count} risultati in {files} file",
        "{count} risultato",
        "{count} risultati",
    ],
    large_file: "«{name}» è grande ({size}). Vuoi aprirlo?",
    overwrite_question: "«{name}» è cambiato sul disco dopo che Keel l'ha letto. Vuoi sovrascriverlo con le tue modifiche?",
    move_question: "Vuoi spostare «{name}» in «{folder}»?",
    trash_question: "Vuoi spostare «{name}» nel Cestino?",
    coming_in_phase: "Arriva con la fase {phase}",
    line_column: "Riga {line}, col {column}",
    spaces: "Spazi: {size}",
    tab_size: "Tabulazione: {size}",
    output_languages: "Linguaggi",
    lsp_started: "avviato",
    lsp_installing_text: "Installo il server {name}…",
    lsp_failed_text: "Il server {name} non è partito: {error}",
    prerequisite_missing_text: "Per il server {server} serve {what}, che non è installato.",
    prerequisite_detail_text: "Keel può aprire un terminale con il comando:\n{command}\n\nQuando ha finito, usa «Linguaggi: Riavvia i language server» dalla tavolozza.",
    install_in_terminal: "Installa nel terminale",
    install_terminal_name_text: "Installa {what}",
    lsp_stopped_text: "Il server {name} si è fermato più volte. Per riprovare: «Riavvia i language server» dalla tavolozza.",
    lsp_restarting: "si è fermato: lo riavvio",
    lsp_restarted: "Language server riavviati",
    command_restart_language_servers: "Linguaggi: Riavvia i language server",
    problems_none: "Nessun problema rilevato finora.",
    command_next_problem: "Vai al problema successivo",
    command_previous_problem: "Vai al problema precedente",
    command_trigger_parameter_hints: "Mostra i parametri",
    command_trigger_suggest: "Mostra i suggerimenti",
    lsp_not_supported: "Nessun language server sa farlo per questo file.",
    lsp_no_definition: "Nessuna definizione trovata",
    lsp_no_references: "Nessun riferimento trovato",
    lsp_references_text: "{count} riferimenti",
    lsp_definitions_text: "{count} definizioni",
    lsp_cannot_rename: "Qui non c'è niente da rinominare.",
    lsp_rename_title: "Rinomina il simbolo",
    lsp_renamed_text: "{edits} modifiche in {files} file",
    lsp_symbols_in_file: "Vai a un simbolo del file",
    lsp_symbols_in_workspace: "Vai a un simbolo del progetto",
    command_reveal_definition: "Vai alla definizione",
    command_go_to_implementation: "Vai alle implementazioni",
    command_go_to_type_definition: "Vai alla definizione del tipo",
    command_go_to_references: "Vai ai riferimenti",
    command_rename_symbol: "Rinomina il simbolo",
    command_symbol_in_file: "Vai al simbolo nel file",
    command_symbol_in_workspace: "Vai al simbolo nel progetto",
    format_timed_out: "Le correzioni e la formattazione al salvataggio non hanno finito in 3 secondi: il file è salvato così.",
    command_format_document: "Formatta il documento",
    command_organize_imports: "Organizza gli import",
    preview_title_text: "Anteprima {name}",
    command_markdown_preview: "Markdown: Apri l'anteprima",
    command_markdown_preview_side: "Markdown: Apri l'anteprima a lato",
    image_fit: "Adatta",
    pdf_search_placeholder: "Cerca nel PDF",
    pdf_thumbnails: "Miniature",
    pdf_page_text: "Pagina {page} di {count}",
    find_previous: "Risultato precedente (Maiusc+Invio)",
    find_next: "Risultato successivo (Invio)",
    command_git_graph: "Git Graph: Mostra il grafico",
    graph_no_repository: "Questa cartella non è un repository git.",
    graph_search_placeholder: "Cerca: messaggio, hash, autore, branch",
    graph_uncommitted: "Modifiche non committate",
    graph_all_branches: "Tutti i branch",
    graph_branches: "Branch",
    graph_column_graph: "Grafico",
    graph_column_description: "Descrizione",
    graph_column_date: "Data",
    graph_column_author: "Autore",
    graph_column_commit: "Commit",
    graph_load_more_text: "Carica altri {count} commit",
    graph_parents: "Genitori",
    graph_loading: "Caricamento…",
    graph_checkout: "Checkout",
    graph_create_branch: "Crea un branch qui…",
    graph_create_tag: "Aggiungi un tag…",
    graph_cherry_pick: "Cherry-pick",
    graph_revert: "Revert",
    graph_merge: "Merge",
    graph_delete: "Elimina",
    graph_copy_hash: "Copia l'hash",
    graph_copy_subject: "Copia il messaggio",
    graph_tag_name: "Nome del tag",
    graph_checkout_branch_text: "Checkout di «{name}»",
    graph_merge_branch_text: "Merge di «{name}» nel branch corrente…",
    graph_delete_branch_text: "Elimina il branch «{name}»…",
    graph_delete_tag_text: "Elimina il tag «{name}»…",
    graph_checkout_question_text: "Vuoi fare il checkout del commit {hash}? Non sarai su nessun branch (HEAD staccato).",
    graph_cherry_pick_question_text: "Vuoi applicare il commit {hash} al branch corrente (cherry-pick)?",
    graph_revert_question_text: "Vuoi annullare il commit {hash} con un nuovo commit (revert)?",
    graph_merge_question_text: "Vuoi fare il merge di «{name}» nel branch corrente?",
    graph_delete_branch_question_text: "Vuoi eliminare il branch «{name}»?",
    graph_delete_tag_question_text: "Vuoi eliminare il tag «{name}»?",
    command_toggle_blame: "Git: Mostra o nascondi l'autore delle righe",
    command_file_history: "Git: Storico del file",
    blame_not_in_git: "Questo file non è in un repository git.",
    blame_uncommitted: "Non ancora committato",
    file_history_title_text: "Storico di {name}",
    extensions_install_vsix: "Estensioni: Installa da VSIX…",
    extensions_restart: "Riavvia le estensioni",
    extensions_failed: "L'host delle estensioni non è ripartito.",
    extension_installed_text: "{name} installata",
    extension_uninstall_question_text: "Vuoi disinstallare «{name}»?",
    extension_tab_title_text: "Estensione: {name}",
    extension_enable: "Attiva",
    extension_disable: "Disattiva",
    extension_disable_project: "Disattiva per questo progetto",
    extension_uninstall: "Disinstalla",
    extension_disabled_badge: "Disattivata",
    extension_missing: "Questa estensione non è più installata.",
    extension_commands: "comandi",
    extension_keybindings: "scorciatoie",
    extension_themes: "temi",
    extension_icon_themes: "temi di icone",
    extension_snippets: "file di snippet",
    extension_languages: "linguaggi",
    extensions_search_placeholder: "Cerca estensioni in Open VSX",
    extensions_searching: "Ricerca in corso…",
    extensions_none_found: "Nessuna estensione trovata.",
    extensions_check_updates: "Controlla aggiornamenti estensioni",
    extensions_update_all: "Aggiorna tutte le estensioni",
    extensions_up_to_date: "Le estensioni sono aggiornate.",
    extension_updates_found_text: "Aggiornamenti disponibili: {count}",
    extension_install: "Installa",
    extension_update: "Aggiorna",
    extension_installing: "Installazione…",
    extension_installed_badge: "Installata",
    command_icon_theme: "Preferenze: Tema icona file",
    choose_icon_theme: "Scegli il tema delle icone dei file",
    icon_theme_none: "Icone di Keel",
    extensions_restarted_text: "Un'estensione non rispondeva ({what}): estensioni riavviate.",
    extensions_gave_up: "Le estensioni si sono bloccate troppe volte e restano spente. Riavviale dal menu «…» della vista Estensioni.",
    extensions_import_cursor: "Importa da Cursor…",
    command_import_cursor: "Estensioni: Importa da Cursor",
    cursor_heading: "Da Cursor",
    cursor_close: "Chiudi",
    cursor_loading: "Lettura delle estensioni di Cursor…",
    cursor_none: "Cursor non ha estensioni da importare.",
    cursor_install_all_text: "Installa le disponibili ({count})",
    cursor_group_install: "Da installare",
    cursor_group_keel: "Già in Keel",
    cursor_group_blocked: "Non funzionano in Keel",
    cursor_installed: "Già installata",
    cursor_local: "Copiata dalla cartella di Cursor",
    cursor_partial_text: "Senza: {reasons}",
    cursor_blocked_text: "Non funziona: {reasons}",
    cursor_needs_text: "Serve {name}, che non funziona in Keel",
    cursor_not_on_open_vsx: "Non è su Open VSX",
    cursor_builtin_prettier: "Keel usa già il Prettier del progetto",
    cursor_builtin_git_graph: "Git Graph e gli autori delle righe sono già in Keel",
    cursor_builtin_claude_code: "Claude Code è già nel pannello a destra",
    cursor_builtin_pdf: "Keel apre già i PDF",
    cursor_builtin_markdown_preview: "Keel ha già l'anteprima Markdown",
    cursor_builtin_yaml: "Keel ha già il language server YAML",
    cursor_builtin_typescript: "Keel usa già TypeScript con vtsls",
    cursor_blocker_cursor_only: "fatta solo per Cursor",
    cursor_blocker_debugger: "serve il debugger",
    cursor_blocker_notebooks: "servono i notebook",
    cursor_blocker_custom_editors: "editor propri",
    cursor_blocker_views: "pannelli propri",
    cursor_blocker_webviews: "pagine web interne",
    cursor_blocker_language_server: "avvia un language server",
    command_insert_at_mention: "Claude Code: Inserisci il riferimento al file",
    claude_not_connected: "Claude Code non è collegato a questa finestra: avvialo in un terminale di Keel.",
    command_accept_proposed: "Claude Code: Accetta la modifica proposta",
    command_reject_proposed: "Claude Code: Rifiuta la modifica proposta",
    claude_proposed_edit: "Proposta di Claude Code",
    claude_accept: "Accetta",
    claude_reject: "Rifiuta",
    agent_view_terminal: "Terminale",
    agent_view_agent: "Agente",
    agent_placeholder: "Chiedi a Claude… (Invio invia, Maiusc+Invio va a capo)",
    agent_no_data: "Keel non sa dove installare l'adattatore di Claude.",
    agent_stopped: "Claude si è fermato. Premi Avvia per ripartire; i dettagli sono nell'Output.",
    agent_stopped_text: "Turno finito: {reason}.",
    agent_reason_cancelled: "interrotto",
    agent_reason_limit: "raggiunto il limite",
    agent_reason_refusal: "Claude ha rifiutato",
    agent_history: "Conversazioni passate",
    agent_new: "Nuova conversazione",
    agent_installing: "Installazione dell'adattatore di Claude, solo la prima volta (circa 2 minuti)…",
    agent_starting: "Avvio di Claude…",
    agent_empty: "Scrivi a Claude qui sotto. Con @ aggiungi il file aperto o la selezione.",
    agent_start: "Avvia",
    agent_working: "Claude sta lavorando…",
    agent_thinking: "Ragionamento",
    agent_plan_text: "Piano: {done} di {total}",
    agent_permission_text: "Claude chiede il permesso: {title}",
    agent_mode: "Modalità",
    agent_stop: "Interrompi",
    agent_send: "Invia",
    agent_add_file: "Aggiungi il file aperto o la selezione",
    command_open_agent: "Claude Code: Apri l'agente",
    command_new_agent: "Claude Code: Nuova conversazione",
    command_add_to_agent: "Claude Code: Aggiungi il file o la selezione alla conversazione",
    agent_changes_text: "Modifiche di Claude: {count} file",
    agent_accept_all: "Accetta tutte",
    agent_reject_all: "Rifiuta tutte",
    agent_new_file: "nuovo",
    agent_change_of_claude: "Modifica di Claude",
};

pub const ENGLISH: Strings = Strings {
    save: "Save",
    dont_save: "Don't Save",
    cancel: "Cancel",
    ok: "OK",
    untitled: "Untitled",
    untitled_file_name: "Untitled.txt",
    changes_lost: "Your changes will be lost if you don't save them.",
    type_a_command: "Type a command",
    choose_language: "Interface language",
    command_save: "File: Save",
    command_close_window: "View: Close Window",
    command_language: "Preferences: Language",
    command_theme: "Preferences: Color Theme",
    command_quit: "Quit Keel",
    menu_file: "File",
    menu_edit: "Edit",
    menu_view: "View",
    menu_services: "Services",
    menu_theme: "Color Theme…",
    menu_language: "Language…",
    menu_close_window: "Close Window",
    menu_undo: "Undo",
    menu_redo: "Redo",
    menu_cut: "Cut",
    menu_copy: "Copy",
    menu_paste: "Paste",
    menu_select_all: "Select All",
    menu_find: "Find…",
    menu_replace: "Replace…",
    menu_command_palette: "Command Palette…",
    choose_theme: "Color theme",
    settings_unavailable: "Settings are not available yet",
    search_commands: "Search commands",
    toggle_sidebar: "Show or hide the sidebar",
    toggle_panel: "Show or hide the panel",
    toggle_agent: "Show or hide Claude Code",
    view_explorer: "Explorer",
    view_search: "Search",
    view_source_control: "Source Control",
    view_extensions: "Extensions",
    open_editors: "Open Editors",
    installed_extensions: "Installed",
    no_extensions: "No extensions installed",
    panel_problems: "Problems",
    panel_output: "Output",
    panel_terminal: "Terminal",
    terminal_failed: "The terminal did not start: {error}",
    no_terminal: "No terminal open",
    panel_ports: "Ports",
    no_output: "No messages",
    plain_text: "Plain Text",
    close_editor: "Close",
    welcome_open: "Open…",
    welcome_new_file: "New File",
    menu_new_window: "New Window",
    menu_open_recent: "Open Recent",
    recent_projects: "Recent",
    open_recent_title: "Open a recent project",
    no_recent_projects: "No recent projects",
    command_open: "File: Open…",
    command_new_file: "File: New File",
    command_new_window: "File: New Window",
    command_open_recent: "File: Open Recent…",
    no_folder: "Open a folder to see its files.",
    explorer_new_file: "New File…",
    explorer_new_folder: "New Folder…",
    explorer_refresh: "Refresh",
    explorer_collapse: "Collapse All",
    explorer_rename: "Rename…",
    explorer_trash: "Move to Trash",
    explorer_reveal: "Reveal in Finder",
    explorer_copy_path: "Copy Path",
    explorer_copy_relative: "Copy Relative Path",
    explorer_move: "Move",
    trash_detail: "You can restore it from the Trash.",
    command_explorer_new_file: "Explorer: New File",
    command_explorer_new_folder: "Explorer: New Folder",
    command_explorer_refresh: "Explorer: Refresh",
    command_explorer_collapse: "Explorer: Collapse All",
    tab_pin: "Pin",
    tab_unpin: "Unpin",
    tab_close_others: "Close Others",
    tab_close_right: "Close to the Right",
    tab_close_all: "Close All",
    tab_split: "Split Right",
    menu_close_editor: "Close Editor",
    menu_reopen_editor: "Reopen Closed Editor",
    menu_split_editor: "Split Editor",
    command_close_editor: "View: Close Editor",
    command_close_others: "View: Close Other Editors in Group",
    command_close_group: "View: Close All Editors in Group",
    command_close_all: "View: Close All Editors",
    command_reopen_editor: "View: Reopen Closed Editor",
    command_pin_editor: "View: Pin or Unpin Editor",
    command_keep_editor: "View: Keep Editor",
    command_next_editor: "View: Open Next Editor",
    command_previous_editor: "View: Open Previous Editor",
    command_split_editor: "View: Split Editor",
    command_focus_group_1: "View: Focus First Editor Group",
    command_focus_group_2: "View: Focus Second Editor Group",
    command_focus_group_3: "View: Focus Third Editor Group",
    search_files: "Search files",
    quick_open_placeholder: "Search files by name; add :line to go there",
    menu_go: "Go",
    menu_go_to_file: "Go to File…",
    menu_go_to_line: "Go to Line…",
    menu_rename_terminal: "Rename Terminal…",
    command_quick_open: "Go to File…",
    command_go_to_line: "Go to Line/Column…",
    command_comment_line: "Toggle Line Comment",
    command_block_comment: "Toggle Block Comment",
    command_move_up: "Move Line Up",
    command_move_down: "Move Line Down",
    command_copy_up: "Copy Line Up",
    command_copy_down: "Copy Line Down",
    command_delete_lines: "Delete Line",
    command_line_after: "Insert Line Below",
    command_line_before: "Insert Line Above",
    command_word_wrap: "View: Toggle Word Wrap",
    command_zoom_in: "View: Zoom In",
    command_zoom_out: "View: Zoom Out",
    command_zoom_reset: "View: Reset Zoom",
    menu_selection: "Selection",
    menu_word_wrap: "Word Wrap",
    menu_zoom_in: "Zoom In",
    menu_zoom_out: "Zoom Out",
    menu_zoom_reset: "Reset Zoom",
    overwrite: "Overwrite",
    use_disk_version: "Use the Version on Disk",
    large_file_detail: "Opening it may slow Keel down for a moment.",
    open_anyway: "Open Anyway",
    search_placeholder: "Search",
    include_placeholder: "Files to include (e.g. src, *.ts)",
    exclude_placeholder: "Files to exclude",
    match_case: "Match Case",
    whole_word: "Match Whole Word",
    use_regex: "Use Regular Expression",
    folders_toggle: "Files to include and exclude",
    searching: "Searching…",
    no_results: "No results",
    results_limited: " · showing the first 20,000: narrow the search",
    replace_placeholder: "Replace",
    toggle_replace: "Toggle Replace",
    replace_all: "Replace All",
    replace_in_file: "Replace in File",
    replace_detail: "Open files change in their editor and wait to be saved; the others are written at once.",
    overwrite_detail: "If you use the version on disk, the changes made in Keel are lost.",
    command_toggle_sidebar: "View: Toggle Sidebar",
    command_toggle_panel: "View: Toggle Panel",
    command_toggle_agent: "View: Toggle Claude Code",
    command_explorer: "View: Show Explorer",
    command_search: "View: Show Search",
    command_source_control: "View: Show Source Control",
    command_extensions: "View: Show Extensions",
    command_problems: "View: Toggle Problems",
    command_output: "View: Toggle Output",
    command_terminal: "View: Toggle Terminal",
    terminal_name: "Terminal name",
    hunk_changed: "Changed block",
    hunk_preview: "Diff",
    hunk_stage: "Stage",
    hunk_revert: "Revert",
    command_stage_change: "Git: Stage Selected Ranges",
    command_revert_change: "Git: Revert Selected Ranges",
    command_next_change: "Go to Next Change",
    command_previous_change: "Go to Previous Change",
    diff_working_tree: "Working Tree",
    diff_index: "Index",
    diff_conflict: "Merge Conflict",
    diff_side_by_side: "Side by Side",
    diff_inline: "Inline",
    diff_save_resolution: "Save and Stage",
    scm_message: "Message (⌘↵ to commit)",
    scm_commit: "Commit",
    scm_commit_amend: "Commit (Amend)",
    scm_stage: "Stage Changes",
    scm_unstage: "Unstage Changes",
    scm_stage_all: "Stage All Changes",
    scm_unstage_all: "Unstage All Changes",
    scm_discard: "Discard Changes",
    scm_discard_all: "Discard All Changes",
    scm_discard_detail: "The changes cannot be recovered; new files go to the Trash.",
    scm_discard_button: "Discard",
    scm_no_message: "Write a commit message",
    scm_stage_and_commit_question: "There are no staged changes. Stage all your changes and commit them?",
    scm_stage_and_commit: "Stage All and Commit",
    scm_push: "Push",
    scm_pull: "Pull",
    scm_fetch: "Fetch",
    scm_sync: "Sync",
    scm_publish: "Publish Branch",
    scm_stash: "Stash Changes",
    scm_stash_pop: "Pop Stash",
    scm_refresh: "Refresh",
    scm_checkout: "Checkout to…",
    scm_create_branch: "Create Branch…",
    scm_new_branch: "+ Create New Branch…",
    scm_remote_branch: "remote",
    scm_choose_branch: "Select a branch to checkout",
    scm_branch_name: "New branch name",
    scm_no_repository: "The project is not a git repository",
    scm_clean: "No changes",
    scm_staged: "Staged Changes",
    scm_changes: "Changes",
    scm_conflicts: "Merge Changes",
    scm_open_file: "Open File",
    scm_commit_on: "Commit to {branch}",
    scm_discard_question: "Discard the changes in “{name}”?",
    scm_discard_many_question: "Discard the changes in {count} files?",
    command_git_commit: "Git: Commit",
    command_git_amend: "Git: Commit Staged (Amend)",
    command_git_stage_all: "Git: Stage All Changes",
    command_git_unstage_all: "Git: Unstage All Changes",
    command_git_discard_all: "Git: Discard All Changes",
    command_git_push: "Git: Push",
    command_git_pull: "Git: Pull",
    command_git_fetch: "Git: Fetch",
    command_git_sync: "Git: Sync",
    command_git_stash: "Git: Stash",
    command_git_stash_pop: "Git: Pop Stash",
    command_git_refresh: "Git: Refresh",
    command_git_checkout: "Git: Checkout to…",
    command_git_branch: "Git: Create Branch…",
    output_extensions: "Extensions",
    output_clear: "Clear",
    no_ports: "No ports opened by the terminals' programs",
    port: "Port",
    port_address: "Address",
    port_process: "Process",
    open_in_browser: "Open in Browser",
    port_opened: "Port {port} is open ({process}): see Ports",
    npm_scripts: "NPM Scripts",
    no_npm_scripts: "The project has no scripts in package.json",
    run_npm_script: "Select the script to run",
    command_run_npm_script: "npm: Run Script…",
    find_in_terminal: "Find in terminal",
    find_older: "Previous Match (Enter)",
    find_newer: "Next Match (Shift+Enter)",
    close_find: "Close (Escape)",
    terminal_new: "New Terminal",
    terminal_split: "Split Terminal",
    terminal_rename: "Rename…",
    terminal_kill: "Kill Terminal",
    maximize_panel: "Maximize Panel",
    restore_panel: "Restore Panel",
    open_claude_code: "Open Claude Code",
    claude_code_hint: "Claude Code runs in a terminal here (⌘Esc)",
    close_claude_code: "Close Claude Code",
    command_new_terminal: "Terminal: Create New Terminal",
    command_split_terminal: "Terminal: Split Terminal",
    command_kill_terminal: "Terminal: Kill the Active Terminal Instance",
    command_rename_terminal: "Terminal: Rename…",
    command_next_terminal: "Terminal: Focus Next Terminal Group",
    command_previous_terminal: "Terminal: Focus Previous Terminal Group",
    command_maximize_panel: "View: Toggle Maximized Panel",
    command_claude_code: "Claude Code: Open on the Side",
    command_claude_code_panel: "Claude Code: Open in the Bottom Panel",
    save_changes: "Do you want to save the changes you made to {name}?",
    could_not_save: "Keel could not save {name}",
    could_not_use: "Keel could not use {path}",
    could_not_start: "{extension} could not start: {error}",
    could_not_open: "Keel could not open {path}: {error}",
    could_not_change: "Could not change {name}: {error}",
    go_to_line: "Current line {current} of {total}. Type a line, with :column if you like",
    changed_on_disk: "\"{name}\" changed on disk; when you save, Keel asks which version to keep",
    not_text: "\"{name}\" is not a text file, so Keel does not open it",
    replace_question: "Replace {count} matches in {files} files with \"{text}\"?",
    replaced_summary: "Replaced {count} matches in {files} files",
    results: [
        "{count} result in {files} file",
        "{count} results in {files} files",
        "{count} result",
        "{count} results",
    ],
    large_file: "\"{name}\" is large ({size}). Do you want to open it?",
    overwrite_question: "\"{name}\" changed on disk after Keel read it. Do you want to overwrite it with your changes?",
    move_question: "Do you want to move \"{name}\" into \"{folder}\"?",
    trash_question: "Do you want to move \"{name}\" to the Trash?",
    coming_in_phase: "Coming in phase {phase}",
    line_column: "Ln {line}, Col {column}",
    spaces: "Spaces: {size}",
    tab_size: "Tab Size: {size}",
    output_languages: "Languages",
    lsp_started: "started",
    lsp_installing_text: "Installing the {name} server…",
    lsp_failed_text: "The {name} server did not start: {error}",
    prerequisite_missing_text: "The {server} server needs {what}, which is not installed.",
    prerequisite_detail_text: "Keel can open a terminal with the command:\n{command}\n\nWhen it is done, run “Languages: Restart Language Servers” from the palette.",
    install_in_terminal: "Install in Terminal",
    install_terminal_name_text: "Install {what}",
    lsp_stopped_text: "The {name} server stopped several times. To try again: \"Restart Language Servers\" from the Command Palette.",
    lsp_restarting: "stopped: starting it again",
    lsp_restarted: "Language servers restarted",
    command_restart_language_servers: "Languages: Restart Language Servers",
    problems_none: "No problems have been detected so far.",
    command_next_problem: "Go to Next Problem",
    command_previous_problem: "Go to Previous Problem",
    command_trigger_parameter_hints: "Trigger Parameter Hints",
    command_trigger_suggest: "Trigger Suggest",
    lsp_not_supported: "No language server can do this for this file.",
    lsp_no_definition: "No definition found",
    lsp_no_references: "No references found",
    lsp_references_text: "{count} references",
    lsp_definitions_text: "{count} definitions",
    lsp_cannot_rename: "Nothing to rename here.",
    lsp_rename_title: "Rename Symbol",
    lsp_renamed_text: "{edits} edits in {files} files",
    lsp_symbols_in_file: "Go to a symbol in the file",
    lsp_symbols_in_workspace: "Go to a symbol in the project",
    command_reveal_definition: "Go to Definition",
    command_go_to_implementation: "Go to Implementations",
    command_go_to_type_definition: "Go to Type Definition",
    command_go_to_references: "Go to References",
    command_rename_symbol: "Rename Symbol",
    command_symbol_in_file: "Go to Symbol in Editor",
    command_symbol_in_workspace: "Go to Symbol in Workspace",
    format_timed_out: "Fixes and formatting on save did not finish within 3 seconds: the file was saved as it was.",
    command_format_document: "Format Document",
    command_organize_imports: "Organize Imports",
    preview_title_text: "Preview {name}",
    command_markdown_preview: "Markdown: Open Preview",
    command_markdown_preview_side: "Markdown: Open Preview to the Side",
    image_fit: "Fit",
    pdf_search_placeholder: "Find in PDF",
    pdf_thumbnails: "Thumbnails",
    pdf_page_text: "Page {page} of {count}",
    find_previous: "Previous Match (Shift+Enter)",
    find_next: "Next Match (Enter)",
    command_git_graph: "Git Graph: View Git Graph",
    graph_no_repository: "This folder is not a git repository.",
    graph_search_placeholder: "Find: message, hash, author, branch",
    graph_uncommitted: "Uncommitted Changes",
    graph_all_branches: "Show All",
    graph_branches: "Branches",
    graph_column_graph: "Graph",
    graph_column_description: "Description",
    graph_column_date: "Date",
    graph_column_author: "Author",
    graph_column_commit: "Commit",
    graph_load_more_text: "Load {count} more commits",
    graph_parents: "Parents",
    graph_loading: "Loading…",
    graph_checkout: "Checkout",
    graph_create_branch: "Create Branch…",
    graph_create_tag: "Add Tag…",
    graph_cherry_pick: "Cherry Pick",
    graph_revert: "Revert",
    graph_merge: "Merge",
    graph_delete: "Delete",
    graph_copy_hash: "Copy Commit Hash",
    graph_copy_subject: "Copy Commit Subject",
    graph_tag_name: "Tag name",
    graph_checkout_branch_text: "Checkout \"{name}\"",
    graph_merge_branch_text: "Merge \"{name}\" into Current Branch…",
    graph_delete_branch_text: "Delete Branch \"{name}\"…",
    graph_delete_tag_text: "Delete Tag \"{name}\"…",
    graph_checkout_question_text: "Do you want to check out commit {hash}? You will be on no branch (detached HEAD).",
    graph_cherry_pick_question_text: "Do you want to apply commit {hash} to the current branch (cherry-pick)?",
    graph_revert_question_text: "Do you want to undo commit {hash} with a new commit (revert)?",
    graph_merge_question_text: "Do you want to merge \"{name}\" into the current branch?",
    graph_delete_branch_question_text: "Do you want to delete branch \"{name}\"?",
    graph_delete_tag_question_text: "Do you want to delete tag \"{name}\"?",
    command_toggle_blame: "Git: Toggle File Blame",
    command_file_history: "Git: File History",
    blame_not_in_git: "This file is not in a git repository.",
    blame_uncommitted: "Not committed yet",
    file_history_title_text: "History of {name}",
    extensions_install_vsix: "Extensions: Install from VSIX…",
    extensions_restart: "Restart Extension Host",
    extensions_failed: "The extension host did not start again.",
    extension_installed_text: "{name} installed",
    extension_uninstall_question_text: "Do you want to uninstall \"{name}\"?",
    extension_tab_title_text: "Extension: {name}",
    extension_enable: "Enable",
    extension_disable: "Disable",
    extension_disable_project: "Disable (Workspace)",
    extension_uninstall: "Uninstall",
    extension_disabled_badge: "Disabled",
    extension_missing: "This extension is no longer installed.",
    extension_commands: "commands",
    extension_keybindings: "keybindings",
    extension_themes: "themes",
    extension_icon_themes: "icon themes",
    extension_snippets: "snippet files",
    extension_languages: "languages",
    extensions_search_placeholder: "Search Extensions in Open VSX",
    extensions_searching: "Searching…",
    extensions_none_found: "No extensions found.",
    extensions_check_updates: "Check for Extension Updates",
    extensions_update_all: "Update All Extensions",
    extensions_up_to_date: "All extensions are up to date.",
    extension_updates_found_text: "Updates available: {count}",
    extension_install: "Install",
    extension_update: "Update",
    extension_installing: "Installing…",
    extension_installed_badge: "Installed",
    command_icon_theme: "Preferences: File Icon Theme",
    choose_icon_theme: "Select File Icon Theme",
    icon_theme_none: "Keel's icons",
    extensions_restarted_text: "An extension stopped responding ({what}): extensions restarted.",
    extensions_gave_up: "Extensions stopped responding too many times and stay off. Restart them from the … menu of the Extensions view.",
    extensions_import_cursor: "Import from Cursor…",
    command_import_cursor: "Extensions: Import from Cursor",
    cursor_heading: "From Cursor",
    cursor_close: "Close",
    cursor_loading: "Reading Cursor's extensions…",
    cursor_none: "Cursor has no extensions to import.",
    cursor_install_all_text: "Install available ({count})",
    cursor_group_install: "To install",
    cursor_group_keel: "Already in Keel",
    cursor_group_blocked: "Not working in Keel",
    cursor_installed: "Already installed",
    cursor_local: "Copied from Cursor's folder",
    cursor_partial_text: "Without: {reasons}",
    cursor_blocked_text: "Does not work: {reasons}",
    cursor_needs_text: "Needs {name}, which does not work in Keel",
    cursor_not_on_open_vsx: "Not on Open VSX",
    cursor_builtin_prettier: "Keel already uses the project's Prettier",
    cursor_builtin_git_graph: "Git Graph and line authors are built into Keel",
    cursor_builtin_claude_code: "Claude Code is already in the right panel",
    cursor_builtin_pdf: "Keel already opens PDFs",
    cursor_builtin_markdown_preview: "Keel already has the Markdown preview",
    cursor_builtin_yaml: "Keel already runs the YAML language server",
    cursor_builtin_typescript: "Keel already uses TypeScript through vtsls",
    cursor_blocker_cursor_only: "made for Cursor only",
    cursor_blocker_debugger: "needs the debugger",
    cursor_blocker_notebooks: "needs notebooks",
    cursor_blocker_custom_editors: "its own editors",
    cursor_blocker_views: "its own panels",
    cursor_blocker_webviews: "web pages inside the window",
    cursor_blocker_language_server: "starts a language server",
    command_insert_at_mention: "Claude Code: Insert At-Mention",
    claude_not_connected: "Claude Code is not connected to this window: start it in a Keel terminal.",
    command_accept_proposed: "Claude Code: Accept Proposed Changes",
    command_reject_proposed: "Claude Code: Reject Proposed Changes",
    claude_proposed_edit: "Proposed by Claude Code",
    claude_accept: "Accept",
    claude_reject: "Reject",
    agent_view_terminal: "Terminal",
    agent_view_agent: "Agent",
    agent_placeholder: "Ask Claude… (Enter sends, Shift+Enter new line)",
    agent_no_data: "Keel does not know where to install Claude's adapter.",
    agent_stopped: "Claude stopped. Press Start to begin again; details are in the Output.",
    agent_stopped_text: "Turn ended: {reason}.",
    agent_reason_cancelled: "stopped",
    agent_reason_limit: "limit reached",
    agent_reason_refusal: "Claude refused",
    agent_history: "Past conversations",
    agent_new: "New conversation",
    agent_installing: "Installing Claude's adapter, only the first time (about 2 minutes)…",
    agent_starting: "Starting Claude…",
    agent_empty: "Write to Claude below. @ adds the open file or the selection.",
    agent_start: "Start",
    agent_working: "Claude is working…",
    agent_thinking: "Thinking",
    agent_plan_text: "Plan: {done} of {total}",
    agent_permission_text: "Claude asks permission: {title}",
    agent_mode: "Mode",
    agent_stop: "Stop",
    agent_send: "Send",
    agent_add_file: "Add the open file or the selection",
    command_open_agent: "Claude Code: Open Agent",
    command_new_agent: "Claude Code: New Conversation",
    command_add_to_agent: "Claude Code: Add File or Selection to Conversation",
    agent_changes_text: "Claude's changes: {count} files",
    agent_accept_all: "Accept all",
    agent_reject_all: "Reject all",
    agent_new_file: "new",
    agent_change_of_claude: "Change by Claude",
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_setting_wins_over_the_mac() {
        assert_eq!(
            UiLanguage::from_setting(Some("en"), Some("it-IT")),
            UiLanguage::English
        );
        assert_eq!(
            UiLanguage::from_setting(Some("it"), Some("en-US")),
            UiLanguage::Italian
        );
    }

    #[test]
    fn auto_follows_the_mac_and_falls_back_to_english() {
        assert_eq!(
            UiLanguage::from_setting(Some("auto"), Some("it-IT")),
            UiLanguage::Italian
        );
        assert_eq!(
            UiLanguage::from_setting(None, Some("IT")),
            UiLanguage::Italian
        );
        assert_eq!(
            UiLanguage::from_setting(None, Some("de-DE")),
            UiLanguage::English
        );
        assert_eq!(UiLanguage::from_setting(None, None), UiLanguage::English);
    }

    #[test]
    fn texts_fill_in_their_values() {
        assert_eq!(
            ITALIAN.save_changes("app.ts"),
            "Vuoi salvare le modifiche a app.ts?"
        );
        assert_eq!(
            ENGLISH.could_not_save("app.ts"),
            "Keel could not save app.ts"
        );
        assert_eq!(
            ITALIAN.could_not_start("manuel-local.vsauto-ghost", "boom"),
            "manuel-local.vsauto-ghost non è partita: boom"
        );
        assert_eq!(ITALIAN.coming_in_phase(3), "Arriva con la fase 3");
        assert_eq!(ENGLISH.line_column(33, 28), "Ln 33, Col 28");
        assert_eq!(ITALIAN.indentation(2, false), "Spazi: 2");
        assert_eq!(ENGLISH.indentation(4, true), "Tab Size: 4");
    }
}
