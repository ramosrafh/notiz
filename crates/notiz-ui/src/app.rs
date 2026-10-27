use std::{collections::HashSet, path::Path, time::Duration};

use iced::widget::{
    Space, button, checkbox, column, container, markdown, mouse_area, opaque, pin, row, scrollable,
    stack, text, text_editor, text_input, tooltip,
};
use iced::{
    Background, Border, Color, Element, Fill, Font, Padding, Subscription, Task, Theme, mouse,
    theme::Palette, time,
};
use notiz_core::{Backlink, KnowledgeGraph, Note, NoteSummary, renderable_markdown};
use notiz_services::{
    AppServices, AppSnapshot, DeleteResult, FolderMutationResult, MutationResult, NoteContext,
    NoteTemplate,
};
use uuid::Uuid;

use crate::graph;
use crate::searchable_markdown;
use crate::selectable_preview;
use crate::theme::{
    self, BACKGROUND, BORDER, DANGER, DIM, INDIGO, INDIGO_LIGHT, MUTED, SIDEBAR, SUCCESS, SURFACE,
    TEXT,
};

const SIDEBAR_DEFAULT_WIDTH: f32 = 320.0;
const SIDEBAR_MIN_WIDTH: f32 = 240.0;
const SIDEBAR_MAX_WIDTH: f32 = 540.0;
const SIDEBAR_COLLAPSED_WIDTH: f32 = 62.0;
const SIDEBAR_RESIZE_WIDTH: f32 = 5.0;
const TREE_INDENT: f32 = 18.0;
const TREE_LEADING_WIDTH: f32 = 40.0;
const TREE_ACTION_WIDTH: f32 = 28.0;
const TREE_ROW_PADDING_X: u16 = 8;
const INSPECTOR_WIDTH: f32 = 280.0;
const INSPECTOR_COLLAPSED_WIDTH: f32 = 56.0;
const AUTOSAVE_DELAY: Duration = Duration::from_millis(650);
const PREVIEW_DELAY: Duration = Duration::from_millis(140);
const UI_FONT: Font = Font::with_name("Inter");
const CODE_FONT: Font = Font::with_name("JetBrains Mono");

