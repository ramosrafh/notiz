use std::{
    collections::{HashMap, hash_map::DefaultHasher},
    hash::{Hash, Hasher},
    path::PathBuf,
};

use directories::ProjectDirs;
use notiz_core::{Backlink, KnowledgeGraph, Note, NoteSummary, normalize_tag, normalize_title};
use notiz_storage::Vault;
use uuid::Uuid;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Notiz could not determine its data directory")]
    DataDirectory,
    #[error(transparent)]
    Storage(#[from] notiz_storage::Error),
    #[error("vault folder does not exist: {0}")]
    VaultNotFound(PathBuf),
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoteTemplate {
    Daily,
    Session,
    Memory,
}

impl NoteTemplate {
    pub fn label(self) -> &'static str {
        match self {
            Self::Daily => "Diário",
            Self::Session => "Sessão",
            Self::Memory => "Memória",
        }
    }

    /// Uses the local calendar date for date-based titles.
    pub fn default_title(self) -> String {
        self.title_on(chrono::Local::now().date_naive())
    }

    fn title_on(self, date: chrono::NaiveDate) -> String {
        match self {
            Self::Daily => date.to_string(),
            Self::Session => format!("Sessão {date}"),
            Self::Memory => "Nova memória".into(),
        }
    }

    fn folder(self) -> &'static str {
        match self {
            Self::Daily => "Daily",
            Self::Session => "Sessions",
            Self::Memory => "Memory",
        }
    }

    fn body(self, title: &str) -> String {
        let sections = match self {
            Self::Daily => {
                "## Prioridades\n\n- [ ] \n\n## Registro do dia\n\n\n## Sessões e notas relacionadas\n\n\n## Aprendizados e próximos passos"
            }
            Self::Session => {
                "## Objetivo\n\n\n## Contexto e referências\n\n\n## Notas da sessão\n\n\n## Decisões\n\n\n## Próximos passos\n\n- [ ] \n\n## Memórias a preservar"
            }
            Self::Memory => {
                "## Conhecimento a preservar\n\n\n## Contexto\n\n\n## Fontes e notas relacionadas\n\n\n## Aplicação e revisão"
            }
        };
        format!("# {title}\n\n{sections}")
    }
}

#[derive(Debug, Clone)]
pub struct AppSnapshot {
    pub notes: Vec<NoteSummary>,
    pub folders: Vec<String>,
    pub graph: KnowledgeGraph,
    pub vault_revision: u64,
}

#[derive(Debug, Clone)]
pub struct NoteContext {
    pub note: Note,
    pub backlinks: Vec<Backlink>,
}

#[derive(Debug, Clone)]
pub struct MutationResult {
    pub context: NoteContext,
    pub snapshot: AppSnapshot,
}

#[derive(Debug, Clone)]
pub struct DeleteResult {
    pub snapshot: AppSnapshot,
    pub next: Option<NoteContext>,
}

#[derive(Debug, Clone)]
pub struct FolderMutationResult {
    pub folder: String,
    pub snapshot: AppSnapshot,
}

#[derive(Debug, Clone)]
pub struct AppServices {
    vault: Vault,
}

impl AppServices {
    pub fn open_default() -> Result<Self> {
        let project = ProjectDirs::from("dev", "notiz", "Notiz").ok_or(Error::DataDirectory)?;
        let vault_path = std::env::var_os("NOTIZ_VAULT")
            .map(PathBuf::from)
            .unwrap_or_else(|| project.data_dir().join("vault"));
        let index_path = project.cache_dir().join("notiz.sqlite3");
        Self::open(vault_path, index_path)
    }

    pub fn open(vault_path: impl Into<PathBuf>, index_path: impl Into<PathBuf>) -> Result<Self> {
        Ok(Self {
            vault: Vault::open(vault_path, index_path)?,
        })
    }

    pub fn open_vault(vault_path: impl Into<PathBuf>, create: bool) -> Result<Self> {
        let vault_path = vault_path.into();
        if !create && !vault_path.is_dir() {
            return Err(Error::VaultNotFound(vault_path));
        }
        let project = ProjectDirs::from("dev", "notiz", "Notiz").ok_or(Error::DataDirectory)?;
        let mut hasher = DefaultHasher::new();
        vault_path.hash(&mut hasher);
        let index_path = project
            .cache_dir()
            .join("vaults")
            .join(format!("{:016x}.sqlite3", hasher.finish()));
        Self::open(vault_path, index_path)
    }

