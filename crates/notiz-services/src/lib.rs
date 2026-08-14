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