pub fn run(services: AppServices) -> iced::Result {
    iced::application(
        move || Notiz::new(services.clone()),
        Notiz::update,
        Notiz::view,
    )
    .title("Notiz")
    .theme(Notiz::theme)
    .subscription(Notiz::subscription)
    .font(iced_lucide::FONT_BYTES)
    .font(include_bytes!("../../../assets/fonts/inter-variable.ttf").as_slice())
    .font(include_bytes!("../../../assets/fonts/jetbrains-mono-variable.ttf").as_slice())
    .default_font(UI_FONT)
    .antialiasing(true)
    .window(iced::window::Settings {
        size: iced::Size::new(1360.0, 860.0),
        min_size: Some(iced::Size::new(880.0, 620.0)),
        position: iced::window::Position::Centered,
        exit_on_close_request: false,
        platform_specific: iced::window::settings::PlatformSpecific {
            application_id: "dev.notiz.Notiz".into(),
            ..Default::default()
        },
        ..Default::default()
    })
    .run()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WorkspaceMode {
    Editor,
    Preview,
    Split,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TreeSort {
    Alphabetical,
    Updated,
}

impl TreeSort {
    fn label(self) -> &'static str {
        match self {
            Self::Alphabetical => "A–Z",
            Self::Updated => "Updated",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WorkspaceTab {
    Note(Uuid),
    Graph,
}

#[derive(Debug, Clone)]
enum PaletteAction {
    CreateNote,
    CreateFromTemplate(NoteTemplate),
    OpenTree,
    OpenGraph,
    ToggleInspector,
    SetMode(WorkspaceMode),
    Save,
    Refresh,
    OpenNote(Uuid),
}

struct PaletteEntry {
    label: String,
    hint: String,
    action: PaletteAction,
}

#[derive(Debug)]
struct PreviewBlock {
    start_line: usize,
    end_line: usize,
}

#[derive(Debug, Clone)]
struct PreviewSource {
    start_line: usize,
    end_line: usize,
    markdown: String,
}

#[derive(Debug, Clone)]
struct SearchHighlighter {
    query: String,
    current_line: usize,
}

impl iced::advanced::text::Highlighter for SearchHighlighter {
    type Settings = String;
    type Highlight = ();
    type Iterator<'a> = std::vec::IntoIter<(std::ops::Range<usize>, ())>;

    fn new(settings: &Self::Settings) -> Self {
        Self {
            query: settings.clone(),
            current_line: 0,
        }
    }

    fn update(&mut self, settings: &Self::Settings) {
        self.query.clone_from(settings);
        self.current_line = 0;
    }

    fn change_line(&mut self, line: usize) {
        self.current_line = line;
    }

    fn highlight_line(&mut self, line: &str) -> Self::Iterator<'_> {
        self.current_line += 1;
        searchable_markdown::case_insensitive_ranges(line, &self.query)
            .into_iter()
            .map(|range| (range, ()))
            .collect::<Vec<_>>()
            .into_iter()
    }

    fn current_line(&self) -> usize {
        self.current_line
    }
}

#[derive(Debug, Clone, Copy)]
struct TabDrag {
    tab: WorkspaceTab,
    last_x: f32,
}

#[derive(Debug, Clone, Copy)]
struct TabReorderAnimation {
    first: WorkspaceTab,
    second: WorkspaceTab,
    progress: f32,
    target: f32,
    finish_after: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum TreeItem {
    Folder(String),
    Note(Uuid),
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum TreeContextTarget {
    Root,
    Folder(String),
    Note(Uuid),
}

#[derive(Debug, Clone)]
struct TreeContextMenu {
    target: TreeContextTarget,
    x: f32,
    y: f32,
}

#[derive(Debug, Clone)]
struct TreeDrag {
    item: TreeItem,
    origin_x: f32,
    active: bool,
}

#[derive(Debug, Clone)]
struct TreeHold {
    item: TreeItem,
    origin_x: f32,
    origin_y: f32,
}

#[derive(Debug, Clone)]
enum Message {
    Initialized(Result<AppSnapshot, String>),
    SearchChanged(String),
    SearchFinished(String, Result<Vec<NoteSummary>, String>),
    CycleTreeSort,
    OpenFolderCreator(String),
    FolderNameChanged(String),
    CreateFolderRequested,
    FolderCreated(Result<AppSnapshot, String>),
    CreateNote,
    CreateNoteIn(String),
    NoteCreatorTitleChanged(String),
    CancelCreators,
    ConfirmCreateNote,
    CreateFromTemplate(NoteTemplate),
    Created(Result<MutationResult, String>),
    OpenNote(Uuid),
    OpenHome,
    OpenGraph,
    SelectTab(WorkspaceTab),
    CloseTab(WorkspaceTab),
    BeginTabDrag(WorkspaceTab),
    ActivateTabDrag(WorkspaceTab),
    EndTabDrag,
    Loaded(Uuid, Result<NoteContext, String>),
    TitleChanged(String),
    TagsChanged(String),
    EditorAction(text_editor::Action),
    SetMode(WorkspaceMode),
    ToggleEditorPreview,
    SaveRequested,
    AutosaveTick,
    Saved(Uuid, u64, Result<MutationResult, String>),
    HoverNote(Option<Uuid>),
    HoverFolder(Option<String>),
    HoverNoteSelector(Option<Uuid>),
    HoverFolderSelector(Option<String>),
    HoverFolderNest(Option<String>),
    ToggleFolderExpanded(String),
    ToggleNoteSelection(Uuid),
    ToggleFolderSelection(String),
    OpenTreeContext(TreeContextTarget),
    DeleteTreeItem(TreeItem),
    DeleteSelection,
    SelectionDeleted(Vec<Uuid>, Vec<String>, Result<DeleteResult, String>),
    BeginTreeInteraction(TreeItem),
    ActivateTreeDrag(TreeItem),
    EndTreeInteraction,
    TreeNoteMoved(Uuid, Result<MutationResult, String>),
    TreeFolderMoved(String, Result<FolderMutationResult, String>),
    RefreshRequested,
    Refreshed(Result<AppSnapshot, String>),
    MarkdownLinkClicked(markdown::Uri),
    LinkResolved(Result<Option<Uuid>, String>),
    Graph(graph::Message),
    ToggleSidebar,
    ToggleInspector,
    SidebarResizeStart,
    SidebarCursorMoved(f32, f32),
    SidebarResizeEnd,
    TogglePalette,
    CloseOverlay,
    CloseWindow(iced::window::Id),
    PaletteQueryChanged(String),
    PaletteMove(i32),
    PaletteSubmit,
    PaletteExecute(PaletteAction),
    PreviewScrolled(f32),
    ToggleNoteSearch,
    NoteSearchChanged(String),
    NavigateNoteSearch(i32),
    PreviewPrepared(u64, Vec<PreviewSource>, selectable_preview::Document),
    ActivateWorkspaceMode(WorkspaceMode),
    FinishWorkspaceModePreparation(WorkspaceMode),
    ToggleVaultManager,
    VaultPathChanged(String),
    OpenVaultRequested,
    CreateVaultRequested,
    VaultSwitched(Result<(AppServices, AppSnapshot), String>),
    AnimationTick,
    ExternalTick,
    RevisionChecked(Result<u64, String>),
}

struct Notiz {
    services: AppServices,
    vault_path: String,
    notes: Vec<NoteSummary>,
    folders: Vec<String>,
    visible_notes: Vec<NoteSummary>,
    search: String,
    tree_sort: TreeSort,
    selected_folder: String,
    folder_creator_open: bool,
    folder_name: String,
    note_creator_open: bool,
    note_creator_title: String,
    note_creator_folder: String,
    hovered_note: Option<Uuid>,
    hovered_folder: Option<String>,
    hovered_note_selector: Option<Uuid>,
    hovered_folder_selector: Option<String>,
    selected_notes: HashSet<Uuid>,
    selected_folders: HashSet<String>,
    context_menu: Option<TreeContextMenu>,
    tree_hold: Option<TreeHold>,
    tree_drag: Option<TreeDrag>,
    tree_drop_target: Option<TreeItem>,
    tree_nest_target: Option<String>,
    tree_drag_progress: f32,
    collapsed_folders: HashSet<String>,
    active_note: Option<Note>,
    editor: text_editor::Content,
    preview_blocks: Vec<PreviewBlock>,
    preview_document: selectable_preview::Document,
    title: String,
    tags: String,
    backlinks: Vec<Backlink>,
    graph: KnowledgeGraph,
    mode: WorkspaceMode,
    open_tabs: Vec<WorkspaceTab>,
    active_tab: Option<WorkspaceTab>,
    sidebar_width: f32,
    sidebar_target: f32,
    sidebar_expanded_width: f32,
    sidebar_collapsed: bool,
    sidebar_resizing: bool,
    inspector_collapsed: bool,
    inspector_width: f32,
    inspector_target: f32,
    palette_open: bool,
    palette_query: String,
    palette_selected: usize,
    selected_line: usize,
    editor_scroll_ratio: f32,
    editor_scroll_target: f32,
    preview_scroll_ratio: f32,
    preview_scroll_target: f32,
    preview_scroll_programmatic: bool,
    cursor_x: f32,
    cursor_y: f32,
    tab_drag: Option<TabDrag>,
    tab_hold: Option<WorkspaceTab>,
    tab_reorder: Option<TabReorderAnimation>,
    dirty: bool,
    saving: bool,
    revision: u64,
    last_edit: Option<std::time::Instant>,
    preview_dirty: bool,
    preview_building: bool,
    preview_job_revision: u64,
    note_search_open: bool,
    note_search_query: String,
    note_search_matches: Vec<(usize, usize, usize)>,
    note_search_index: usize,
    workspace_loading: Option<WorkspaceMode>,
    workspace_mounting: bool,
    vault_manager_open: bool,
    vault_path_draft: String,
    busy: bool,
    status: String,
    vault_revision: u64,
    deferred_action: Option<Box<Message>>,
    external_checking: bool,
}

impl Notiz {
    fn new(services: AppServices) -> (Self, Task<Message>) {
        let vault_path = services.vault_path().display().to_string();
        let initialize = service_task(
            services.clone(),
            |services| services.initialize(),
            Message::Initialized,
        );
        (
            Self {
                services,
                vault_path: vault_path.clone(),
                notes: Vec::new(),
                folders: Vec::new(),
                visible_notes: Vec::new(),
                search: String::new(),
                tree_sort: TreeSort::Alphabetical,
                selected_folder: String::new(),
                folder_creator_open: false,
                folder_name: String::new(),
                note_creator_open: false,
                note_creator_title: String::new(),
                note_creator_folder: String::new(),
                hovered_note: None,
                hovered_folder: None,
                hovered_note_selector: None,
                hovered_folder_selector: None,
                selected_notes: HashSet::new(),
                selected_folders: HashSet::new(),
                context_menu: None,
                tree_hold: None,
                tree_drag: None,
                tree_drop_target: None,
                tree_nest_target: None,
                tree_drag_progress: 0.0,
                collapsed_folders: HashSet::new(),
                active_note: None,
                editor: text_editor::Content::new(),
                preview_blocks: build_preview_blocks(""),
                preview_document: selectable_preview::Document::parse("*Empty note.*"),
                title: String::new(),
                tags: String::new(),
                backlinks: Vec::new(),
                graph: KnowledgeGraph::default(),
                mode: WorkspaceMode::Editor,
                open_tabs: Vec::new(),
                active_tab: None,
                sidebar_width: SIDEBAR_DEFAULT_WIDTH,
                sidebar_target: SIDEBAR_DEFAULT_WIDTH,
                sidebar_expanded_width: SIDEBAR_DEFAULT_WIDTH,
                sidebar_collapsed: false,
                sidebar_resizing: false,
                inspector_collapsed: true,
                inspector_width: INSPECTOR_COLLAPSED_WIDTH,
                inspector_target: INSPECTOR_COLLAPSED_WIDTH,
                palette_open: false,
                palette_query: String::new(),
                palette_selected: 0,
                selected_line: 0,
                editor_scroll_ratio: 0.0,
                editor_scroll_target: 0.0,
                preview_scroll_ratio: 0.0,
                preview_scroll_target: 0.0,
                preview_scroll_programmatic: false,
                cursor_x: 0.0,
                cursor_y: 0.0,
                tab_drag: None,
                tab_hold: None,
                tab_reorder: None,
                dirty: false,
                saving: false,
                revision: 0,
                last_edit: None,
                preview_dirty: false,
                preview_building: false,
                preview_job_revision: 0,
                note_search_open: false,
                note_search_query: String::new(),
                note_search_matches: Vec::new(),
                note_search_index: 0,
                workspace_loading: None,
                workspace_mounting: false,
                vault_manager_open: false,
                vault_path_draft: vault_path.clone(),
                busy: true,
                status: "Preparing the vault…".into(),
                vault_revision: 0,
                deferred_action: None,
                external_checking: false,
            },
            initialize,
        )
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Initialized(result) => match result {
                Ok(snapshot) => {
                    self.apply_snapshot(snapshot);
                    self.busy = false;
                    self.status = "Vault ready".into();
                }
                Err(error) => {
                    self.busy = false;
                    self.status = error;
                }
            },
            Message::SearchChanged(query) => {
                self.search = query.clone();
                if query.trim().is_empty() {
                    self.visible_notes = self.notes.clone();
                    return Task::none();
                }
                let services = self.services.clone();
                let sent_query = query.clone();
                return Task::perform(
                    async move {
                        tokio::task::spawn_blocking(move || services.search(&query))
                            .await
                            .map_err(|error| error.to_string())?
                            .map_err(|error| error.to_string())
                    },
                    move |result| Message::SearchFinished(sent_query.clone(), result),
                );
            }
            Message::SearchFinished(query, result) => {
                if query == self.search {
                    match result {
                        Ok(notes) => self.visible_notes = notes,
                        Err(error) => self.status = error,
                    }
                }
            }
            Message::CycleTreeSort => {
                self.tree_sort = match self.tree_sort {
                    TreeSort::Alphabetical => TreeSort::Updated,
                    TreeSort::Updated => TreeSort::Alphabetical,
                };
                self.status = match self.tree_sort {
                    TreeSort::Alphabetical => "Tree sorted alphabetically".into(),
                    TreeSort::Updated => "Tree sorted by latest update".into(),
                };
            }
            Message::OpenFolderCreator(folder) => {
                self.context_menu = None;
                self.note_creator_open = false;
                self.note_creator_title.clear();
                self.folder_creator_open = true;
                self.selected_folder = folder;
                self.folder_name.clear();
                if self.sidebar_collapsed {
                    self.sidebar_collapsed = false;
                    self.sidebar_target = self.sidebar_expanded_width;
                }
                return iced::widget::operation::focus(folder_input_id());
            }
            Message::FolderNameChanged(name) => self.folder_name = name,
            Message::CreateFolderRequested => {
                let name = self.folder_name.trim();
                if name.is_empty() {
                    return Task::none();
                }
                let folder = if self.selected_folder.is_empty() {
                    name.to_owned()
                } else {
                    format!("{}/{name}", self.selected_folder)
                };
                self.status = "Creating folder…".into();
                return service_task(
                    self.services.clone(),
                    move |services| services.create_folder(&folder),
                    Message::FolderCreated,
                );
            }
            Message::FolderCreated(result) => match result {
                Ok(snapshot) => {
                    self.apply_snapshot(snapshot);
                    self.folder_creator_open = false;
                    self.folder_name.clear();
                    self.status = "Folder created".into();
                }
                Err(error) => self.status = error,
            },
            Message::CreateNote => {
                return self.update(Message::CreateNoteIn(self.selected_folder.clone()));
            }
            Message::CreateNoteIn(folder) => {
                self.palette_open = false;
                self.context_menu = None;
                self.folder_creator_open = false;
                self.folder_name.clear();
                self.note_creator_open = true;
                self.note_creator_title.clear();
                self.note_creator_folder = folder;
                return iced::widget::operation::focus(note_creator_title_id());
            }
            Message::NoteCreatorTitleChanged(title) => self.note_creator_title = title,
            Message::CancelCreators => return self.update(Message::CloseOverlay),
            Message::ConfirmCreateNote => {
                if self.busy {
                    return Task::none();
                }
                if let Some(task) = self.save_before(Message::ConfirmCreateNote) {
                    return task;
                }
                let title = self.note_creator_title.trim();
                if title.is_empty() {
                    return Task::none();
                }
                self.busy = true;
                self.note_creator_open = false;
                self.status = "Creating note…".into();
                let folder = self.note_creator_folder.clone();
                let title = title.to_owned();
                return service_task(
                    self.services.clone(),
                    move |services| services.create_note_in(&title, &folder),
                    Message::Created,
                );
            }
            Message::CreateFromTemplate(template) => {
                if self.busy {
                    return Task::none();
                }
                if let Some(task) = self.save_before(Message::CreateFromTemplate(template)) {
                    return task;
                }
                self.palette_open = false;
                self.context_menu = None;
                self.note_creator_open = false;
                self.folder_creator_open = false;
                self.busy = true;
                self.status = format!("Opening {}…", template.label());
                return service_task(
                    self.services.clone(),
                    move |services| services.create_note_from_template(template),
                    Message::Created,
                );
            }
            Message::Created(result) => {
                self.busy = false;
                match result {
                    Ok(mutation) => {
                        self.apply_snapshot(mutation.snapshot);
                        self.open_note_tab(mutation.context.note.id);
                        self.apply_context(mutation.context);
                        self.mode = WorkspaceMode::Editor;
                        self.status = "Note created".into();
                    }
                    Err(error) => self.status = error,
                }
            }
            Message::OpenNote(id) => {
                if self.busy || self.active_note.as_ref().is_some_and(|note| note.id == id) {
                    if !self.busy {
                        self.open_note_tab(id);
                    }
                    return Task::none();
                }
                if let Some(task) = self.save_before(Message::OpenNote(id)) {
                    return task;
                }
                return self.load_note_task(id);
            }
            Message::Loaded(id, result) => {
                self.busy = false;
                match result {
                    Ok(context) if context.note.id == id => {
                        if self.active_tab == Some(WorkspaceTab::Note(id)) {
                            self.open_note_tab(id);
                            self.apply_context(context);
                            self.mode = WorkspaceMode::Preview;
                            self.status = "Note loaded".into();
                        }
                    }
                    Ok(_) => {}
                    Err(error) => self.status = error,
                }
            }
            Message::TitleChanged(title) => {
                if self.busy {
                    return Task::none();
                }
                self.title = title;
                self.mark_dirty();
            }
            Message::TagsChanged(tags) => {
                if self.busy {
                    return Task::none();
                }
                self.tags = tags;
                self.mark_dirty();
            }
            Message::EditorAction(action) => {
                let edited = action.is_edit();
                if edited && self.busy {
                    return Task::none();
                }
                let scroll_lines = match &action {
                    text_editor::Action::Scroll { lines } => Some(*lines),
                    _ => None,
                };
                self.editor.perform(action);
                if edited {
                    self.preview_dirty = true;
                    self.mark_dirty();
                }
                let line_count = self.editor.line_count().max(1);
                if let Some(lines) = scroll_lines {
                    self.editor_scroll_ratio = (self.editor_scroll_ratio
                        + lines as f32 / line_count as f32)
                        .clamp(0.0, 1.0);
                } else {
                    self.selected_line = self.editor.cursor().position.line;
                    self.editor_scroll_ratio = line_ratio(self.selected_line, line_count);
                }
                self.editor_scroll_target = self.editor_scroll_ratio;
                if self.mode == WorkspaceMode::Split {
                    let sync_line = if scroll_lines.is_some() {
                        (self.editor_scroll_ratio * line_count.saturating_sub(1) as f32).round()
                            as usize
                    } else {
                        self.selected_line
                    };
                    self.preview_scroll_target = self.preview_ratio_for_line(sync_line);
                }
            }
            Message::SetMode(mode) => {
                if mode != WorkspaceMode::Preview
                    && self.mode == WorkspaceMode::Preview
                    && (self.editor.line_count() > 350
                        || self
                            .active_note
                            .as_ref()
                            .is_some_and(|note| note.body.len() > 40_000))
                {
                    self.workspace_loading = Some(mode);
                    self.workspace_mounting = false;
                    return Task::perform(
                        async move {
                            tokio::time::sleep(Duration::from_millis(70)).await;
                            mode
                        },
                        Message::ActivateWorkspaceMode,
                    );
                }
                self.mode = mode;
                if mode != WorkspaceMode::Editor {
                    self.preview_scroll_target = self.preview_ratio_for_line(self.selected_line);
                }
            }
            Message::ToggleEditorPreview => {
                let mode = match self.mode {
                    WorkspaceMode::Editor | WorkspaceMode::Split => WorkspaceMode::Preview,
                    WorkspaceMode::Preview => WorkspaceMode::Editor,
                };
                return self.update(Message::SetMode(mode));
            }
            Message::ActivateWorkspaceMode(mode) => {
                if self.workspace_loading == Some(mode) {
                    self.mode = mode;
                    self.workspace_mounting = true;
                    if mode != WorkspaceMode::Editor {
                        self.preview_scroll_target =
                            self.preview_ratio_for_line(self.selected_line);
                    }
                    return Task::perform(
                        async move {
                            tokio::time::sleep(Duration::from_millis(120)).await;
                            mode
                        },
                        Message::FinishWorkspaceModePreparation,
                    );
                }
            }
            Message::FinishWorkspaceModePreparation(mode) => {
                if self.workspace_loading == Some(mode) && self.workspace_mounting {
                    self.workspace_loading = None;
                    self.workspace_mounting = false;
                }
            }
            Message::SaveRequested => return self.save_task(),
            Message::AutosaveTick => {
                if self.preview_dirty
                    && !self.preview_building
                    && self
                        .last_edit
                        .is_some_and(|last_edit| last_edit.elapsed() >= PREVIEW_DELAY)
                {
                    return self.build_preview_task();
                }
                if self.dirty
                    && !self.saving
                    && self
                        .last_edit
                        .is_some_and(|last_edit| last_edit.elapsed() >= AUTOSAVE_DELAY)
                {
                    return self.save_task();
                }
            }
            Message::Saved(id, revision, result) => {
                self.saving = false;
                match result {
                    Ok(mutation) => {
                        self.apply_snapshot(mutation.snapshot);
                        if self.active_note.as_ref().is_some_and(|note| note.id == id) {
                            self.active_note = Some(mutation.context.note);
                            self.backlinks = mutation.context.backlinks;
                            if self.revision == revision {
                                self.dirty = false;
                                self.last_edit = None;
                            }
                        }
                        self.status = "Saved".into();
                        if let Some(action) = self.deferred_action.take() {
                            if let Some(task) = self.save_before(*action.clone()) {
                                return task;
                            }
                            return self.update(*action);
                        }
                    }
                    Err(error) => {
                        self.deferred_action = None;
                        self.dirty = true;
                        self.status = format!("Could not save: {error}");
                    }
                }
            }
            Message::HoverNote(note) => {
                self.hovered_note = note;
                if self.tree_drag.is_some() {
                    self.set_tree_drop_target(note.map(TreeItem::Note));
                }
            }
            Message::HoverFolder(folder) => {
                self.hovered_folder = folder.clone();
                if self.tree_drag.is_some() {
                    self.set_tree_drop_target(folder.map(TreeItem::Folder));
                }
            }
            Message::HoverNoteSelector(note) => self.hovered_note_selector = note,
            Message::HoverFolderSelector(folder) => self.hovered_folder_selector = folder,
            Message::HoverFolderNest(folder) => {
                if self.tree_drag.is_some() {
                    self.tree_nest_target = folder;
                }
            }
            Message::ToggleFolderExpanded(folder) => {
                if !self.collapsed_folders.insert(folder.clone()) {
                    self.collapsed_folders.remove(&folder);
                }
            }
            Message::ToggleNoteSelection(id) => {
                if !self.selected_notes.insert(id) {
                    self.selected_notes.remove(&id);
                }
            }
            Message::ToggleFolderSelection(folder) => {
                if !self.selected_folders.insert(folder.clone()) {
                    self.selected_folders.remove(&folder);
                }
            }
            Message::OpenTreeContext(target) => {
                if let TreeContextTarget::Folder(folder) = &target {
                    self.selected_folder = folder.clone();
                } else if matches!(target, TreeContextTarget::Root) {
                    self.selected_folder.clear();
                }
                self.context_menu = Some(TreeContextMenu {
                    target,
                    x: self.cursor_x,
                    y: self.cursor_y,
                });
                self.folder_creator_open = false;
                self.palette_open = false;
            }
            Message::DeleteTreeItem(item) => {
                self.context_menu = None;
                self.selected_notes.clear();
                self.selected_folders.clear();
                match item {
                    TreeItem::Note(id) => {
                        self.selected_notes.insert(id);
                    }
                    TreeItem::Folder(folder) => {
                        self.selected_folders.insert(folder);
                    }
                }
                return self.update(Message::DeleteSelection);
            }
            Message::DeleteSelection => {
                if self.selected_notes.is_empty() && self.selected_folders.is_empty() {
                    return Task::none();
                }
                let folders = self.selected_folders.iter().cloned().collect::<Vec<_>>();
                let ids = self
                    .selected_notes
                    .iter()
                    .copied()
                    .filter(|id| {
                        self.notes
                            .iter()
                            .find(|note| note.id == *id)
                            .is_none_or(|note| {
                                !folders
                                    .iter()
                                    .any(|folder| path_is_in_folder(&note.relative_path, folder))
                            })
                    })
                    .collect::<Vec<_>>();
                let services = self.services.clone();
                let pending = self
                    .active_note
                    .clone()
                    .filter(|note| {
                        self.dirty
                            && (self.selected_notes.contains(&note.id)
                                || folders
                                    .iter()
                                    .any(|folder| path_is_in_folder(&note.relative_path, folder)))
                    })
                    .map(|note| {
                        (
                            note,
                            self.title.clone(),
                            self.editor.text(),
                            self.tags.clone(),
                        )
                    });
                self.busy = true;
                self.status = format!(
                    "Moving {} selected items to Trash…",
                    ids.len() + folders.len()
                );
                let sent_ids = ids.clone();
                let sent_folders = folders.clone();
                return Task::perform(
                    async move {
                        tokio::task::spawn_blocking(move || {
                            if let Some((note, title, body, tags)) = pending {
                                services.save_note(note, title, body, tags)?;
                            }
                            services.delete_items(&ids, &folders)
                        })
                        .await
                        .map_err(|error| error.to_string())?
                        .map_err(|error| error.to_string())
                    },
                    move |result| {
                        Message::SelectionDeleted(sent_ids.clone(), sent_folders.clone(), result)
                    },
                );
            }
            Message::SelectionDeleted(ids, folders, result) => {
                self.busy = false;
                match result {
                    Ok(deleted) => {
                        let deleted_ids = ids.iter().copied().collect::<HashSet<_>>();
                        let deleted_tab_ids = self
                            .notes
                            .iter()
                            .filter(|note| {
                                deleted_ids.contains(&note.id)
                                    || folders.iter().any(|folder| {
                                        path_is_in_folder(&note.relative_path, folder)
                                    })
                            })
                            .map(|note| note.id)
                            .collect::<HashSet<_>>();
                        let active_deleted = self.active_note.as_ref().is_none_or(|note| {
                            deleted_ids.contains(&note.id)
                                || folders
                                    .iter()
                                    .any(|folder| path_is_in_folder(&note.relative_path, folder))
                        });
                        self.open_tabs.retain(|tab| {
                            !matches!(tab, WorkspaceTab::Note(id) if deleted_tab_ids.contains(id))
                        });
                        self.selected_notes.clear();
                        self.selected_folders.clear();
                        self.apply_snapshot(deleted.snapshot);
                        if active_deleted {
                            if let Some(context) = deleted.next {
                                self.open_note_tab(context.note.id);
                                self.apply_context(context);
                            } else {
                                self.clear_active_note();
                            }
                        }
                        self.status = format!(
                            "{} selected items moved to Trash",
                            ids.len() + folders.len()
                        );
                    }
                    Err(error) => self.status = error,
                }
            }
            Message::BeginTreeInteraction(item) => {
                self.tree_hold = Some(TreeHold {
                    item: item.clone(),
                    origin_x: self.cursor_x,
                    origin_y: self.cursor_y,
                });
                self.tree_drag = None;
                self.tree_drop_target = None;
                self.tree_nest_target = None;
                return Task::perform(
                    async move {
                        tokio::time::sleep(Duration::from_millis(180)).await;
                    },
                    move |_| Message::ActivateTreeDrag(item),
                );
            }
            Message::ActivateTreeDrag(item) => {
                if let Some(origin_x) = self
                    .tree_hold
                    .as_ref()
                    .filter(|hold| hold.item == item)
                    .map(|hold| hold.origin_x)
                {
                    let target = self
                        .hovered_note
                        .map(TreeItem::Note)
                        .or_else(|| self.hovered_folder.clone().map(TreeItem::Folder));
                    self.set_tree_drop_target(target);
                    self.tree_drag = Some(TreeDrag {
                        item,
                        origin_x,
                        active: true,
                    });
                }
            }
            Message::EndTreeInteraction => return self.finish_tree_interaction(),
            Message::TreeNoteMoved(id, result) => match result {
                Ok(mutation) => {
                    self.apply_snapshot(mutation.snapshot);
                    if self.active_note.as_ref().is_some_and(|note| note.id == id) {
                        self.apply_context(mutation.context);
                    }
                    self.status = "Note moved".into();
                }
                Err(error) => self.status = error,
            },
            Message::TreeFolderMoved(source, result) => match result {
                Ok(mutation) => {
                    if self.selected_folder == source
                        || self.selected_folder.starts_with(&format!("{source}/"))
                    {
                        self.selected_folder = format!(
                            "{}{}",
                            mutation.folder,
                            self.selected_folder
                                .strip_prefix(&source)
                                .unwrap_or_default()
                        );
                    }
                    self.apply_snapshot(mutation.snapshot);
                    self.status = format!("Folder moved to {}", mutation.folder);
                    if let Some(id) = self.active_note.as_ref().map(|note| note.id) {
                        return self.load_note_task(id);
                    }
                }
                Err(error) => self.status = error,
            },
            Message::RefreshRequested => {
                if self.dirty {
                    return self.save_task();
                }
                self.busy = true;
                self.status = "Reindexing Markdown…".into();
                return service_task(
                    self.services.clone(),
                    |services| services.refresh(),
                    Message::Refreshed,
                );
            }
            Message::Refreshed(result) => {
                self.busy = false;
                match result {
                    Ok(snapshot) => {
                        self.apply_snapshot(snapshot);
                        self.status = "Index updated".into();
                    }
                    Err(error) => self.status = error,
                }
            }
            Message::MarkdownLinkClicked(uri) => {
                if let Some(target) = internal_link_target(&uri) {
                    let services = self.services.clone();
                    return Task::perform(
                        async move {
                            tokio::task::spawn_blocking(move || services.resolve_link(&target))
                                .await
                                .map_err(|error| error.to_string())?
                                .map_err(|error| error.to_string())
                        },
                        Message::LinkResolved,
                    );
                }
                self.status = format!("External link: {uri}");
            }
            Message::LinkResolved(result) => match result {
                Ok(Some(id)) => return self.update(Message::OpenNote(id)),
                Ok(None) => self.status = "No note matches this link yet".into(),
                Err(error) => self.status = error,
            },
            Message::Graph(graph::Message::OpenNote(id)) => {
                return self.update(Message::OpenNote(id));
            }
            Message::OpenHome => {
                if self.busy {
                    return Task::none();
                }
                if let Some(task) = self.save_before(Message::OpenHome) {
                    return task;
                }
                self.active_tab = None;
            }
            Message::OpenGraph => {
                self.open_tab(WorkspaceTab::Graph);
            }
            Message::SelectTab(tab) => {
                if self.active_tab == Some(tab) {
                    return Task::none();
                }
                return match tab {
                    WorkspaceTab::Graph => {
                        self.active_tab = Some(tab);
                        Task::none()
                    }
                    WorkspaceTab::Note(id) => self.update(Message::OpenNote(id)),
                };
            }
            Message::CloseTab(tab) => {
                if self.busy {
                    return Task::none();
                }
                if self.active_tab == Some(tab)
                    && let Some(task) = self.save_before(Message::CloseTab(tab))
                {
                    return task;
                }
                if self.tab_hold == Some(tab) {
                    self.tab_hold = None;
                }
                if self.tab_drag.is_some_and(|drag| drag.tab == tab) {
                    self.tab_drag = None;
                }
                if self
                    .tab_reorder
                    .is_some_and(|animation| animation.first == tab || animation.second == tab)
                {
                    self.tab_reorder = None;
                }
                let was_active = self.active_tab == Some(tab);
                self.open_tabs.retain(|open| *open != tab);
                if was_active {
                    let next = self.open_tabs.last().copied();
                    self.active_tab = next;
                    return match next {
                        Some(WorkspaceTab::Note(id)) => self.load_note_task(id),
                        Some(WorkspaceTab::Graph) | None => Task::none(),
                    };
                }
            }
            Message::BeginTabDrag(tab) => {
                let switch = self.update(Message::SelectTab(tab));
                self.tab_hold = Some(tab);
                self.tab_drag = None;
                let hold = Task::perform(
                    async move {
                        tokio::time::sleep(Duration::from_millis(180)).await;
                    },
                    move |_| Message::ActivateTabDrag(tab),
                );
                return Task::batch([switch, hold]);
            }
            Message::ActivateTabDrag(tab) => {
                if self.tab_hold == Some(tab) {
                    self.tab_drag = Some(TabDrag {
                        tab,
                        last_x: self.cursor_x,
                    });
                }
            }
            Message::EndTabDrag => {
                self.tab_hold = None;
                if self.tab_drag.take().is_some()
                    && let Some(animation) = self.tab_reorder.as_mut()
                {
                    animation.target = if animation.target >= 0.5 { 1.0 } else { 0.0 };
                    animation.finish_after = true;
                }
            }
            Message::ToggleSidebar => {
                self.sidebar_collapsed = !self.sidebar_collapsed;
                if self.sidebar_collapsed {
                    self.sidebar_expanded_width = self
                        .sidebar_width
                        .max(SIDEBAR_MIN_WIDTH)
                        .clamp(SIDEBAR_MIN_WIDTH, SIDEBAR_MAX_WIDTH);
                    self.sidebar_target = SIDEBAR_COLLAPSED_WIDTH;
                } else {
                    self.sidebar_target = self
                        .sidebar_expanded_width
                        .clamp(SIDEBAR_MIN_WIDTH, SIDEBAR_MAX_WIDTH);
                }
            }
            Message::ToggleInspector => {
                self.inspector_collapsed = !self.inspector_collapsed;
                self.inspector_target = if self.inspector_collapsed {
                    INSPECTOR_COLLAPSED_WIDTH
                } else {
                    INSPECTOR_WIDTH
                };
            }
            Message::SidebarResizeStart => self.sidebar_resizing = true,
            Message::SidebarCursorMoved(x, y) => {
                self.cursor_x = x;
                self.cursor_y = y;
                if self.tree_drag.is_none()
                    && let Some(hold) = self.tree_hold.as_ref()
                    && ((x - hold.origin_x).powi(2) + (y - hold.origin_y).powi(2)).sqrt() >= 4.0
                {
                    return self.update(Message::ActivateTreeDrag(hold.item.clone()));
                }
                if self.sidebar_resizing && !self.sidebar_collapsed {
                    self.sidebar_width = x.clamp(SIDEBAR_MIN_WIDTH, SIDEBAR_MAX_WIDTH);
                    self.sidebar_target = self.sidebar_width;
                    self.sidebar_expanded_width = self.sidebar_width;
                }
                if let Some(drag) = self.tab_drag {
                    let delta = x - drag.last_x;
                    if let Some(animation) = self.tab_reorder {
                        let dragged_first = drag.tab == animation.first;
                        let other = if dragged_first {
                            animation.second
                        } else {
                            animation.first
                        };
                        let directional_delta = if dragged_first { delta } else { -delta };
                        let travel = self.tab_width(other) + 7.0;
                        if let Some(animation) = self.tab_reorder.as_mut() {
                            animation.target = (directional_delta / travel).clamp(0.0, 1.0);
                        }
                    } else if delta.abs() >= 24.0
                        && let Some(index) = self.open_tabs.iter().position(|tab| *tab == drag.tab)
                    {
                        let next = if delta > 0.0 {
                            (index + 1).min(self.open_tabs.len().saturating_sub(1))
                        } else {
                            index.saturating_sub(1)
                        };
                        if next != index {
                            let (first, second) = if index < next {
                                (self.open_tabs[index], self.open_tabs[next])
                            } else {
                                (self.open_tabs[next], self.open_tabs[index])
                            };
                            let travel = self.tab_width(self.open_tabs[next]) + 7.0;
                            self.tab_reorder = Some(TabReorderAnimation {
                                first,
                                second,
                                progress: 0.0,
                                target: (delta.abs() / travel).clamp(0.0, 1.0),
                                finish_after: false,
                            });
                        }
                    }
                }
            }
            Message::SidebarResizeEnd => {
                self.sidebar_resizing = false;
                if self.tab_drag.is_some() || self.tab_hold.is_some() {
                    return self.update(Message::EndTabDrag);
                }
                if self.tree_drag.is_some() || self.tree_hold.is_some() {
                    return self.update(Message::EndTreeInteraction);
                }
            }
            Message::TogglePalette => {
                self.palette_open = !self.palette_open;
                self.palette_query.clear();
                self.palette_selected = 0;
                if self.palette_open {
                    return iced::widget::operation::focus(palette_input_id());
                }
            }
            Message::CloseWindow(id) => {
                if self.busy {
                    return Task::none();
                }
                if let Some(task) = self.save_before(Message::CloseWindow(id)) {
                    return task;
                }
                return iced::window::close(id);
            }
            Message::CloseOverlay => {
                self.context_menu = None;
                self.folder_creator_open = false;
                self.folder_name.clear();
                self.note_creator_open = false;
                self.note_creator_title.clear();
                self.note_creator_folder.clear();
                self.palette_open = false;
                self.palette_query.clear();
                self.palette_selected = 0;
                self.vault_manager_open = false;
            }
            Message::ToggleVaultManager => {
                self.vault_manager_open = !self.vault_manager_open;
                self.palette_open = false;
                self.context_menu = None;
                self.note_creator_open = false;
                self.vault_path_draft = self.vault_path.clone();
                if self.vault_manager_open {
                    return iced::widget::operation::focus(vault_path_input_id());
                }
            }
            Message::VaultPathChanged(path) => self.vault_path_draft = path,
            Message::OpenVaultRequested | Message::CreateVaultRequested => {
                if self.busy || self.vault_path_draft.trim().is_empty() {
                    return Task::none();
                }
                let create = matches!(message, Message::CreateVaultRequested);
                if let Some(task) = self.save_before(message) {
                    return task;
                }
                self.busy = true;
                return self.switch_vault_task(create);
            }
            Message::VaultSwitched(result) => {
                self.busy = false;
                match result {
                    Ok((services, snapshot)) => {
                        self.services = services;
                        self.vault_path = self.services.vault_path().display().to_string();
                        self.vault_path_draft = self.vault_path.clone();
                        self.vault_manager_open = false;
                        self.reset_workspace_for_vault(snapshot);
                        self.status = "Vault ready".into();
                    }
                    Err(error) => self.status = error,
                }
            }
            Message::PaletteQueryChanged(query) => {
                self.palette_query = query;
                self.palette_selected = 0;
            }
            Message::PaletteMove(delta) => {
                if self.palette_open {
                    let count = self.palette_entries().len();
                    if count > 0 {
                        self.palette_selected = if delta < 0 {
                            self.palette_selected.saturating_sub(1)
                        } else {
                            (self.palette_selected + 1).min(count - 1)
                        };
                    }
                }
            }
            Message::PaletteSubmit => {
                if self.palette_open {
                    let action = self
                        .palette_entries()
                        .get(self.palette_selected)
                        .map(|entry| entry.action.clone());
                    if let Some(action) = action {
                        return self.update(Message::PaletteExecute(action));
                    }
                }
            }
            Message::PaletteExecute(action) => {
                self.palette_open = false;
                self.palette_query.clear();
                self.palette_selected = 0;
                return match action {
                    PaletteAction::CreateNote => {
                        self.update(Message::CreateNoteIn(self.selected_folder.clone()))
                    }
                    PaletteAction::OpenTree => {
                        if self.sidebar_collapsed {
                            self.sidebar_collapsed = false;
                            self.sidebar_target = self.sidebar_expanded_width;
                        }
                        Task::none()
                    }
                    PaletteAction::CreateFromTemplate(template) => {
                        self.update(Message::CreateFromTemplate(template))
                    }
                    PaletteAction::OpenGraph => self.update(Message::OpenGraph),
                    PaletteAction::ToggleInspector => {
                        self.inspector_collapsed = false;
                        self.inspector_target = INSPECTOR_WIDTH;
                        Task::none()
                    }
                    PaletteAction::SetMode(mode) => self.update(Message::SetMode(mode)),
                    PaletteAction::Save => self.update(Message::SaveRequested),
                    PaletteAction::Refresh => self.update(Message::RefreshRequested),
                    PaletteAction::OpenNote(id) => self.update(Message::OpenNote(id)),
                };
            }
            Message::PreviewScrolled(ratio) => {
                let ratio = ratio.clamp(0.0, 1.0);
                if self.preview_scroll_programmatic {
                    self.preview_scroll_programmatic = false;
                    self.preview_scroll_ratio = ratio;
                    return Task::none();
                }
                self.preview_scroll_target = ratio;
                if self.mode == WorkspaceMode::Split {
                    let line_count = self.editor.line_count().max(1);
                    let target_line = self.preview_line_for_ratio(ratio);
                    let target_editor_ratio = line_ratio(target_line, line_count);
                    let lines = ((target_editor_ratio - self.editor_scroll_ratio)
                        * line_count as f32)
                        .round() as i32;
                    if lines != 0 {
                        self.editor.perform(text_editor::Action::Scroll { lines });
                    }
                    self.editor_scroll_ratio = target_editor_ratio;
                    self.editor_scroll_target = target_editor_ratio;
                    self.selected_line = target_line;
                }
                self.preview_scroll_ratio = ratio;
            }
            Message::ToggleNoteSearch => {
                self.note_search_open = !self.note_search_open;
                if self.note_search_open {
                    return iced::widget::operation::focus(note_search_input_id());
                }
                self.note_search_query.clear();
                self.note_search_matches.clear();
                self.note_search_index = 0;
            }
            Message::NoteSearchChanged(query) => {
                self.note_search_query = query;
                self.refresh_note_search();
                if !self.note_search_matches.is_empty() {
                    return self.navigate_note_search(0);
                }
            }
            Message::NavigateNoteSearch(delta) => {
                return self.navigate_note_search(delta);
            }
            Message::PreviewPrepared(revision, sources, document) => {
                if revision == self.revision {
                    self.preview_job_revision = revision;
                    self.preview_blocks =
                        sources.into_iter().map(preview_block_from_source).collect();
                    self.preview_document = document;
                    self.preview_building = false;
                    self.preview_dirty = false;
                } else {
                    self.preview_building = false;
                }
            }
            Message::AnimationTick => {
                animate_ratio(
                    &mut self.tree_drag_progress,
                    if self.tree_drag.is_some() { 1.0 } else { 0.0 },
                    0.30,
                );
                if self.preview_building && self.preview_job_revision != self.revision {
                    self.preview_building = false;
                }
                animate_value(&mut self.sidebar_width, self.sidebar_target, 0.30);
                animate_value(&mut self.inspector_width, self.inspector_target, 0.28);
                let before = self.preview_scroll_ratio;
                animate_ratio(
                    &mut self.preview_scroll_ratio,
                    self.preview_scroll_target,
                    0.24,
                );
                if (before - self.preview_scroll_ratio).abs() > f32::EPSILON
                    && self.mode != WorkspaceMode::Editor
                {
                    self.preview_scroll_programmatic = true;
                    return snap_preview(self.preview_scroll_ratio);
                }
                if let Some(mut animation) = self.tab_reorder {
                    let distance = animation.target - animation.progress;
                    if distance.abs() < 0.004 {
                        animation.progress = animation.target;
                    } else {
                        animation.progress += distance * 0.38;
                    }
                    if animation.target >= 0.999 && animation.progress >= 0.985 {
                        let first = self
                            .open_tabs
                            .iter()
                            .position(|tab| *tab == animation.first);
                        let second = self
                            .open_tabs
                            .iter()
                            .position(|tab| *tab == animation.second);
                        if let (Some(first), Some(second)) = (first, second) {
                            let dragged_first = self
                                .tab_drag
                                .is_some_and(|drag| drag.tab == animation.first);
                            let travel = if dragged_first {
                                self.tab_width(animation.second) + 7.0
                            } else {
                                self.tab_width(animation.first) + 7.0
                            };
                            if let Some(drag) = self.tab_drag.as_mut() {
                                drag.last_x += if dragged_first { travel } else { -travel };
                            }
                            self.open_tabs.swap(first, second);
                        }
                        self.tab_reorder = None;
                    } else if animation.target <= 0.001
                        && animation.progress <= 0.015
                        && animation.finish_after
                    {
                        self.tab_reorder = None;
                    } else {
                        self.tab_reorder = Some(animation);
                    }
                }
            }
            Message::ExternalTick => {
                if self.busy || self.dirty || self.saving || self.external_checking {
                    return Task::none();
                }
                self.external_checking = true;
                return service_task(
                    self.services.clone(),
                    |services| services.vault_revision(),
                    Message::RevisionChecked,
                );
            }
            Message::RevisionChecked(result) => {
                self.external_checking = false;
                match result {
                    Ok(revision) if revision != self.vault_revision => {
                        self.busy = true;
                        self.status = "External change detected; reindexing…".into();
                        return service_task(
                            self.services.clone(),
                            |services| services.refresh(),
                            Message::Refreshed,
                        );
                    }
                    Ok(_) => {}
                    Err(error) => self.status = error,
                }
            }
        }
        Task::none()
    }

    fn view(&self) -> Element<'_, Message> {
        let resize_handle: Element<'_, Message> = if self.sidebar_collapsed {
            Space::new().width(0).into()
        } else {
            mouse_area(
                container(Space::new())
                    .width(SIDEBAR_RESIZE_WIDTH)
                    .height(Fill)
                    .style(move |_| {
                        theme::panel(
                            if self.sidebar_resizing {
                                INDIGO_LIGHT
                            } else {
                                Color::TRANSPARENT
                            },
                            Color::TRANSPARENT,
                            0.0,
                        )
                    }),
            )
            .on_press(Message::SidebarResizeStart)
            .interaction(mouse::Interaction::ResizingHorizontally)
            .into()
        };
        let base: Element<'_, Message> = container(
            row![
                self.sidebar(),
                resize_handle,
                self.main_workspace(),
                self.inspector_panel()
            ]
            .width(Fill)
            .height(Fill),
        )
        .width(Fill)
        .height(Fill)
        .style(|_| theme::panel(BACKGROUND, Color::TRANSPARENT, 0.0))
        .into();

        if !self.palette_open && !self.vault_manager_open {
            if let Some(menu) = &self.context_menu {
                let backdrop = mouse_area(container(Space::new()).width(Fill).height(Fill))
                    .on_press(Message::CloseOverlay)
                    .on_right_press(Message::CloseOverlay);
                let x = menu.x.clamp(8.0, 1120.0);
                let y = menu.y.clamp(8.0, 690.0);
                let overlay = pin(opaque(self.tree_context_menu(&menu.target))).x(x).y(y);
                return stack![base, backdrop, overlay].into();
            }
            return base;
        }

        let backdrop = mouse_area(container(Space::new()).width(Fill).height(Fill).style(|_| {
            theme::panel(
                Color::from_rgba(0.0, 0.0, 0.0, 0.64),
                Color::TRANSPARENT,
                0.0,
            )
        }))
        .on_press(Message::CloseOverlay)
        .on_right_press(Message::CloseOverlay);
        let overlay_content = if self.vault_manager_open {
            self.vault_manager()
        } else {
            self.command_palette()
        };
        let overlay = container(opaque(overlay_content))
            .width(Fill)
            .height(Fill)
            .align_x(iced::alignment::Horizontal::Center)
            .align_y(iced::alignment::Vertical::Top)
            .padding(Padding {
                top: 92.0,
                right: 18.0,
                bottom: 18.0,
                left: 18.0,
            });
        stack![base, backdrop, overlay].into()
    }

    fn sidebar(&self) -> Element<'_, Message> {
        if self.sidebar_width < 170.0 {
            let mut rail = column![
                button(lucide_icon("\u{E12A}").size(20).color(MUTED))
                    .on_press(Message::ToggleSidebar)
                    .padding([9, 12])
                    .style(theme::ghost_button),
            ]
            .spacing(10)
            .align_x(iced::Alignment::Center);
            rail = rail
                .push(
                    button(lucide_icon("\u{E33C}").size(17).color(MUTED))
                        .on_press(Message::ToggleSidebar)
                        .padding([10, 13])
                        .style(theme::ghost_button),
                )
                .push(tooltip(
                    button(lucide_icon("\u{E130}").size(17).color(INDIGO))
                        .on_press(Message::OpenHome)
                        .padding([10, 13])
                        .style(theme::ghost_button),
                    text("Início").size(12),
                    tooltip::Position::Right,
                ))
                .push(
                    button(lucide_icon("\u{E125}").size(17).color(MUTED))
                        .on_press(Message::OpenGraph)
                        .padding([10, 13])
                        .style(theme::ghost_button),
                );
            return container(rail.padding([14, 8]))
                .width(self.sidebar_width)
                .height(Fill)
                .style(|_| theme::panel(SIDEBAR, Color::TRANSPARENT, 0.0))
                .into();
        }

        let header = row![
            column![
                text("Notiz").size(24).color(TEXT),
                text(format!("{} notes", self.notes.len()))
                    .size(12)
                    .color(MUTED),
            ]
            .spacing(2)
            .width(Fill),
            button(lucide_icon("\u{E21C}").size(20).color(MUTED))
                .on_press(Message::ToggleSidebar)
                .padding([7, 10])
                .style(theme::ghost_button),
        ]
        .spacing(7)
        .align_y(iced::Alignment::Center);

        let footer = column![
            button(
                row![
                    lucide_icon("\u{E125}").size(15).color(MUTED),
                    text("Open graph").size(12).color(MUTED),
                    Space::new().width(Fill),
                    text(format!("{} nodes", self.graph.nodes.len()))
                        .size(11)
                        .color(DIM),
                ]
                .spacing(8)
                .align_y(iced::Alignment::Center),
            )
            .on_press(Message::OpenGraph)
            .width(Fill)
            .padding([9, 10])
            .style(theme::ghost_button),
            text("VAULT").size(10).color(DIM),
            button(
                row![
                    lucide_icon("\u{E40C}").size(13).color(MUTED),
                    text(shorten(&self.vault_path, 34)).size(11).color(MUTED),
                    Space::new().width(Fill),
                    text("Change").size(10).color(DIM),
                ]
                .spacing(7)
                .align_y(iced::Alignment::Center),
            )
            .on_press(Message::ToggleVaultManager)
            .width(Fill)
            .padding([8, 9])
            .style(theme::ghost_button),
        ]
        .spacing(7);

        let shortcuts = column![
            button(
                row![
                    text("Início").size(13),
                    Space::new().width(Fill),
                    text("⌂").size(16)
                ]
                .align_y(iced::Alignment::Center)
            )
            .on_press(Message::OpenHome)
            .padding([9, 12])
            .width(Fill)
            .style(|_, status| theme::note_button(self.active_tab.is_none(), status)),
            button(
                row![
                    text("Diário de hoje").size(13),
                    Space::new().width(Fill),
                    text("⇧ D").size(10).color(DIM)
                ]
                .align_y(iced::Alignment::Center)
            )
            .on_press_maybe(
                (!self.busy).then_some(Message::CreateFromTemplate(NoteTemplate::Daily))
            )
            .padding([9, 12])
            .width(Fill)
            .style(theme::ghost_button),
            row![
                button(text("+ Sessão").size(12))
                    .on_press_maybe(
                        (!self.busy).then_some(Message::CreateFromTemplate(NoteTemplate::Session))
                    )
                    .width(Fill)
                    .padding([9, 10])
                    .style(theme::ghost_button),
                button(text("+ Memória").size(12))
                    .on_press_maybe(
                        (!self.busy).then_some(Message::CreateFromTemplate(NoteTemplate::Memory))
                    )
                    .width(Fill)
                    .padding([9, 10])
                    .style(theme::ghost_button),
            ]
            .spacing(4),
        ]
        .spacing(3);
        container(
            column![header, shortcuts, self.tree_panel(), footer]
                .spacing(12)
                .padding(12),
        )
        .width(self.sidebar_width)
        .height(Fill)
        .style(|_| theme::panel(SIDEBAR, Color::TRANSPARENT, 0.0))
        .into()
    }

    fn tree_panel(&self) -> Element<'_, Message> {
        let search = text_input("Search notes…", &self.search)
            .id(search_input_id())
            .on_input(Message::SearchChanged)
            .padding([10, 12])
            .size(14)
            .style(theme::input);

        let search_controls = row![
            search,
            button(lucide_icon("\u{E0D7}").size(16).color(MUTED))
                .on_press(Message::OpenFolderCreator(self.selected_folder.clone()))
                .width(36)
                .height(36)
                .padding(0)
                .style(theme::ghost_button),
            button(lucide_icon("\u{E151}").size(16).color(MUTED))
                .on_press(Message::TogglePalette)
                .width(36)
                .height(36)
                .padding(0)
                .style(theme::ghost_button),
        ]
        .spacing(6)
        .align_y(iced::Alignment::Center);

        let mut tree_header = row![
            text("NOTES").size(10).color(DIM),
            Space::new().width(Fill),
            button(text(self.tree_sort.label()).size(10).color(MUTED))
                .on_press(Message::CycleTreeSort)
                .height(28)
                .padding([0, 8])
                .style(theme::ghost_button),
        ]
        .align_y(iced::Alignment::Center);
        if !self.selected_notes.is_empty() || !self.selected_folders.is_empty() {
            tree_header = tree_header.push(
                button(lucide_icon("\u{E18E}").size(15).color(DANGER))
                    .on_press(Message::DeleteSelection)
                    .width(28)
                    .height(28)
                    .padding(0)
                    .style(theme::danger_button),
            );
        }

        let ordered_notes = self.ordered_visible_notes();
        let ordered_folders = self.ordered_folders();
        let mut list = column![].spacing(2).width(Fill);
        if self.folder_creator_open && self.selected_folder.is_empty() {
            list = list.push(self.tree_creator_input(0, true));
        }
        if self.note_creator_open && self.note_creator_folder.is_empty() {
            list = list.push(self.tree_creator_input(0, false));
        }
        for note in ordered_notes
            .iter()
            .copied()
            .filter(|note| note_folder(&note.relative_path).is_empty())
        {
            list = list.push(self.note_card(note, 10.0));
        }

        for folder in &ordered_folders {
            if !self.folder_row_visible(folder) {
                continue;
            }
            let selected = self.selected_folder == *folder;
            let expanded = !self.collapsed_folders.contains(folder);
            let depth = folder.split('/').count().saturating_sub(1);
            let label = Path::new(folder)
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or(folder)
                .to_owned();
            let checked = self.selected_folders.contains(folder);
            let hovered = self.hovered_folder.as_deref() == Some(folder.as_str());
            let selector_hovered = self.hovered_folder_selector.as_deref() == Some(folder.as_str());
            let dragging = self
                .tree_drag
                .as_ref()
                .is_some_and(|drag| drag.active && drag.item == TreeItem::Folder(folder.clone()));
            let drop_target = self.tree_drop_target == Some(TreeItem::Folder(folder.clone()));
            let folder_indicator: Element<'_, Message> = if selector_hovered || checked {
                let folder = folder.clone();
                checkbox(checked)
                    .on_toggle(move |_| Message::ToggleFolderSelection(folder.clone()))
                    .size(16)
                    .into()
            } else {
                container(
                    lucide_icon(if expanded { "\u{E247}" } else { "\u{E0D7}" })
                        .size(14)
                        .color(if expanded { INDIGO_LIGHT } else { MUTED }),
                )
                .width(16)
                .align_x(iced::alignment::Horizontal::Center)
                .into()
            };
            let folder = folder.clone();
            let folder_selector = mouse_area(
                container(folder_indicator)
                    .width(22)
                    .align_x(iced::alignment::Horizontal::Center),
            )
            .on_enter(Message::HoverFolderSelector(Some(folder.clone())))
            .on_exit(Message::HoverFolderSelector(None));
            let folder_body = mouse_area(
                row![
                    text(label)
                        .size(13)
                        .color(if selected { TEXT } else { MUTED }),
                    Space::new().width(Fill),
                ]
                .spacing(7)
                .align_y(iced::Alignment::Center)
                .width(Fill),
            )
            .on_enter(Message::HoverFolderNest(Some(folder.clone())))
            .on_exit(Message::HoverFolderNest(None));
            let folder_leading = row![
                container(
                    text(if expanded { "⌄" } else { "›" })
                        .size(15)
                        .color(if expanded { INDIGO_LIGHT } else { MUTED }),
                )
                .width(18)
                .align_x(iced::alignment::Horizontal::Center),
                folder_selector,
            ]
            .width(TREE_LEADING_WIDTH)
            .spacing(0)
            .align_y(iced::Alignment::Center);
            let add_note: Element<'_, Message> = container(if hovered {
                button(text("+").size(16).color(MUTED))
                    .on_press(Message::CreateNoteIn(folder.clone()))
                    .width(TREE_ACTION_WIDTH)
                    .height(26)
                    .padding(0)
                    .style(theme::ghost_button)
            } else {
                button(text("").size(16))
                    .width(TREE_ACTION_WIDTH)
                    .height(26)
                    .padding(0)
                    .style(theme::transparent_button)
            })
            .width(TREE_ACTION_WIDTH)
            .height(26)
            .into();
            let folder_row = container(
                row![
                    Space::new().width(depth as f32 * TREE_INDENT),
                    folder_leading,
                    folder_body,
                    add_note,
                ]
                .spacing(4)
                .align_y(iced::Alignment::Center),
            )
            .width(Fill)
            .padding([6, TREE_ROW_PADDING_X])
            .style(move |_| {
                theme::folder_surface(
                    selected,
                    expanded,
                    hovered,
                    dragging || drop_target,
                    self.tree_drag_progress,
                )
            });
            list = list.push(
                mouse_area(folder_row)
                    .on_enter(Message::HoverFolder(Some(folder.clone())))
                    .on_exit(Message::HoverFolder(None))
                    .on_press(Message::BeginTreeInteraction(TreeItem::Folder(
                        folder.clone(),
                    )))
                    .on_release(Message::EndTreeInteraction)
                    .on_right_press(Message::OpenTreeContext(TreeContextTarget::Folder(
                        folder.clone(),
                    )))
                    .interaction(if dragging {
                        mouse::Interaction::Grabbing
                    } else {
                        mouse::Interaction::Pointer
                    }),
            );
            if self.folder_creator_open && self.selected_folder == *folder {
                list = list.push(self.tree_creator_input(depth, true));
            }
            if self.note_creator_open && self.note_creator_folder == *folder {
                list = list.push(self.tree_creator_input(depth, false));
            }
            if expanded {
                for note in ordered_notes
                    .iter()
                    .copied()
                    .filter(|note| note_folder(&note.relative_path) == *folder)
                {
                    let note_indent = depth as f32 * TREE_INDENT + 10.0;
                    list = list.push(self.note_card(note, note_indent));
                }
            }
        }

        let drag_hint: Element<'_, Message> = if self.tree_drag.is_some() {
            container(
                text("Drop on a folder to move · drag left to move out")
                    .size(10)
                    .color(INDIGO_LIGHT),
            )
            .padding([7, 8])
            .style(|_| theme::panel(SURFACE, BORDER, 7.0))
            .into()
        } else {
            Space::new().height(0).into()
        };

        column![
            search_controls,
            tree_header,
            mouse_area(scrollable(list).height(Fill))
                .on_right_press(Message::OpenTreeContext(TreeContextTarget::Root)),
            drag_hint,
        ]
        .spacing(8)
        .height(Fill)
        .into()
    }

    fn note_card<'a>(&'a self, note: &'a NoteSummary, indent: f32) -> Element<'a, Message> {
        let active = self
            .active_tab
            .is_some_and(|tab| tab == WorkspaceTab::Note(note.id));
        let excerpt = if note.excerpt.is_empty() {
            note.relative_path.clone()
        } else {
            shorten(&note.excerpt, 56)
        };
        let selected = self.selected_notes.contains(&note.id);
        let hovered = self.hovered_note == Some(note.id);
        let selector_hovered = self.hovered_note_selector == Some(note.id);
        let dragging = self
            .tree_drag
            .as_ref()
            .is_some_and(|drag| drag.active && drag.item == TreeItem::Note(note.id));
        let drop_target = self.tree_drop_target == Some(TreeItem::Note(note.id));
        let selector: Element<'_, Message> = if selector_hovered || selected {
            let id = note.id;
            checkbox(selected)
                .on_toggle(move |_| Message::ToggleNoteSelection(id))
                .size(16)
                .into()
        } else {
            Space::new().width(16).into()
        };
        let id = note.id;
        let selector = mouse_area(
            container(selector)
                .width(22)
                .align_x(iced::alignment::Horizontal::Center),
        )
        .on_enter(Message::HoverNoteSelector(Some(id)))
        .on_exit(Message::HoverNoteSelector(None));
        let card = container(
            row![
                Space::new().width(indent),
                selector,
                column![
                    text(&note.title)
                        .size(13)
                        .color(if active { TEXT } else { MUTED }),
                    text(excerpt)
                        .size(10)
                        .color(DIM)
                        .width(Fill)
                        .wrapping(iced::widget::text::Wrapping::None),
                ]
                .spacing(1)
                .width(Fill),
            ]
            .spacing(6)
            .align_y(iced::Alignment::Center),
        )
        .width(Fill)
        .padding([7, TREE_ROW_PADDING_X])
        .style(move |_| {
            theme::note_surface(
                active,
                indent > TREE_LEADING_WIDTH,
                hovered,
                dragging || drop_target,
                self.tree_drag_progress,
            )
        });
        mouse_area(card)
            .on_enter(Message::HoverNote(Some(note.id)))
            .on_exit(Message::HoverNote(None))
            .on_press(Message::BeginTreeInteraction(TreeItem::Note(note.id)))
            .on_release(Message::EndTreeInteraction)
            .on_right_press(Message::OpenTreeContext(TreeContextTarget::Note(note.id)))
            .interaction(if dragging {
                mouse::Interaction::Grabbing
            } else {
                mouse::Interaction::Pointer
            })
            .into()
    }

    fn inspector_panel(&self) -> Element<'_, Message> {
        if self.inspector_width < 160.0 {
            return container(
                column![
                    button(lucide_icon("\u{E431}").size(20).color(MUTED))
                        .on_press(Message::ToggleInspector)
                        .padding([8, 8])
                        .style(theme::ghost_button),
                    lucide_icon("\u{E102}").size(16).color(DIM),
                ]
                .spacing(13)
                .align_x(iced::Alignment::Center)
                .padding([12, 4]),
            )
            .width(self.inspector_width)
            .height(Fill)
            .style(|_| theme::panel(SIDEBAR, Color::TRANSPARENT, 0.0))
            .into();
        }

        let mut backlinks = column![].spacing(6).width(Fill);
        if self.backlinks.is_empty() {
            backlinks = backlinks.push(text("No backlinks.").size(12).color(DIM));
        } else {
            for backlink in &self.backlinks {
                backlinks = backlinks.push(
                    button(
                        column![
                            text(&backlink.source_title).size(13).color(TEXT),
                            text(&backlink.source_path).size(11).color(DIM),
                        ]
                        .spacing(3),
                    )
                    .on_press(Message::OpenNote(backlink.source_id))
                    .padding([8, 9])
                    .width(Fill)
                    .style(theme::ghost_button),
                );
            }
        }

        let active_id = self.active_note.as_ref().map(|note| note.id);
        let outgoing_edges = self
            .graph
            .edges
            .iter()
            .filter(|edge| Some(edge.source) == active_id)
            .collect::<Vec<_>>();
        let mut outgoing = column![].spacing(6).width(Fill);
        if outgoing_edges.is_empty() {
            outgoing = outgoing.push(text("No outgoing links.").size(12).color(DIM));
        } else {
            for edge in &outgoing_edges {
                if let Some(target) = edge.target
                    && let Some(node) = self.graph.nodes.iter().find(|node| node.id == target)
                {
                    outgoing = outgoing.push(
                        button(
                            column![
                                text(&node.title).size(13).color(TEXT),
                                text(&node.relative_path).size(11).color(DIM),
                            ]
                            .spacing(3),
                        )
                        .on_press(Message::OpenNote(target))
                        .padding([8, 9])
                        .width(Fill)
                        .style(theme::ghost_button),
                    );
                } else if let Some(target) = &edge.unresolved_target {
                    outgoing = outgoing.push(
                        container(
                            column![
                                text(target).size(13).color(MUTED),
                                text("unresolved link").size(11).color(DIM),
                            ]
                            .spacing(3),
                        )
                        .padding([8, 9]),
                    );
                }
            }
        }

        let mut tags = column![].spacing(5).width(Fill);
        if let Some(note) = &self.active_note {
            if note.tags.is_empty() {
                tags = tags.push(text("No tags.").size(12).color(DIM));
            } else {
                for tag in &note.tags {
                    tags = tags.push(
                        container(text(format!("#{tag}")).size(12).color(INDIGO_LIGHT))
                            .padding([6, 8])
                            .width(Fill)
                            .style(|_| {
                                theme::panel(
                                    Color::from_rgba(1.0, 0.78, 0.60, 0.10),
                                    Color::TRANSPARENT,
                                    7.0,
                                )
                            }),
                    );
                }
            }
        }

        let body = column![
            row![
                lucide_icon("\u{E102}").size(14).color(MUTED),
                text(format!("BACKLINKS · {}", self.backlinks.len()))
                    .size(11)
                    .color(MUTED),
            ]
            .spacing(7)
            .align_y(iced::Alignment::Center),
            backlinks,
            row![
                lucide_icon("\u{E102}").size(14).color(MUTED),
                text(format!("OUTGOING LINKS · {}", outgoing_edges.len()))
                    .size(11)
                    .color(MUTED),
            ]
            .spacing(7)
            .align_y(iced::Alignment::Center),
            outgoing,
            row![
                lucide_icon("\u{E35C}").size(14).color(MUTED),
                text("TAGS").size(11).color(MUTED),
            ]
            .spacing(7)
            .align_y(iced::Alignment::Center),
            tags,
        ]
        .spacing(10)
        .width(Fill);

        container(
            column![
                row![
                    column![
                        text("CONTEXT").size(13).color(TEXT),
                        text(format!("{} references", self.backlinks.len()))
                            .size(11)
                            .color(DIM),
                    ]
                    .spacing(2)
                    .width(Fill),
                    button(lucide_icon("\u{E432}").size(20).color(MUTED))
                        .on_press(Message::ToggleInspector)
                        .padding([7, 10])
                        .style(theme::ghost_button),
                ]
                .align_y(iced::Alignment::Center),
                scrollable(body).height(Fill),
            ]
            .spacing(12)
            .padding(13),
        )
        .width(self.inspector_width)
        .height(Fill)
        .style(|_| theme::panel(SIDEBAR, Color::TRANSPARENT, 0.0))
        .into()
    }

    fn home_workspace(&self) -> Element<'_, Message> {
        let mut recent = column![text("RETOMAR DE ONDE PAROU").size(11).color(DIM)].spacing(8);
        let mut notes = self.notes.iter().collect::<Vec<_>>();
        notes.sort_by_key(|note| std::cmp::Reverse(note.updated_at));
        for note in notes.into_iter().take(7) {
            recent = recent.push(
                button(
                    row![
                        lucide_icon("\u{E130}").size(16).color(INDIGO),
                        column![
                            text(&note.title).size(14).color(TEXT),
                            text(&note.relative_path).size(11).color(DIM),
                        ]
                        .spacing(3)
                        .width(Fill),
                        text(note.updated_at.format("%d/%m").to_string())
                            .size(11)
                            .color(DIM),
                    ]
                    .spacing(12)
                    .align_y(iced::Alignment::Center),
                )
                .on_press(Message::OpenNote(note.id))
                .width(Fill)
                .padding([12, 14])
                .style(theme::ghost_button),
            );
        }
        if self.notes.is_empty() {
            recent = recent.push(
                text("Suas notas recentes aparecerão aqui.")
                    .size(13)
                    .color(MUTED),
            );
        }
        let content = column![
            text("SEU ESPAÇO PESSOAL").size(11).color(INDIGO),
            text("Menos dispersão. Mais contexto.").size(32).color(TEXT),
            text("Reúna seu dia, suas sessões e o que vale lembrar.\nTudo em Markdown, no seu computador.")
                .size(15).color(MUTED),
            Space::new().height(16),
            self.template_button(NoteTemplate::Daily, "Diário de hoje", "Prioridades, registro e próximos passos", "\u{E130}"),
            self.template_button(NoteTemplate::Session, "Nova sessão", "Contexto, decisões e continuidade", "\u{E05F}"),
            self.template_button(NoteTemplate::Memory, "Nova memória", "Aprendizados e referências duráveis", "\u{E125}"),
            button(text("+ Nota livre").size(13))
                .on_press(Message::CreateNote)
                .padding([10, 14])
                .style(theme::ghost_button),
            Space::new().height(18),
            recent,
            Space::new().height(12),
            text("Conecte ideias com [[links]]  ·  Ctrl+K para comandos  ·  Ctrl+Shift+D para hoje")
                .size(11).color(DIM),
            self.status_bar(),
        ]
        .spacing(10)
        .max_width(780)
        .padding([28, 32]);
        scrollable(
            container(content)
                .width(Fill)
                .align_x(iced::Alignment::Center),
        )
        .height(Fill)
        .into()
    }

    fn template_button<'a>(
        &self,
        template: NoteTemplate,
        label: &'a str,
        description: &'a str,
        icon: &'static str,
    ) -> Element<'a, Message> {
        button(
            row![
                lucide_icon(icon).size(20).color(INDIGO),
                column![
                    text(label).size(15).color(TEXT),
                    text(description).size(12).color(MUTED),
                ]
                .spacing(4)
                .width(Fill),
                text("→").size(18).color(DIM),
            ]
            .spacing(14)
            .align_y(iced::Alignment::Center),
        )
        .on_press_maybe((!self.busy).then_some(Message::CreateFromTemplate(template)))
        .width(Fill)
        .padding([16, 18])
        .style(|_, status| theme::note_button(false, status))
        .into()
    }

    fn main_workspace(&self) -> Element<'_, Message> {
        let content = match self.active_tab {
            Some(WorkspaceTab::Graph) => self.graph_workspace(),
            Some(WorkspaceTab::Note(_)) => self.note_workspace(),
            None => self.home_workspace(),
        };
        container(column![self.tab_bar(), content])
            .width(Fill)
            .height(Fill)
            .style(|_| theme::panel(BACKGROUND, Color::TRANSPARENT, 0.0))
            .into()
    }

    fn tab_bar(&self) -> Element<'_, Message> {
        let mut tabs = row![tooltip(
            button(lucide_icon("\u{E130}").size(16).color(INDIGO))
                .on_press(Message::OpenHome)
                .width(34)
                .height(38)
                .padding(8)
                .style(theme::ghost_button),
            text("Início").size(12),
            tooltip::Position::Bottom,
        )]
        .spacing(7)
        .align_y(iced::Alignment::Center);
        let mut index = 0;
        while index < self.open_tabs.len() {
            if let Some(animation) = self.tab_reorder
                && self.open_tabs[index] == animation.first
                && self.open_tabs.get(index + 1) == Some(&animation.second)
            {
                let first_width = self.tab_width(animation.first);
                let second_width = self.tab_width(animation.second);
                let eased = 1.0 - (1.0 - animation.progress).powi(3);
                let first_x = eased * (second_width + 7.0);
                let second_x = (1.0 - eased) * (first_width + 7.0);
                let first_dragging = self
                    .tab_drag
                    .is_some_and(|drag| drag.tab == animation.first);
                let second_dragging = self
                    .tab_drag
                    .is_some_and(|drag| drag.tab == animation.second);
                let first_layer = row![
                    Space::new().width(first_x),
                    self.tab_element(animation.first, first_dragging, first_width),
                ];
                let second_layer = row![
                    Space::new().width(second_x),
                    self.tab_element(animation.second, second_dragging, second_width),
                ];
                let pair: Element<'_, Message> = if first_dragging {
                    stack![second_layer, first_layer]
                        .width(first_width + second_width + 7.0)
                        .height(40)
                        .into()
                } else {
                    stack![first_layer, second_layer]
                        .width(first_width + second_width + 7.0)
                        .height(40)
                        .into()
                };
                tabs = tabs.push(pair);
                index += 2;
                continue;
            }
            let tab = self.open_tabs[index];
            let dragging = self.tab_drag.is_some_and(|drag| drag.tab == tab);
            tabs = tabs.push(self.tab_element(tab, dragging, self.tab_width(tab)));
            index += 1;
        }
        tabs = tabs.push(
            button(
                container(lucide_icon("\u{E13D}").size(17).color(MUTED))
                    .width(Fill)
                    .height(Fill)
                    .center(Fill),
            )
            .on_press(Message::CreateNote)
            .width(38)
            .height(38)
            .padding(0)
            .style(theme::ghost_button),
        );
        container(
            scrollable(tabs).direction(scrollable::Direction::Horizontal(
                scrollable::Scrollbar::hidden(),
            )),
        )
        .width(Fill)
        .height(52)
        .padding([6, 10])
        .style(|_| theme::panel(BACKGROUND, Color::TRANSPARENT, 0.0))
        .into()
    }

    fn tab_element(&self, tab: WorkspaceTab, dragging: bool, width: f32) -> Element<'_, Message> {
        let active = self.active_tab == Some(tab);
        let label = self.tab_label(tab);
        let icon = match tab {
            WorkspaceTab::Note(_) => "\u{E130}",
            WorkspaceTab::Graph => "\u{E125}",
        };
        let tab_content = container(
            row![
                container(
                    row![
                        lucide_icon(icon)
                            .size(15)
                            .color(if active { INDIGO_LIGHT } else { DIM }),
                        text(label)
                            .size(12)
                            .color(if active { TEXT } else { MUTED }),
                    ]
                    .spacing(7)
                    .height(Fill)
                    .align_y(iced::Alignment::Center),
                )
                .height(Fill)
                .padding([0, 6])
                .align_y(iced::alignment::Vertical::Center),
                Space::new().width(Fill),
                button(
                    container(lucide_icon("\u{E1B2}").size(13).color(MUTED))
                        .width(Fill)
                        .height(Fill)
                        .center(Fill),
                )
                .on_press(Message::CloseTab(tab))
                .width(26)
                .height(26)
                .padding(0)
                .style(theme::transparent_button),
            ]
            .spacing(3)
            .height(Fill)
            .align_y(iced::Alignment::Center),
        )
        .width(width)
        .height(38)
        .padding([0, 3])
        .style(move |_| theme::tab_surface(active, dragging));
        mouse_area(tab_content)
            .on_press(Message::BeginTabDrag(tab))
            .on_release(Message::EndTabDrag)
            .interaction(if dragging {
                mouse::Interaction::Grabbing
            } else {
                mouse::Interaction::Pointer
            })
            .into()
    }

    fn tab_label(&self, tab: WorkspaceTab) -> &str {
        match tab {
            WorkspaceTab::Note(id) => self
                .notes
                .iter()
                .find(|note| note.id == id)
                .map(|note| note.title.as_str())
                .unwrap_or("Note"),
            WorkspaceTab::Graph => "Graph",
        }
    }

    fn tab_width(&self, tab: WorkspaceTab) -> f32 {
        (76.0 + self.tab_label(tab).chars().count() as f32 * 7.2).clamp(132.0, 250.0)
    }

    fn graph_workspace(&self) -> Element<'_, Message> {
        let unresolved = self
            .graph
            .edges
            .iter()
            .filter(|edge| edge.target.is_none())
            .count();
        container(
            column![
                row![
                    column![
                        text("Knowledge graph").size(18).color(TEXT),
                        text(format!(
                            "{} nodes · {} relationships · {unresolved} unresolved links",
                            self.graph.nodes.len(),
                            self.graph.edges.len()
                        ))
                        .size(10)
                        .color(MUTED),
                    ]
                    .spacing(3),
                    Space::new().width(Fill),
                    button(lucide_icon("\u{E145}").size(15).color(MUTED))
                        .on_press(Message::RefreshRequested)
                        .padding([8, 10])
                        .style(theme::ghost_button),
                ]
                .align_y(iced::Alignment::Center),
                container(
                    graph::view(&self.graph, self.active_note.as_ref().map(|note| note.id))
                        .map(Message::Graph),
                )
                .width(Fill)
                .height(Fill)
                .style(|_| theme::panel(BACKGROUND, Color::TRANSPARENT, 0.0)),
            ]
            .spacing(8)
            .padding(12),
        )
        .width(Fill)
        .height(Fill)
        .style(|_| theme::panel(BACKGROUND, Color::TRANSPARENT, 0.0))
        .into()
    }

    fn note_workspace(&self) -> Element<'_, Message> {
        if self.active_note.is_none() {
            return container(
                column![
                    text(if self.busy {
                        "Loading…"
                    } else {
                        "No note open"
                    })
                    .size(24)
                    .color(TEXT),
                    text("Create a note to start building your knowledge graph.")
                        .size(13)
                        .color(MUTED),
                ]
                .spacing(8),
            )
            .width(Fill)
            .height(Fill)
            .center(Fill)
            .style(|_| theme::panel(BACKGROUND, Color::TRANSPARENT, 0.0))
            .into();
        }

        let title = text_input("Title", &self.title)
            .on_input(Message::TitleChanged)
            .padding([6, 8])
            .size(20)
            .style(theme::inline_input);
        let mode_icon = match self.mode {
            WorkspaceMode::Editor | WorkspaceMode::Split => "\u{E130}",
            WorkspaceMode::Preview => "\u{E05F}",
        };
        let split_active = self.mode == WorkspaceMode::Split;
        let header = row![
            title,
            button(lucide_icon(mode_icon).size(18).color(MUTED))
                .on_press(Message::ToggleEditorPreview)
                .padding([8, 10])
                .style(theme::ghost_button),
            button(
                lucide_icon("\u{E099}")
                    .size(19)
                    .color(if split_active { TEXT } else { MUTED })
            )
            .on_press(Message::SetMode(WorkspaceMode::Split))
            .padding([8, 10])
            .style(move |_, status| theme::note_button(split_active, status)),
            button(lucide_icon("\u{E151}").size(18).color(MUTED))
                .on_press(Message::ToggleNoteSearch)
                .padding([8, 10])
                .style(theme::ghost_button),
            button(text("↻").size(17))
                .on_press(Message::RefreshRequested)
                .padding([5, 11])
                .style(theme::ghost_button),
        ]
        .spacing(8)
        .align_y(iced::Alignment::Center);

        let tags = row![
            text("#").size(15).color(INDIGO_LIGHT),
            text_input("tags separated by commas", &self.tags)
                .on_input(Message::TagsChanged)
                .padding([5, 8])
                .size(12)
                .style(theme::inline_input),
            text(
                self.active_note
                    .as_ref()
                    .map(|note| note.relative_path.as_str())
                    .unwrap_or_default(),
            )
            .size(10)
            .color(DIM),
        ]
        .spacing(8)
        .align_y(iced::Alignment::Center);

        let loading_workspace = || -> Element<'_, Message> {
            container(
                column![
                    lucide_icon("\u{E145}").size(25).color(INDIGO_LIGHT),
                    text(if self.workspace_mounting {
                        "Mounting large note editor…"
                    } else {
                        "Preparing large note editor…"
                    })
                    .size(14)
                    .color(TEXT),
                    text("The interface will stay available while the editor is prepared.")
                        .size(11)
                        .color(MUTED),
                ]
                .spacing(9)
                .align_x(iced::Alignment::Center),
            )
            .width(Fill)
            .height(Fill)
            .center(Fill)
            .style(|_| theme::panel(BACKGROUND, Color::TRANSPARENT, 0.0))
            .into()
        };
        let workspace_content = || -> Element<'_, Message> {
            match self.mode {
                WorkspaceMode::Editor => self.editor_view(),
                WorkspaceMode::Preview => self.preview_view(),
                WorkspaceMode::Split => row![self.editor_view(), self.preview_view()]
                    .spacing(1)
                    .width(Fill)
                    .height(Fill)
                    .into(),
            }
        };
        let workspace = match (self.workspace_loading, self.workspace_mounting) {
            (None, _) => workspace_content(),
            (Some(_), false) => loading_workspace(),
            (Some(_), true) => stack![workspace_content(), opaque(loading_workspace())]
                .width(Fill)
                .height(Fill)
                .into(),
        };

        let search_bar: Element<'_, Message> = if self.note_search_open {
            let result = if self.note_search_query.trim().is_empty() {
                String::new()
            } else if self.note_search_matches.is_empty() {
                "No matches".into()
            } else {
                format!(
                    "{} / {}",
                    self.note_search_index + 1,
                    self.note_search_matches.len()
                )
            };
            row![
                lucide_icon("\u{E151}").size(15).color(MUTED),
                text_input("Find in note…", &self.note_search_query)
                    .id(note_search_input_id())
                    .on_input(Message::NoteSearchChanged)
                    .on_submit(Message::NavigateNoteSearch(1))
                    .padding([7, 9])
                    .size(12)
                    .style(theme::inline_input),
                text(result).size(11).color(DIM),
                button(text("↑").size(14))
                    .on_press(Message::NavigateNoteSearch(-1))
                    .padding([6, 9])
                    .style(theme::ghost_button),
                button(text("↓").size(14))
                    .on_press(Message::NavigateNoteSearch(1))
                    .padding([6, 9])
                    .style(theme::ghost_button),
                button(text("×").size(15))
                    .on_press(Message::ToggleNoteSearch)
                    .padding([6, 9])
                    .style(theme::ghost_button),
            ]
            .spacing(7)
            .align_y(iced::Alignment::Center)
            .into()
        } else {
            Space::new().height(0).into()
        };

        container(
            column![header, tags, search_bar, workspace, self.status_bar()]
                .spacing(7)
                .padding(Padding {
                    top: 10.0,
                    right: 12.0,
                    bottom: 8.0,
                    left: 12.0,
                }),
        )
        .width(Fill)
        .height(Fill)
        .style(|_| theme::panel(BACKGROUND, Color::TRANSPARENT, 0.0))
        .into()
    }

    fn editor_view(&self) -> Element<'_, Message> {
        let editor = text_editor(&self.editor)
            .on_action(Message::EditorAction)
            .height(Fill)
            .font(CODE_FONT)
            .size(14)
            .line_height(1.6)
            .padding([20, 24])
            .highlight_with::<SearchHighlighter>(
                self.note_search_query.clone(),
                search_highlight_format,
            )
            .style(|_, _| text_editor::Style {
                background: Background::Color(BACKGROUND),
                border: Border {
                    color: Color::TRANSPARENT,
                    width: 0.0,
                    radius: 0.0.into(),
                },
                placeholder: DIM,
                value: TEXT,
                selection: Color::from_rgba(1.0, 0.78, 0.60, 0.42),
            });
        container(editor).width(Fill).height(Fill).into()
    }

    fn preview_view(&self) -> Element<'_, Message> {
        let document = self
            .preview_document
            .view(&self.note_search_query)
            .map(Message::MarkdownLinkClicked);
        let preview = scrollable(container(document).width(Fill).padding([22, 26]))
            .id(preview_scroll_id())
            .on_scroll(|viewport| Message::PreviewScrolled(viewport.relative_offset().y))
            .height(Fill);
        container(preview)
            .width(Fill)
            .height(Fill)
            .style(|_| theme::panel(BACKGROUND, Color::TRANSPARENT, 0.0))
            .into()
    }

    fn tree_context_menu(&self, target: &TreeContextTarget) -> Element<'_, Message> {
        let folder = match target {
            TreeContextTarget::Folder(folder) => folder.clone(),
            TreeContextTarget::Root => String::new(),
            TreeContextTarget::Note(id) => self
                .notes
                .iter()
                .find(|note| note.id == *id)
                .map(|note| note_folder(&note.relative_path))
                .unwrap_or_default(),
        };
        let title = match target {
            TreeContextTarget::Root => "Vault root".to_owned(),
            TreeContextTarget::Folder(folder) => folder.clone(),
            TreeContextTarget::Note(_) => "Note".to_owned(),
        };
        let delete: Element<'_, Message> = match target {
            TreeContextTarget::Folder(folder) => button(text("Delete folder").size(12).color(TEXT))
                .on_press(Message::DeleteTreeItem(TreeItem::Folder(folder.clone())))
                .width(Fill)
                .padding([8, 10])
                .style(theme::danger_button)
                .into(),
            TreeContextTarget::Note(id) => button(text("Delete note").size(12).color(TEXT))
                .on_press(Message::DeleteTreeItem(TreeItem::Note(*id)))
                .width(Fill)
                .padding([8, 10])
                .style(theme::danger_button)
                .into(),
            TreeContextTarget::Root => Space::new().height(0).into(),
        };
        container(
            column![
                text("Create or manage").size(14).color(TEXT),
                text(title).size(10).color(MUTED),
                button(text("New note").size(12).color(TEXT))
                    .on_press(Message::CreateNoteIn(folder.clone()))
                    .width(Fill)
                    .padding([8, 10])
                    .style(theme::ghost_button),
                button(text("New folder").size(12).color(TEXT))
                    .on_press(Message::OpenFolderCreator(folder))
                    .width(Fill)
                    .padding([8, 10])
                    .style(theme::ghost_button),
                delete,
            ]
            .spacing(6)
            .padding(10),
        )
        .width(190)
        .style(|_| theme::panel(SIDEBAR, BORDER, 9.0))
        .into()
    }

    fn tree_creator_input(&self, depth: usize, is_folder: bool) -> Element<'_, Message> {
        let value = if is_folder {
            &self.folder_name
        } else {
            &self.note_creator_title
        };
        let message = if is_folder {
            Message::FolderNameChanged
        } else {
            Message::NoteCreatorTitleChanged
        };
        let submit = if is_folder {
            Message::CreateFolderRequested
        } else {
            Message::ConfirmCreateNote
        };
        let placeholder = if is_folder {
            "New folder…"
        } else {
            "New note…"
        };
        container(
            row![
                Space::new().width(depth as f32 * TREE_INDENT + TREE_LEADING_WIDTH + 4.0),
                text_input(placeholder, value)
                    .id(if is_folder {
                        folder_input_id()
                    } else {
                        note_creator_title_id()
                    })
                    .on_input(message)
                    .on_submit(submit)
                    .padding([7, 9])
                    .size(12)
                    .style(theme::inline_input),
            ]
            .align_y(iced::Alignment::Center),
        )
        .padding([4, TREE_ROW_PADDING_X])
        .width(Fill)
        .into()
    }

    fn vault_manager(&self) -> Element<'_, Message> {
        let valid = !self.vault_path_draft.trim().is_empty();
        container(
            column![
                column![
                    text("Vault manager").size(19).color(TEXT),
                    text("Open an existing Markdown vault or create a new one.")
                        .size(12)
                        .color(MUTED),
                ]
                .spacing(4),
                column![
                    text("FOLDER PATH").size(10).color(DIM),
                    text_input("/home/you/Notes", &self.vault_path_draft)
                        .id(vault_path_input_id())
                        .on_input(Message::VaultPathChanged)
                        .on_submit(Message::OpenVaultRequested)
                        .padding([11, 12])
                        .size(13)
                        .style(theme::input),
                    text(
                        "Notiz stores Markdown files in this folder and keeps its index separately."
                    )
                    .size(11)
                    .color(DIM),
                ]
                .spacing(7),
                row![
                    button(text("Cancel").size(12))
                        .on_press(Message::CloseOverlay)
                        .padding([9, 13])
                        .style(theme::ghost_button),
                    Space::new().width(Fill),
                    button(text("Open vault").size(12))
                        .on_press_maybe(valid.then_some(Message::OpenVaultRequested))
                        .padding([9, 13])
                        .style(theme::ghost_button),
                    button(text("Create vault").size(12))
                        .on_press_maybe(valid.then_some(Message::CreateVaultRequested))
                        .padding([9, 14])
                        .style(theme::primary_button),
                ]
                .spacing(8)
                .align_y(iced::Alignment::Center),
            ]
            .spacing(18)
            .padding(18),
        )
        .width(560)
        .style(|_| theme::panel(SIDEBAR, BORDER, 12.0))
        .into()
    }

    fn command_palette(&self) -> Element<'_, Message> {
        let entries = self.palette_entries();
        let input = text_input("Type a command or note name…", &self.palette_query)
            .id(palette_input_id())
            .on_input(Message::PaletteQueryChanged)
            .on_submit(Message::PaletteSubmit)
            .padding([13, 15])
            .size(14)
            .style(theme::palette_input);
        let mut results = column![].spacing(4).width(Fill);
        if entries.is_empty() {
            results = results.push(
                container(text("No commands or notes found.").size(12).color(DIM)).padding(14),
            );
        } else {
            for (index, entry) in entries.into_iter().enumerate() {
                let active = index == self.palette_selected;
                results = results.push(
                    button(
                        row![
                            text(entry.label).size(13).color(TEXT),
                            Space::new().width(Fill),
                            text(entry.hint).size(10).color(DIM),
                        ]
                        .align_y(iced::Alignment::Center),
                    )
                    .on_press(Message::PaletteExecute(entry.action))
                    .padding([10, 12])
                    .width(Fill)
                    .style(move |_, status| theme::palette_item(active, status)),
                );
            }
        }
        container(
            column![
                row![
                    text("Command Palette").size(11).color(MUTED),
                    Space::new().width(Fill),
                    text("ESC to close").size(10).color(DIM),
                ],
                input,
                scrollable(results).height(390),
                text("↑↓ navigate  ·  Enter run  ·  Ctrl+K open/close")
                    .size(10)
                    .color(DIM),
            ]
            .spacing(10)
            .padding(14),
        )
        .width(620)
        .style(|_| theme::panel(SIDEBAR, BORDER, 12.0))
        .into()
    }

    fn palette_entries(&self) -> Vec<PaletteEntry> {
        let commands = [
            ("Create new note", "Ctrl+N", PaletteAction::CreateNote),
            (
                "Abrir diário de hoje",
                "Ctrl+Shift+D",
                PaletteAction::CreateFromTemplate(NoteTemplate::Daily),
            ),
            (
                "Nova sessão",
                "Sessions",
                PaletteAction::CreateFromTemplate(NoteTemplate::Session),
            ),
            (
                "Nova memória",
                "Memory",
                PaletteAction::CreateFromTemplate(NoteTemplate::Memory),
            ),
            ("Open note tree", "Sidebar", PaletteAction::OpenTree),
            ("Open graph", "New tab", PaletteAction::OpenGraph),
            (
                "Open backlinks",
                "Right panel",
                PaletteAction::ToggleInspector,
            ),
            (
                "Show editor",
                "View",
                PaletteAction::SetMode(WorkspaceMode::Editor),
            ),
            (
                "Show preview",
                "View",
                PaletteAction::SetMode(WorkspaceMode::Preview),
            ),
            (
                "Show editor and preview",
                "Split",
                PaletteAction::SetMode(WorkspaceMode::Split),
            ),
            ("Save note", "Ctrl+S", PaletteAction::Save),
            ("Refresh index", "Markdown", PaletteAction::Refresh),
        ];
        let query = self.palette_query.trim().to_lowercase();
        let mut entries = commands
            .into_iter()
            .filter(|(label, _, _)| query.is_empty() || label.to_lowercase().contains(&query))
            .map(|(label, hint, action)| PaletteEntry {
                label: label.into(),
                hint: hint.into(),
                action,
            })
            .collect::<Vec<_>>();
        entries.extend(
            self.notes
                .iter()
                .filter(|note| {
                    query.is_empty()
                        || note.title.to_lowercase().contains(&query)
                        || note.relative_path.to_lowercase().contains(&query)
                        || note.excerpt.to_lowercase().contains(&query)
                })
                .take(if query.is_empty() { 6 } else { 20 })
                .map(|note| PaletteEntry {
                    label: note.title.clone(),
                    hint: note.relative_path.clone(),
                    action: PaletteAction::OpenNote(note.id),
                }),
        );
        entries
    }

    fn status_bar(&self) -> Element<'_, Message> {
        let save_state = if self.saving {
            ("● saving", INDIGO_LIGHT)
        } else if self.dirty {
            ("● local changes", INDIGO_LIGHT)
        } else {
            ("● saved to Markdown", SUCCESS)
        };
        row![
            text(&self.status).size(11).color(MUTED),
            Space::new().width(Fill),
            text(save_state.0).size(11).color(save_state.1),
        ]
        .align_y(iced::Alignment::Center)
        .into()
    }

    fn apply_snapshot(&mut self, snapshot: AppSnapshot) {
        self.notes = snapshot.notes;
        self.folders = snapshot.folders;
        self.graph = snapshot.graph;
        self.vault_revision = snapshot.vault_revision;
        if self.search.trim().is_empty() {
            self.visible_notes = self.notes.clone();
        }
    }

    fn switch_vault_task(&mut self, create: bool) -> Task<Message> {
        let path = self.vault_path_draft.trim().to_owned();
        if path.is_empty() {
            return Task::none();
        }
        self.status = if create {
            "Creating vault…".into()
        } else {
            "Opening vault…".into()
        };
        Task::perform(
            async move {
                tokio::task::spawn_blocking(move || {
                    let services = AppServices::open_vault(path, create)?;
                    let snapshot = services.initialize()?;
                    Ok((services, snapshot))
                })
                .await
                .map_err(|error| error.to_string())?
                .map_err(|error: notiz_services::Error| error.to_string())
            },
            Message::VaultSwitched,
        )
    }

    fn reset_workspace_for_vault(&mut self, snapshot: AppSnapshot) {
        self.open_tabs.clear();
        self.active_tab = None;
        self.selected_folder.clear();
        self.selected_notes.clear();
        self.selected_folders.clear();
        self.hovered_note_selector = None;
        self.hovered_folder_selector = None;
        self.search.clear();
        self.tree_hold = None;
        self.tree_drag = None;
        self.tree_drop_target = None;
        self.tree_nest_target = None;
        self.context_menu = None;
        self.collapsed_folders.clear();
        self.tab_hold = None;
        self.tab_drag = None;
        self.tab_reorder = None;
        self.workspace_loading = None;
        self.workspace_mounting = false;
        self.clear_active_note();
        self.apply_snapshot(snapshot);
    }

    fn folder_row_visible(&self, folder: &str) -> bool {
        let mut parent = folder_parent(folder);
        while !parent.is_empty() {
            if self.collapsed_folders.contains(&parent) {
                return false;
            }
            parent = folder_parent(&parent);
        }
        true
    }

    fn ordered_visible_notes(&self) -> Vec<&NoteSummary> {
        self.ordered_note_refs(&self.visible_notes)
    }

    fn ordered_note_refs<'a>(&self, notes: &'a [NoteSummary]) -> Vec<&'a NoteSummary> {
        let mut ordered = notes.iter().collect::<Vec<_>>();
        match self.tree_sort {
            TreeSort::Alphabetical => ordered.sort_by(|left, right| {
                left.title
                    .to_lowercase()
                    .cmp(&right.title.to_lowercase())
                    .then_with(|| left.relative_path.cmp(&right.relative_path))
            }),
            TreeSort::Updated => ordered.sort_by(|left, right| {
                right
                    .updated_at
                    .cmp(&left.updated_at)
                    .then_with(|| left.title.to_lowercase().cmp(&right.title.to_lowercase()))
            }),
        }
        ordered
    }

    fn ordered_folders(&self) -> Vec<String> {
        fn visit(
            parent: &str,
            folders: &[String],
            notes: &[NoteSummary],
            sort: TreeSort,
            ordered: &mut Vec<String>,
        ) {
            let mut children = folders
                .iter()
                .filter(|folder| folder_parent(folder) == parent)
                .collect::<Vec<_>>();
            children.sort_by(|left, right| match sort {
                TreeSort::Updated => {
                    let updated = |folder: &str| {
                        notes
                            .iter()
                            .filter(|note| path_is_in_folder(&note.relative_path, folder))
                            .map(|note| note.updated_at)
                            .max()
                    };
                    updated(right)
                        .cmp(&updated(left))
                        .then_with(|| left.to_lowercase().cmp(&right.to_lowercase()))
                }
                TreeSort::Alphabetical => left.to_lowercase().cmp(&right.to_lowercase()),
            });
            for child in children {
                ordered.push(child.clone());
                visit(child, folders, notes, sort, ordered);
            }
        }

        let mut ordered = Vec::with_capacity(self.folders.len());
        visit("", &self.folders, &self.notes, self.tree_sort, &mut ordered);
        ordered
    }

    fn finish_tree_interaction(&mut self) -> Task<Message> {
        let held = self.tree_hold.take();
        let Some(drag) = self.tree_drag.take() else {
            return match held.map(|hold| hold.item) {
                Some(TreeItem::Note(id)) => self.update(Message::OpenNote(id)),
                Some(TreeItem::Folder(folder)) => {
                    self.selected_folder = folder.clone();
                    self.update(Message::ToggleFolderExpanded(folder))
                }
                None => Task::none(),
            };
        };
        let target = self.tree_drop_target.take();
        let nest_target = self.tree_nest_target.take();
        let horizontal = self.cursor_x - drag.origin_x;
        match drag.item {
            TreeItem::Note(id) => {
                let Some(source_folder) = self
                    .notes
                    .iter()
                    .find(|note| note.id == id)
                    .map(|note| note_folder(&note.relative_path))
                else {
                    return Task::none();
                };
                let destination = if horizontal < -26.0 {
                    folder_parent(&source_folder)
                } else {
                    target
                        .as_ref()
                        .and_then(|target| self.tree_item_folder(target))
                        .unwrap_or_else(|| source_folder.clone())
                };
                if destination == source_folder {
                    return Task::none();
                }
                let pending = self
                    .active_note
                    .clone()
                    .filter(|note| note.id == id && self.dirty)
                    .map(|note| {
                        (
                            note,
                            self.title.clone(),
                            self.editor.text(),
                            self.tags.clone(),
                        )
                    });
                let services = self.services.clone();
                self.status = if destination.is_empty() {
                    "Moving note to Root…".into()
                } else {
                    format!("Moving note to {destination}…")
                };
                Task::perform(
                    async move {
                        tokio::task::spawn_blocking(move || {
                            if let Some((note, title, body, tags)) = pending {
                                services.save_note(note, title, body, tags)?;
                            }
                            services.move_note(id, &destination)
                        })
                        .await
                        .map_err(|error| error.to_string())?
                        .map_err(|error| error.to_string())
                    },
                    move |result| Message::TreeNoteMoved(id, result),
                )
            }
            TreeItem::Folder(source) => {
                if source.is_empty() {
                    return Task::none();
                }
                let source_parent = folder_parent(&source);
                let valid_nest_target = nest_target.filter(|folder| {
                    !folder.is_empty()
                        && folder != &source
                        && !folder.starts_with(&format!("{source}/"))
                });
                let destination_parent = if let Some(folder) = valid_nest_target {
                    if horizontal < -26.0 {
                        folder_parent(&source_parent)
                    } else {
                        folder
                    }
                } else if horizontal < -26.0 {
                    folder_parent(&source_parent)
                } else {
                    match target.as_ref() {
                        Some(TreeItem::Folder(folder))
                            if !folder.is_empty()
                                && folder != &source
                                && !folder.starts_with(&format!("{source}/"))
                                && horizontal > 18.0 =>
                        {
                            folder.clone()
                        }
                        _ => source_parent.clone(),
                    }
                };
                if destination_parent == source_parent {
                    return Task::none();
                }
                let services = self.services.clone();
                let sent_source = source.clone();
                self.status = if destination_parent.is_empty() {
                    "Moving folder to Root…".into()
                } else {
                    format!("Moving folder into {destination_parent}…")
                };
                Task::perform(
                    async move {
                        tokio::task::spawn_blocking(move || {
                            services.move_folder(&source, &destination_parent)
                        })
                        .await
                        .map_err(|error| error.to_string())?
                        .map_err(|error| error.to_string())
                    },
                    move |result| Message::TreeFolderMoved(sent_source.clone(), result),
                )
            }
        }
    }

    fn tree_item_folder(&self, item: &TreeItem) -> Option<String> {
        match item {
            TreeItem::Folder(folder) => Some(folder.clone()),
            TreeItem::Note(id) => self
                .notes
                .iter()
                .find(|note| note.id == *id)
                .map(|note| note_folder(&note.relative_path)),
        }
    }

    fn set_tree_drop_target(&mut self, target: Option<TreeItem>) {
        if self.tree_drop_target != target {
            self.tree_drop_target = target;
        }
    }

    fn clear_active_note(&mut self) {
        self.active_note = None;
        self.editor = text_editor::Content::new();
        self.preview_blocks = build_preview_blocks("");
        self.preview_document = selectable_preview::Document::parse("*Empty note.*");
        self.title.clear();
        self.tags.clear();
        self.backlinks.clear();
        self.dirty = false;
        self.preview_dirty = false;
        self.active_tab = self.open_tabs.last().copied();
    }

    fn build_preview_task(&mut self) -> Task<Message> {
        let body = self.editor.text();
        let revision = self.revision;
        self.preview_building = true;
        self.preview_job_revision = revision;
        Task::perform(
            async move {
                tokio::task::spawn_blocking(move || {
                    let rendered = renderable_markdown(&body);
                    (
                        build_preview_sources(&body),
                        selectable_preview::Document::parse(&rendered),
                    )
                })
                .await
                .unwrap_or_else(|_| {
                    (
                        build_preview_sources(""),
                        selectable_preview::Document::parse("*Empty note.*"),
                    )
                })
            },
            move |(sources, document)| Message::PreviewPrepared(revision, sources, document),
        )
    }

    fn refresh_note_search(&mut self) {
        let query = self.note_search_query.trim().to_lowercase();
        self.note_search_matches.clear();
        self.note_search_index = 0;
        if query.is_empty() {
            return;
        }
        self.note_search_matches = self
            .editor
            .lines()
            .enumerate()
            .flat_map(|(line, value)| {
                searchable_markdown::case_insensitive_ranges(&value.text, &query)
                    .into_iter()
                    .map(move |range| {
                        let start = value.text[..range.start].chars().count();
                        let end = start + value.text[range].chars().count();
                        (line, start, end)
                    })
            })
            .collect();
    }

    fn navigate_note_search(&mut self, delta: i32) -> Task<Message> {
        if self.note_search_matches.is_empty() {
            return Task::none();
        }
        let len = self.note_search_matches.len();
        self.note_search_index = if delta < 0 {
            self.note_search_index.checked_sub(1).unwrap_or(len - 1)
        } else if delta > 0 {
            (self.note_search_index + 1) % len
        } else {
            self.note_search_index.min(len - 1)
        };
        let (line, start, end) = self.note_search_matches[self.note_search_index];
        self.selected_line = line;
        self.editor.move_to(text_editor::Cursor {
            position: text_editor::Position { line, column: end },
            selection: Some(text_editor::Position {
                line,
                column: start,
            }),
        });
        let line_count = self.editor.line_count().max(1);
        let ratio = line_ratio(line, line_count);
        let lines = ((ratio - self.editor_scroll_ratio) * line_count as f32).round() as i32;
        if lines != 0 {
            self.editor.perform(text_editor::Action::Scroll { lines });
        }
        self.editor_scroll_ratio = ratio;
        self.editor_scroll_target = ratio;
        self.preview_scroll_target = self.preview_ratio_for_line(line);
        Task::none()
    }

    fn open_tab(&mut self, tab: WorkspaceTab) {
        if !self.open_tabs.contains(&tab) {
            self.open_tabs.push(tab);
        }
        self.active_tab = Some(tab);
    }

    fn open_note_tab(&mut self, id: Uuid) {
        self.open_tab(WorkspaceTab::Note(id));
    }

    fn preview_ratio_for_line(&self, line: usize) -> f32 {
        let Some(index) = self
            .preview_blocks
            .iter()
            .position(|block| line >= block.start_line && line <= block.end_line)
        else {
            return line_ratio(line, self.editor.line_count());
        };
        let block = &self.preview_blocks[index];
        let span = block.end_line.saturating_sub(block.start_line).max(1);
        let within = line.saturating_sub(block.start_line) as f32 / span as f32;
        ((index as f32 + within) / self.preview_blocks.len().max(1) as f32).clamp(0.0, 1.0)
    }

    fn preview_line_for_ratio(&self, ratio: f32) -> usize {
        if self.preview_blocks.is_empty() {
            return 0;
        }
        let position = ratio.clamp(0.0, 1.0) * self.preview_blocks.len() as f32;
        let index = (position.floor() as usize).min(self.preview_blocks.len() - 1);
        let within = if index + 1 == self.preview_blocks.len() && ratio >= 1.0 {
            1.0
        } else {
            position.fract()
        };
        let block = &self.preview_blocks[index];
        let span = block.end_line.saturating_sub(block.start_line);
        block.start_line + (within * span as f32).round() as usize
    }

    fn apply_context(&mut self, context: NoteContext) {
        self.title = context.note.title.clone();
        self.tags = context.note.tags.join(", ");
        self.selected_folder = note_folder(&context.note.relative_path);
        self.editor = text_editor::Content::with_text(&context.note.body);
        self.preview_blocks = vec![PreviewBlock {
            start_line: 0,
            end_line: 0,
        }];
        self.preview_document = selectable_preview::Document::parse("*Rendering preview…*");
        self.backlinks = context.backlinks;
        self.active_note = Some(context.note);
        self.dirty = false;
        self.saving = false;
        self.last_edit = std::time::Instant::now().checked_sub(PREVIEW_DELAY);
        self.preview_dirty = true;
        self.preview_building = false;
        self.note_search_query.clear();
        self.note_search_matches.clear();
        self.selected_line = 0;
        self.editor_scroll_ratio = 0.0;
        self.editor_scroll_target = 0.0;
        self.preview_scroll_ratio = 0.0;
        self.preview_scroll_target = 0.0;
        self.preview_scroll_programmatic = false;
        self.revision = self.revision.wrapping_add(1);
    }

    fn mark_dirty(&mut self) {
        if self.active_note.is_some() {
            self.dirty = true;
            self.revision = self.revision.wrapping_add(1);
            self.last_edit = Some(std::time::Instant::now());
            self.status = "Unsaved changes".into();
        }
    }

    /// Keep the editor mounted until all edits are safely persisted; failed saves cancel navigation.
    fn save_before(&mut self, action: Message) -> Option<Task<Message>> {
        if !self.dirty && !self.saving {
            return None;
        }
        self.deferred_action = Some(Box::new(action));
        Some(self.save_task())
    }

    fn save_task(&mut self) -> Task<Message> {
        if !self.dirty || self.saving {
            return Task::none();
        }
        let Some(note) = self.active_note.clone() else {
            return Task::none();
        };
        let id = note.id;
        let revision = self.revision;
        let title = self.title.clone();
        let body = self.editor.text();
        let tags = self.tags.clone();
        let services = self.services.clone();
        self.saving = true;
        self.status = "Writing Markdown…".into();
        Task::perform(
            async move {
                tokio::task::spawn_blocking(move || services.save_note(note, title, body, tags))
                    .await
                    .map_err(|error| error.to_string())?
                    .map_err(|error| error.to_string())
            },
            move |result| Message::Saved(id, revision, result),
        )
    }

    fn load_note_task(&mut self, id: Uuid) -> Task<Message> {
        self.open_note_tab(id);
        self.busy = true;
        self.status = "Loading note…".into();
        let services = self.services.clone();
        Task::perform(
            async move {
                tokio::task::spawn_blocking(move || services.load_note(id))
                    .await
                    .map_err(|error| error.to_string())?
                    .map_err(|error| error.to_string())
            },
            move |result| Message::Loaded(id, result),
        )
    }

    fn subscription(&self) -> Subscription<Message> {
        let autosave = if (self.dirty && !self.saving) || self.preview_dirty {
            time::every(Duration::from_millis(200)).map(|_| Message::AutosaveTick)
        } else {
            Subscription::none()
        };
        let animation = if (self.sidebar_width - self.sidebar_target).abs() > 0.5
            || (self.inspector_width - self.inspector_target).abs() > 0.5
            || (self.preview_scroll_ratio - self.preview_scroll_target).abs() > 0.002
            || (self.tree_drag_progress - if self.tree_drag.is_some() { 1.0 } else { 0.0 }).abs()
                > 0.002
            || self.tab_reorder.is_some()
        {
            time::every(Duration::from_millis(16)).map(|_| Message::AnimationTick)
        } else {
            Subscription::none()
        };
        Subscription::batch([
            autosave,
            animation,
            time::every(Duration::from_secs(3)).map(|_| Message::ExternalTick),
            iced::event::listen_with(window_events),
        ])
    }

    fn theme(&self) -> Theme {
        Theme::custom(
            "Vesper",
            Palette {
                background: BACKGROUND,
                text: TEXT,
                primary: INDIGO,
                success: SUCCESS,
                warning: INDIGO,
                danger: DANGER,
            },
        )
    }
}