    pub fn vault_path(&self) -> PathBuf {
        self.vault.root().to_owned()
    }

    pub fn initialize(&self) -> Result<AppSnapshot> {
        let mut notes = self.vault.reindex()?;
        if notes.is_empty() {
            let mut welcome = self.vault.create_note("Welcome to Notiz")?;
            welcome.tags = vec!["notiz".into(), "getting-started".into()];
            welcome.body = "# Welcome to Notiz\n\nYour notes are ordinary Markdown files.\n\nCreate relationships with links such as [[Another note]]. Backlinks and the knowledge graph update automatically.\n\nUse search to find titles, content, and tags."
                .into();
            self.vault.save_note(&mut welcome)?;
            notes = self.vault.list_notes()?;
        }
        self.current_snapshot_with_notes(notes)
    }

    pub fn refresh(&self) -> Result<AppSnapshot> {
        let notes = self.vault.reindex()?;
        self.current_snapshot_with_notes(notes)
    }

    pub fn create_note(&self, title: &str) -> Result<MutationResult> {
        self.create_note_in(title, "")
    }

    pub fn create_note_in(&self, title: &str, folder: &str) -> Result<MutationResult> {
        let note = self.vault.create_note_in(title, folder)?;
        Ok(MutationResult {
            context: NoteContext {
                note,
                backlinks: Vec::new(),
            },
            snapshot: self.current_snapshot()?,
        })
    }

    /// Creates a template note, or opens the existing daily note for today.
    pub fn create_note_from_template(&self, template: NoteTemplate) -> Result<MutationResult> {
        self.create_note_from_template_on(template, chrono::Local::now().date_naive())
    }

    fn create_note_from_template_on(
        &self,
        template: NoteTemplate,
        date: chrono::NaiveDate,
    ) -> Result<MutationResult> {
        let title = template.title_on(date);
        let date_tag = format!("date-{date}");
        if template == NoteTemplate::Daily {
            // Refresh the index so an existing file is reused even with a new cache.
            // The date tag keeps daily identity stable when its title is edited.
            let path = format!("Daily/{date}.md");
            let existing = self.vault.reindex()?.into_iter().find(|note| {
                note.relative_path == path
                    || (note
                        .relative_path
                        .rsplit_once('/')
                        .map(|(folder, _)| folder)
                        == Some("Daily")
                        && (note.title == title
                            || (note.tags.iter().any(|tag| tag == "daily")
                                && note.tags.contains(&date_tag))))
            });
            if let Some(note) = existing {
                return Ok(MutationResult {
                    context: self.load_note(note.id)?,
                    snapshot: self.current_snapshot()?,
                });
            }
        }

        let mut note = self.vault.create_note_in(&title, template.folder())?;
        note.body = template.body(&title);
        note.tags = vec![match template {
            NoteTemplate::Daily => "daily".into(),
            NoteTemplate::Session => "session".into(),
            NoteTemplate::Memory => "memory".into(),
        }];
        if template == NoteTemplate::Daily {
            note.tags.push(date_tag);
        }
        self.vault.save_note(&mut note)?;
        let backlinks = self.vault.backlinks(note.id)?;
        Ok(MutationResult {
            context: NoteContext { note, backlinks },
            snapshot: self.current_snapshot()?,
        })
    }

    pub fn create_folder(&self, folder: &str) -> Result<AppSnapshot> {
        self.vault.create_folder(folder)?;
        self.current_snapshot()
    }

    pub fn save_folder_order(&self, folders: &[String]) -> Result<AppSnapshot> {
        self.vault.save_folder_order(folders)?;
        self.current_snapshot()
    }

    pub fn save_tree_order(&self, folders: &[String], note_ids: &[Uuid]) -> Result<AppSnapshot> {
        self.vault.save_folder_order(folders)?;
        self.vault.save_note_order(note_ids)?;
        self.current_snapshot()
    }

    pub fn move_folder(&self, source: &str, parent: &str) -> Result<FolderMutationResult> {
        let folder = self.vault.move_folder(source, parent)?;
        Ok(FolderMutationResult {
            folder,
            snapshot: self.current_snapshot()?,
        })
    }