fn service_task<T, F>(
    services: AppServices,
    operation: impl FnOnce(AppServices) -> notiz_services::Result<T> + Send + 'static,
    message: F,
) -> Task<Message>
where
    T: Send + 'static,
    F: Fn(Result<T, String>) -> Message + Send + 'static,
{
    Task::perform(
        async move {
            tokio::task::spawn_blocking(move || operation(services))
                .await
                .map_err(|error| error.to_string())?
                .map_err(|error| error.to_string())
        },
        message,
    )
}

fn search_input_id() -> iced::widget::Id {
    "notiz-search".into()
}

fn folder_input_id() -> iced::widget::Id {
    "notiz-folder-name".into()
}

fn note_creator_title_id() -> iced::widget::Id {
    "notiz-note-creator-title".into()
}

fn note_search_input_id() -> iced::widget::Id {
    "notiz-note-search".into()
}

fn vault_path_input_id() -> iced::widget::Id {
    "notiz-vault-path".into()
}

fn note_folder(relative_path: &str) -> String {
    Path::new(relative_path)
        .parent()
        .and_then(|path| path.to_str())
        .unwrap_or_default()
        .replace('\\', "/")
}

fn path_is_in_folder(relative_path: &str, folder: &str) -> bool {
    let parent = note_folder(relative_path);
    parent == folder
        || parent
            .strip_prefix(folder)
            .is_some_and(|suffix| suffix.starts_with('/'))
}