    pub fn move_note(&self, id: Uuid, folder: &str) -> Result<MutationResult> {
        let note = self.vault.move_note(id, folder)?;
        let backlinks = self.vault.backlinks(id)?;
        Ok(MutationResult {
            context: NoteContext { note, backlinks },
            snapshot: self.current_snapshot()?,
        })
    }

    pub fn load_note(&self, id: Uuid) -> Result<NoteContext> {
        Ok(NoteContext {
            note: self.vault.load_note(id)?,
            backlinks: self.vault.backlinks(id)?,
        })
    }

    pub fn save_note(
        &self,
        mut note: Note,
        title: String,
        body: String,
        tags: String,
    ) -> Result<MutationResult> {
        note.title = normalize_title(&title);
        note.body = body;
        note.tags = parse_tags(&tags);
        note.updated_at = chrono::Utc::now();
        self.vault.save_note(&mut note)?;
        let backlinks = self.vault.backlinks(note.id)?;
        Ok(MutationResult {
            context: NoteContext { note, backlinks },
            snapshot: self.current_snapshot()?,
        })
    }

    pub fn delete_note(&self, id: Uuid) -> Result<DeleteResult> {
        self.delete_notes(&[id])
    }

    pub fn delete_notes(&self, ids: &[Uuid]) -> Result<DeleteResult> {
        self.vault.delete_notes(ids)?;
        self.delete_result()
    }

    pub fn delete_items(&self, ids: &[Uuid], folders: &[String]) -> Result<DeleteResult> {
        self.vault.delete_folders(folders)?;
        self.vault.delete_notes(ids)?;
        self.delete_result()
    }

    fn delete_result(&self) -> Result<DeleteResult> {
        let snapshot = self.current_snapshot()?;
        let next = snapshot
            .notes
            .first()
            .map(|note| self.load_note(note.id))
            .transpose()?;
        Ok(DeleteResult { snapshot, next })
    }

    pub fn search(&self, query: &str) -> Result<Vec<NoteSummary>> {
        Ok(self.vault.search(query, 100)?)
    }

    pub fn resolve_link(&self, target: &str) -> Result<Option<Uuid>> {
        Ok(self.vault.resolve_target(target)?)
    }

    pub fn vault_revision(&self) -> Result<u64> {
        Ok(self.vault.revision()?)
    }

    fn current_snapshot(&self) -> Result<AppSnapshot> {
        self.current_snapshot_with_notes(self.vault.list_notes()?)
    }

    fn current_snapshot_with_notes(&self, notes: Vec<NoteSummary>) -> Result<AppSnapshot> {
        Ok(AppSnapshot {
            notes,
            folders: self.vault.folders()?,
            graph: self.layout_and_persist_graph()?,
            vault_revision: self.vault.revision()?,
        })
    }

    fn layout_and_persist_graph(&self) -> Result<KnowledgeGraph> {
        let mut graph = self.vault.graph()?;
        if graph
            .nodes
            .iter()
            .any(|node| node.x == 0.0 && node.y == 0.0)
        {
            force_layout(&mut graph);
            self.vault.save_graph_positions(&graph)?;
        }
        Ok(graph)
    }
}

fn parse_tags(tags: &str) -> Vec<String> {
    let mut parsed = Vec::new();
    for tag in tags.split([',', ' ', '\n']) {
        let tag = normalize_tag(tag);
        if !tag.is_empty() && !parsed.contains(&tag) {
            parsed.push(tag);
        }
    }
    parsed
}