fn folder_parent(folder: &str) -> String {
    Path::new(folder)
        .parent()
        .and_then(|path| path.to_str())
        .unwrap_or_default()
        .replace('\\', "/")
}

fn shorten(value: &str, max_chars: usize) -> String {
    let mut chars = value.chars();
    let shortened = chars.by_ref().take(max_chars).collect::<String>();
    if chars.next().is_some() {
        format!("{shortened}…")
    } else {
        shortened
    }
}

fn lucide_icon<'a>(codepoint: &'static str) -> iced::widget::Text<'a> {
    text(codepoint).font(Font::with_name("lucide"))
}

fn search_highlight_format(
    _highlight: &(),
    _theme: &Theme,
) -> iced::advanced::text::highlighter::Format<Font> {
    iced::advanced::text::highlighter::Format {
        color: Some(INDIGO),
        font: None,
    }
}

fn palette_input_id() -> iced::widget::Id {
    "notiz-command-palette".into()
}

fn preview_scroll_id() -> iced::widget::Id {
    "notiz-preview-scroll".into()
}

fn snap_preview(ratio: f32) -> Task<Message> {
    iced::widget::operation::snap_to(
        preview_scroll_id(),
        scrollable::RelativeOffset {
            x: 0.0,
            y: ratio.clamp(0.0, 1.0),
        },
    )
}

fn line_ratio(line: usize, line_count: usize) -> f32 {
    if line_count <= 1 {
        0.0
    } else {
        line.min(line_count - 1) as f32 / (line_count - 1) as f32
    }
}

fn animate_value(value: &mut f32, target: f32, speed: f32) {
    let distance = target - *value;
    if distance.abs() <= 0.5 {
        *value = target;
    } else {
        *value += distance * speed;
    }
}

fn animate_ratio(value: &mut f32, target: f32, speed: f32) {
    let distance = target - *value;
    if distance.abs() <= 0.002 {
        *value = target;
    } else {
        *value += distance * speed;
    }
}

fn build_preview_blocks(body: &str) -> Vec<PreviewBlock> {
    build_preview_sources(body)
        .into_iter()
        .map(preview_block_from_source)
        .collect()
}

fn preview_block_from_source(source: PreviewSource) -> PreviewBlock {
    PreviewBlock {
        start_line: source.start_line,
        end_line: source.end_line,
    }
}

fn build_preview_sources(body: &str) -> Vec<PreviewSource> {
    let lines = body.lines().collect::<Vec<_>>();
    if lines.is_empty() {
        return vec![PreviewSource {
            start_line: 0,
            end_line: 0,
            markdown: "*Empty note.*".into(),
        }];
    }

    let mut blocks = Vec::new();
    let mut source = String::new();
    let mut start_line = 0;
    let mut in_fence = false;
    for (line_index, line) in lines.iter().enumerate() {
        let trimmed = line.trim_start();
        let fence = trimmed.starts_with("```") || trimmed.starts_with("~~~");
        if !in_fence && line.trim().is_empty() {
            if !source.trim().is_empty() {
                blocks.push(PreviewSource {
                    start_line,
                    end_line: line_index.saturating_sub(1),
                    markdown: renderable_markdown(source.trim_end()),
                });
                source.clear();
            }
            start_line = line_index + 1;
            continue;
        }
        if source.is_empty() {
            start_line = line_index;
        }
        source.push_str(line);
        source.push('\n');
        if fence {
            in_fence = !in_fence;
        }
    }
    if !source.trim().is_empty() {
        blocks.push(PreviewSource {
            start_line,
            end_line: lines.len() - 1,
            markdown: renderable_markdown(source.trim_end()),
        });
    }
    if blocks.is_empty() {
        blocks.push(PreviewSource {
            start_line: 0,
            end_line: lines.len().saturating_sub(1),
            markdown: "*Empty note.*".into(),
        });
    }
    coalesce_preview_sources(blocks, 280)
}