fn force_layout(graph: &mut KnowledgeGraph) {
    let count = graph.nodes.len();
    if count == 0 {
        return;
    }
    if count == 1 {
        graph.nodes[0].x = 0.0;
        graph.nodes[0].y = 0.0;
        return;
    }

    let tau = std::f32::consts::TAU;
    for (index, node) in graph.nodes.iter_mut().enumerate() {
        let angle = tau * index as f32 / count as f32;
        let ring = 0.52 + (index % 5) as f32 * 0.045;
        node.x = angle.cos() * ring;
        node.y = angle.sin() * ring;
    }
    if count > 450 {
        return;
    }

    let indices: HashMap<Uuid, usize> = graph
        .nodes
        .iter()
        .enumerate()
        .map(|(index, node)| (node.id, index))
        .collect();
    let edges = graph
        .edges
        .iter()
        .filter_map(|edge| Some((*indices.get(&edge.source)?, *indices.get(&edge.target?)?)))
        .collect::<Vec<_>>();
    let area = 4.0f32;
    let ideal = (area / count as f32).sqrt().clamp(0.06, 0.45);

    for iteration in 0..90 {
        let mut movement = vec![(0.0f32, 0.0f32); count];
        for left in 0..count {
            for right in left + 1..count {
                let dx = graph.nodes[left].x - graph.nodes[right].x;
                let dy = graph.nodes[left].y - graph.nodes[right].y;
                let distance = (dx * dx + dy * dy).sqrt().max(0.015);
                let force = ideal * ideal / distance;
                let move_x = dx / distance * force;
                let move_y = dy / distance * force;
                movement[left].0 += move_x;
                movement[left].1 += move_y;
                movement[right].0 -= move_x;
                movement[right].1 -= move_y;
            }
        }
        for &(source, target) in &edges {
            let dx = graph.nodes[source].x - graph.nodes[target].x;
            let dy = graph.nodes[source].y - graph.nodes[target].y;
            let distance = (dx * dx + dy * dy).sqrt().max(0.015);
            let force = distance * distance / ideal;
            let move_x = dx / distance * force;
            let move_y = dy / distance * force;
            movement[source].0 -= move_x;
            movement[source].1 -= move_y;
            movement[target].0 += move_x;
            movement[target].1 += move_y;
        }
        let temperature = 0.09 * (1.0 - iteration as f32 / 90.0) + 0.008;
        for (node, (dx, dy)) in graph.nodes.iter_mut().zip(movement) {
            let magnitude = (dx * dx + dy * dy).sqrt().max(0.001);
            node.x = (node.x + dx / magnitude * magnitude.min(temperature)).clamp(-1.0, 1.0);
            node.y = (node.y + dy / magnitude * magnitude.min(temperature)).clamp(-1.0, 1.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use notiz_core::{GraphEdge, GraphNode};

    struct TestVault(PathBuf);

    impl TestVault {
        fn new() -> Self {
            Self(std::env::temp_dir().join(format!("notiz-services-{}", Uuid::new_v4())))
        }

        fn open(&self, index: &str) -> AppServices {
            AppServices::open(self.0.join("vault"), self.0.join(index)).unwrap()
        }
    }

    impl Drop for TestVault {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn template_labels_and_default_titles_are_useful() {
        let date = chrono::NaiveDate::from_ymd_opt(2026, 10, 27).unwrap();
        assert_eq!(NoteTemplate::Daily.label(), "Diário");
        assert_eq!(NoteTemplate::Session.label(), "Sessão");
        assert_eq!(NoteTemplate::Memory.label(), "Memória");
        assert_eq!(NoteTemplate::Daily.title_on(date), "2026-10-27");
        assert_eq!(NoteTemplate::Session.title_on(date), "Sessão 2026-10-27");
        assert_eq!(NoteTemplate::Memory.default_title(), "Nova memória");
    }

    #[test]
    fn templates_persist_markdown_tags_and_folders() {
        let directory = TestVault::new();
        let services = directory.open("index.sqlite3");
        for (template, folder, tag, section) in [
            (NoteTemplate::Daily, "Daily", "daily", "## Prioridades"),
            (NoteTemplate::Session, "Sessions", "session", "## Decisões"),
            (
                NoteTemplate::Memory,
                "Memory",
                "memory",
                "## Fontes e notas relacionadas",
            ),
        ] {
            let result = services.create_note_from_template(template).unwrap();
            let note = &result.context.note;
            assert!(note.relative_path.starts_with(&format!("{folder}/")));
            assert!(note.body.starts_with(&format!("# {}\n\n", note.title)));
            assert!(note.body.contains(section));
            assert!(note.tags.iter().any(|value| value == tag));
            assert!(result.snapshot.folders.contains(&folder.to_owned()));
            assert!(
                result
                    .snapshot
                    .notes
                    .iter()
                    .any(|entry| entry.id == note.id)
            );
            assert_eq!(services.load_note(note.id).unwrap().note, *note);
            assert!(
                services
                    .search(tag)
                    .unwrap()
                    .iter()
                    .any(|entry| entry.id == note.id)
            );
        }
        assert_eq!(services.refresh().unwrap().notes.len(), 3);
    }

    #[test]
    fn daily_reopens_without_overwriting_edits_even_with_a_new_index() {
        let directory = TestVault::new();
        let services = directory.open("index.sqlite3");
        let date = chrono::NaiveDate::from_ymd_opt(2026, 10, 27).unwrap();
        let first = services
            .create_note_from_template_on(NoteTemplate::Daily, date)
            .unwrap();
        let note = first.context.note;
        let saved = services
            .save_note(
                note.clone(),
                note.title.clone(),
                "# Registro\n\nConteúdo escrito pelo usuário.".into(),
                "daily pessoal".into(),
            )
            .unwrap()
            .context
            .note;
        let path = services.vault_path().join(&saved.relative_path);
        let original_file = std::fs::read(&path).unwrap();
        // Also verify that opening returns backlinks rather than a creation-only context.
        let source = services
            .create_note("Referência ao diário")
            .unwrap()
            .context
            .note;
        services
            .save_note(
                source,
                "Referência ao diário".into(),
                format!("[[{}]]", saved.title),
                String::new(),
            )
            .unwrap();
        let reopened_services = directory.open("new-index.sqlite3");
        let reopened = reopened_services
            .create_note_from_template_on(NoteTemplate::Daily, date)
            .unwrap();
        assert_eq!(reopened.context.note, saved);
        assert_eq!(reopened.context.backlinks.len(), 1);
        assert_eq!(reopened.snapshot.notes.len(), 2);
        assert_eq!(std::fs::read(path).unwrap(), original_file);
        assert!(!services.vault_path().join("Daily/2026-10-27-2.md").exists());
    }

    #[test]
    fn daily_reuses_a_renamed_note_and_keeps_different_days_separate() {
        let directory = TestVault::new();
        let services = directory.open("index.sqlite3");
        let date = chrono::NaiveDate::from_ymd_opt(2026, 10, 27).unwrap();
        let note = services
            .create_note_from_template_on(NoteTemplate::Daily, date)
            .unwrap()
            .context
            .note;
        let saved = services
            .save_note(
                note.clone(),
                "Meu dia".into(),
                note.body.clone(),
                note.tags.join(","),
            )
            .unwrap()
            .context
            .note;
        let reopened = services
            .create_note_from_template_on(NoteTemplate::Daily, date)
            .unwrap();
        assert_eq!(reopened.context.note, saved);
        assert_eq!(reopened.snapshot.notes.len(), 1);
        let tomorrow = services
            .create_note_from_template_on(NoteTemplate::Daily, date.succ_opt().unwrap())
            .unwrap();
        assert_ne!(tomorrow.context.note.id, saved.id);
        assert_eq!(tomorrow.context.note.title, "2026-10-28");
        assert_eq!(tomorrow.snapshot.notes.len(), 2);
    }

    #[test]
    fn session_and_memory_templates_create_distinct_notes() {
        let directory = TestVault::new();
        let services = directory.open("index.sqlite3");
        for template in [NoteTemplate::Session, NoteTemplate::Memory] {
            let first = services
                .create_note_from_template(template)
                .unwrap()
                .context
                .note;
            let second = services
                .create_note_from_template(template)
                .unwrap()
                .context
                .note;
            assert_ne!(first.id, second.id);
            assert_ne!(first.relative_path, second.relative_path);
            assert_eq!(services.load_note(first.id).unwrap().note, first);
        }
        assert_eq!(services.refresh().unwrap().notes.len(), 4);
    }

    #[test]
    fn tags_are_normalized_and_deduplicated() {
        assert_eq!(
            parse_tags("#Trabalho, rust trabalho"),
            vec!["trabalho", "rust"]
        );
    }

    #[test]
    fn force_layout_produces_finite_positions() {
        let left = Uuid::new_v4();
        let right = Uuid::new_v4();
        let mut graph = KnowledgeGraph {
            nodes: vec![node(left), node(right)],
            edges: vec![GraphEdge {
                source: left,
                target: Some(right),
                unresolved_target: None,
            }],
        };
        force_layout(&mut graph);
        assert!(
            graph
                .nodes
                .iter()
                .all(|node| node.x.is_finite() && node.y.is_finite())
        );
        assert_ne!(graph.nodes[0].x, graph.nodes[1].x);
    }

    fn node(id: Uuid) -> GraphNode {
        GraphNode {
            id,
            title: "Note".into(),
            relative_path: "note.md".into(),
            tags: Vec::new(),
            x: 0.0,
            y: 0.0,
            degree: 1,
        }
    }
}