fn coalesce_preview_sources(blocks: Vec<PreviewSource>, max_blocks: usize) -> Vec<PreviewSource> {
    if blocks.len() <= max_blocks {
        return blocks;
    }
    let chunk_size = blocks.len().div_ceil(max_blocks);
    blocks
        .chunks(chunk_size)
        .map(|chunk| PreviewSource {
            start_line: chunk.first().map_or(0, |block| block.start_line),
            end_line: chunk.last().map_or(0, |block| block.end_line),
            markdown: chunk
                .iter()
                .map(|block| block.markdown.as_str())
                .collect::<Vec<_>>()
                .join("\n\n"),
        })
        .collect()
}

fn internal_link_target(uri: &str) -> Option<String> {
    uri.strip_prefix("notiz://")
        .map(|target| target.replace("%20", " "))
}

fn window_events(
    event: iced::Event,
    status: iced::event::Status,
    window: iced::window::Id,
) -> Option<Message> {
    match event {
        iced::Event::Window(iced::window::Event::CloseRequested) => {
            Some(Message::CloseWindow(window))
        }
        iced::Event::Mouse(iced::mouse::Event::CursorMoved { position }) => {
            Some(Message::SidebarCursorMoved(position.x, position.y))
        }
        iced::Event::Mouse(iced::mouse::Event::ButtonReleased(iced::mouse::Button::Left)) => {
            Some(Message::SidebarResizeEnd)
        }
        iced::Event::Keyboard(iced::keyboard::Event::KeyPressed {
            key,
            modifiers,
            repeat: false,
            ..
        }) => {
            use iced::keyboard::{Key, key::Named};
            match key.as_ref() {
                Key::Character("d" | "D") if modifiers.control() && modifiers.shift() => {
                    Some(Message::CreateFromTemplate(NoteTemplate::Daily))
                }
                Key::Character("k") if modifiers.control() => Some(Message::TogglePalette),
                Key::Character("p") if modifiers.control() => Some(Message::TogglePalette),
                Key::Character("n")
                    if modifiers.control() && status == iced::event::Status::Ignored =>
                {
                    Some(Message::CreateNote)
                }
                Key::Character("s")
                    if modifiers.control() && status == iced::event::Status::Ignored =>
                {
                    Some(Message::SaveRequested)
                }
                Key::Character("f") if modifiers.control() => Some(Message::ToggleNoteSearch),
                Key::Named(Named::Escape) => Some(Message::CancelCreators),
                Key::Named(Named::ArrowUp) => Some(Message::PaletteMove(-1)),
                Key::Named(Named::ArrowDown) => Some(Message::PaletteMove(1)),
                _ => None,
            }
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestWorkspace {
        app: Notiz,
        directory: std::path::PathBuf,
    }

    impl TestWorkspace {
        fn new() -> Self {
            let directory = std::env::temp_dir().join(format!("notiz-ui-{}", Uuid::new_v4()));
            let services =
                AppServices::open(directory.join("vault"), directory.join("index.sqlite3"))
                    .unwrap();
            let first = services.create_note("First").unwrap();
            let second = services.create_note("Second").unwrap();
            let (mut app, _) = Notiz::new(services);
            app.apply_snapshot(second.snapshot);
            app.open_note_tab(first.context.note.id);
            app.apply_context(first.context);
            app.busy = false;
            Self { app, directory }
        }
    }

    impl Drop for TestWorkspace {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.directory);
        }
    }

    #[test]
    fn navigation_waits_for_save_and_failed_save_keeps_editor() {
        let mut workspace = TestWorkspace::new();
        let app = &mut workspace.app;
        let first = app.active_note.as_ref().unwrap().id;
        let second = app.notes.iter().find(|note| note.id != first).unwrap().id;
        let _ = app.update(Message::TitleChanged("Unsaved title".into()));
        let revision = app.revision;
        let _ = app.update(Message::OpenNote(second));
        assert!(app.saving);
        assert_eq!(app.active_tab, Some(WorkspaceTab::Note(first)));
        assert!(
            matches!(app.deferred_action.as_deref(), Some(Message::OpenNote(id)) if *id == second)
        );
        let _ = app.update(Message::Saved(first, revision, Err("disk full".into())));
        assert!(app.dirty);
        assert!(!app.saving);
        assert!(app.deferred_action.is_none());
        assert_eq!(app.title, "Unsaved title");
        assert_eq!(app.active_tab, Some(WorkspaceTab::Note(first)));
    }

    #[test]
    fn edits_during_save_are_saved_again_before_closing_tab() {
        let mut workspace = TestWorkspace::new();
        let app = &mut workspace.app;
        let note = app.active_note.clone().unwrap();
        let _ = app.update(Message::TitleChanged("First edit".into()));
        let revision = app.revision;
        let _ = app.update(Message::CloseTab(WorkspaceTab::Note(note.id)));
        assert!(app.open_tabs.contains(&WorkspaceTab::Note(note.id)));
        let mutation = app
            .services
            .save_note(
                note.clone(),
                app.title.clone(),
                app.editor.text(),
                app.tags.clone(),
            )
            .unwrap();
        let _ = app.update(Message::TitleChanged("Second edit".into()));
        let _ = app.update(Message::Saved(note.id, revision, Ok(mutation)));
        assert!(app.saving && app.dirty);
        assert!(app.deferred_action.is_some());
        assert!(app.open_tabs.contains(&WorkspaceTab::Note(note.id)));
        let revision = app.revision;
        let mutation = app
            .services
            .save_note(
                app.active_note.clone().unwrap(),
                app.title.clone(),
                app.editor.text(),
                app.tags.clone(),
            )
            .unwrap();
        let _ = app.update(Message::Saved(note.id, revision, Ok(mutation)));
        assert!(!app.dirty);
        assert!(!app.open_tabs.contains(&WorkspaceTab::Note(note.id)));
        assert_eq!(
            app.services.load_note(note.id).unwrap().note.title,
            "Second edit"
        );
    }

    #[test]
    fn home_and_window_close_wait_for_pending_edits() {
        for close_window in [false, true] {
            let mut workspace = TestWorkspace::new();
            let app = &mut workspace.app;
            let tab = app.active_tab;
            let _ = app.update(Message::TitleChanged("Keep this".into()));
            let action = if close_window {
                Message::CloseWindow(iced::window::Id::unique())
            } else {
                Message::OpenHome
            };
            let _ = app.update(action);
            assert!(app.saving);
            assert!(app.deferred_action.is_some());
            assert_eq!(app.active_tab, tab);
        }
    }

    #[test]
    fn returning_from_home_restores_the_existing_note_without_reloading() {
        let mut workspace = TestWorkspace::new();
        let app = &mut workspace.app;
        let id = app.active_note.as_ref().unwrap().id;
        let _ = app.update(Message::OpenHome);
        assert_eq!(app.active_tab, None);
        let _ = app.update(Message::OpenNote(id));
        assert_eq!(app.active_tab, Some(WorkspaceTab::Note(id)));
        assert!(!app.busy);
    }

    #[test]
    fn recognizes_internal_preview_links() {
        assert_eq!(
            internal_link_target("notiz://Projeto%20Atlas"),
            Some("Projeto Atlas".into())
        );
        assert_eq!(internal_link_target("https://example.com"), None);
    }

    #[test]
    fn huge_previews_are_coalesced_to_bound_widget_count() {
        let body = (0..2_000)
            .map(|index| format!("Paragraph {index}"))
            .collect::<Vec<_>>()
            .join("\n\n");
        let blocks = build_preview_sources(&body);
        assert!(blocks.len() <= 280);
        assert_eq!(blocks.first().map(|block| block.start_line), Some(0));
        assert_eq!(blocks.last().map(|block| block.end_line), Some(3_998));
    }
}
