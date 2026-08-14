use std::{
    collections::HashMap,
    fs,
    hash::{Hash, Hasher},
    io::Write,
    path::{Component, Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use chrono::Utc;
use notiz_core::{
    Backlink, GraphEdge, GraphNode, KnowledgeGraph, Note, NoteSummary, normalize_title, slugify,
};
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use uuid::Uuid;

const TRASH_DIRECTORY: &str = ".notiz-trash";

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("could not create the Notiz vault: {0}")]
    CreateVault(#[source] std::io::Error),
    #[error("vault file operation failed: {0}")]
    File(#[from] std::io::Error),
    #[error("Notiz index failed: {0}")]
    Index(#[from] rusqlite::Error),
    #[error("invalid note id stored in the index: {0}")]
    InvalidId(#[from] uuid::Error),
    #[error("note does not exist: {0}")]
    NoteNotFound(Uuid),
    #[error("unsafe note path: {0}")]
    UnsafePath(String),
    #[error("a file or folder already exists at: {0}")]
    PathExists(String),
    #[error("note path is not valid UTF-8")]
    InvalidPath,
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Clone)]
pub struct Vault {
    root: Arc<PathBuf>,
    index_path: Arc<PathBuf>,
}

impl Vault {
    pub fn open(root: impl Into<PathBuf>, index_path: impl Into<PathBuf>) -> Result<Self> {
        let root = root.into();
        let index_path = index_path.into();
        fs::create_dir_all(&root).map_err(Error::CreateVault)?;
        if let Some(parent) = index_path.parent() {
            fs::create_dir_all(parent).map_err(Error::CreateVault)?;
        }
        let vault = Self {
            root: Arc::new(root),
            index_path: Arc::new(index_path),
        };
        vault.initialize_index()?;
        Ok(vault)
    }

    pub fn root(&self) -> &Path {
        self.root.as_ref()
    }

    pub fn index_path(&self) -> &Path {
        self.index_path.as_ref()
    }

    pub fn revision(&self) -> Result<u64> {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        for path in markdown_files(self.root())? {
            path.hash(&mut hasher);
            let metadata = fs::metadata(&path)?;
            metadata.len().hash(&mut hasher);
            if let Ok(modified) = metadata.modified()
                && let Ok(duration) = modified.duration_since(std::time::UNIX_EPOCH)
            {
                duration.as_nanos().hash(&mut hasher);
            }
        }
        Ok(hasher.finish())
    }

    pub fn reindex(&self) -> Result<Vec<NoteSummary>> {
        let mut notes = Vec::new();
        for path in markdown_files(self.root())? {
            let relative_path = path
                .strip_prefix(self.root())
                .map_err(|_| Error::UnsafePath(path.display().to_string()))?;
            let relative_path = path_text(relative_path)?;
            let markdown = fs::read_to_string(&path)?;
            let parsed = Note::from_markdown(relative_path, &markdown);
            let note = parsed.note;
            if !parsed.had_frontmatter {
                self.write_note_file(&note)?;
            }
            notes.push(note);
        }
        notes.sort_by(|left, right| {
            left.relative_path
                .to_lowercase()
                .cmp(&right.relative_path.to_lowercase())
        });

        let mut connection = self.connect()?;
        let transaction = connection.transaction()?;
        transaction.execute("DELETE FROM links", [])?;
        transaction.execute("DELETE FROM tags", [])?;
        transaction.execute("DELETE FROM notes", [])?;
        transaction.execute("DELETE FROM notes_fts", [])?;
        for note in &notes {
            insert_note(&transaction, note)?;
        }
        resolve_and_insert_links(&transaction, &notes)?;
        transaction.commit()?;

        Ok(notes.iter().map(NoteSummary::from).collect())
    }

    pub fn create_note(&self, title: &str) -> Result<Note> {
        self.create_note_in(title, "")
    }

    pub fn create_note_in(&self, title: &str, folder: &str) -> Result<Note> {
        let folder = validate_folder_path(folder)?;
        fs::create_dir_all(self.root.join(&folder))?;
        let title = normalize_title(title);
        let stem = slugify(&title);
        let mut candidate = folder.join(format!("{stem}.md"));
        let mut suffix = 2;
        while self.root.join(&candidate).exists() {
            candidate = folder.join(format!("{stem}-{suffix}.md"));
            suffix += 1;
        }
        let mut note = Note::new(title, path_text(&candidate)?);
        self.save_note(&mut note)?;
        Ok(note)
    }

    pub fn create_folder(&self, folder: &str) -> Result<()> {
        let folder = validate_folder_path(folder)?;
        if folder.as_os_str().is_empty() {
            return Err(Error::UnsafePath(folder.display().to_string()));
        }
        fs::create_dir_all(self.root.join(folder))?;
        Ok(())
    }

    pub fn folders(&self) -> Result<Vec<String>> {
        fn visit(root: &Path, directory: &Path, folders: &mut Vec<String>) -> Result<()> {
            for entry in fs::read_dir(directory)? {
                let path = entry?.path();
                if !path.is_dir()
                    || path.file_name().and_then(|name| name.to_str()) == Some(TRASH_DIRECTORY)
                {
                    continue;
                }
                let relative = path
                    .strip_prefix(root)
                    .map_err(|_| Error::UnsafePath(path.display().to_string()))?;
                folders.push(path_text(relative)?);
                visit(root, &path, folders)?;
            }
            Ok(())
        }
        let mut folders = Vec::new();
        visit(self.root(), self.root(), &mut folders)?;
        let connection = self.connect()?;
        let positions = {
            let mut statement = connection.prepare("SELECT path, position FROM folder_order")?;
            statement
                .query_map([], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
                })?
                .collect::<rusqlite::Result<HashMap<_, _>>>()?
        };
        folders.sort_by(|left, right| {
            positions
                .get(left)
                .copied()
                .unwrap_or(i64::MAX)
                .cmp(&positions.get(right).copied().unwrap_or(i64::MAX))
                .then_with(|| left.to_lowercase().cmp(&right.to_lowercase()))
        });
        Ok(folders)
    }

    pub fn save_folder_order(&self, folders: &[String]) -> Result<()> {
        let mut connection = self.connect()?;
        let transaction = connection.transaction()?;
        transaction.execute("DELETE FROM folder_order", [])?;
        for (position, folder) in folders.iter().enumerate() {
            transaction.execute(
                "INSERT INTO folder_order (path, position) VALUES (?1, ?2)",
                params![folder, position as i64],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }

    pub fn save_note_order(&self, note_ids: &[Uuid]) -> Result<()> {
        let mut connection = self.connect()?;
        let transaction = connection.transaction()?;
        transaction.execute("DELETE FROM note_order", [])?;
        for (position, id) in note_ids.iter().enumerate() {
            transaction.execute(
                "INSERT INTO note_order (note_id, position) VALUES (?1, ?2)",
                params![id.to_string(), position as i64],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }

    pub fn move_folder(&self, source: &str, destination_parent: &str) -> Result<String> {
        let source = validate_folder_path(source)?;
        let destination_parent = validate_folder_path(destination_parent)?;
        if source.as_os_str().is_empty()
            || destination_parent == source
            || destination_parent.starts_with(&source)
        {
            return Err(Error::UnsafePath(source.display().to_string()));
        }
        let name = source.file_name().ok_or(Error::InvalidPath)?;
        let destination = destination_parent.join(name);
        let source_absolute = self.root.join(&source);
        let destination_absolute = self.root.join(&destination);
        if destination_absolute.exists() {
            return Err(Error::PathExists(destination.display().to_string()));
        }
        fs::create_dir_all(self.root.join(&destination_parent))?;
        fs::rename(&source_absolute, &destination_absolute)?;

        let source_text = path_text(&source)?;
        let destination_text = path_text(&destination)?;
        let mut connection = self.connect()?;
        let transaction = connection.transaction()?;
        let indexed = {
            let mut statement = transaction.prepare("SELECT id, path FROM notes")?;
            statement
                .query_map([], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?
        };
        let moved = indexed
            .into_iter()
            .filter(|(_, path)| {
                path == &source_text || path.starts_with(&format!("{source_text}/"))
            })
            .collect::<Vec<_>>();
        for (id, old_path) in &moved {
            let suffix = old_path.strip_prefix(&source_text).unwrap_or_default();
            let new_path = format!("{destination_text}{suffix}");
            let markdown = fs::read_to_string(self.root.join(&new_path))?;
            let note = Note::from_markdown(new_path, &markdown).note;
            transaction.execute("DELETE FROM notes_fts WHERE note_id = ?1", [id])?;
            transaction.execute("DELETE FROM notes WHERE id = ?1", [id])?;
            insert_note(&transaction, &note)?;
        }
        let folder_positions = {
            let mut statement = transaction.prepare("SELECT path, position FROM folder_order")?;
            statement
                .query_map([], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?
        };
        transaction.execute("DELETE FROM folder_order", [])?;
        for (folder, position) in folder_positions {
            let folder = if folder == source_text || folder.starts_with(&format!("{source_text}/"))
            {
                format!(
                    "{destination_text}{}",
                    folder.strip_prefix(&source_text).unwrap_or_default()
                )
            } else {
                folder
            };
            transaction.execute(
                "INSERT INTO folder_order (path, position) VALUES (?1, ?2)",
                params![folder, position],
            )?;
        }
        rebuild_links_from_index(&transaction)?;
        transaction.commit()?;
        Ok(destination_text)
    }

    pub fn move_note(&self, id: Uuid, folder: &str) -> Result<Note> {
        let mut note = self.load_note(id)?;
        let folder = validate_folder_path(folder)?;
        let current = self.root.join(&note.relative_path);
        let current_parent = Path::new(&note.relative_path)
            .parent()
            .unwrap_or_else(|| Path::new(""));
        if current_parent == folder {
            return Ok(note);
        }
        fs::create_dir_all(self.root.join(&folder))?;
        let stem = current
            .file_stem()
            .and_then(|stem| stem.to_str())
            .ok_or(Error::InvalidPath)?;
        let mut relative = folder.join(format!("{stem}.md"));
        let mut suffix = 2;
        while self.root.join(&relative).exists() {
            relative = folder.join(format!("{stem}-{suffix}.md"));
            suffix += 1;
        }
        fs::rename(&current, self.root.join(&relative))?;
        note.relative_path = path_text(&relative)?;
        note.updated_at = Utc::now();
        self.write_note_file(&note)?;
        self.index_note(&note)?;
        Ok(note)
    }

    pub fn save_note(&self, note: &mut Note) -> Result<()> {
        validate_relative_path(&note.relative_path)?;
        self.rename_note_file_for_title(note)?;
        self.write_note_file(note)?;
        self.index_note(note)?;
        Ok(())
    }

    pub fn load_note(&self, id: Uuid) -> Result<Note> {
        let connection = self.connect()?;
        let relative_path = connection
            .query_row(
                "SELECT path FROM notes WHERE id = ?1",
                [id.to_string()],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .ok_or(Error::NoteNotFound(id))?;
        let markdown = fs::read_to_string(self.root.join(&relative_path))?;
        Ok(Note::from_markdown(relative_path, &markdown).note)
    }

    pub fn list_notes(&self) -> Result<Vec<NoteSummary>> {
        let connection = self.connect()?;
        let mut statement = connection.prepare(
            "SELECT n.id, n.title, n.path, n.tags_text, n.updated_at, n.body
             FROM notes n LEFT JOIN note_order o ON o.note_id = n.id
             ORDER BY COALESCE(o.position, 9223372036854775807), n.path COLLATE NOCASE",
        )?;
        let rows = statement.query_map([], summary_from_row)?;
        rows.map(|row| row.map_err(Error::from)).collect()
    }

    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<NoteSummary>> {
        let query = query.trim();
        if query.is_empty() {
            return self.list_notes();
        }
        let fts_query = query
            .split_whitespace()
            .map(|term| format!("\"{}\"*", term.replace('"', "\"\"")))
            .collect::<Vec<_>>()
            .join(" AND ");
        let connection = self.connect()?;
        let mut statement = connection.prepare(
            "SELECT n.id, n.title, n.path, n.tags_text, n.updated_at, n.body
             FROM notes_fts f JOIN notes n ON n.id = f.note_id
             WHERE notes_fts MATCH ?1
             ORDER BY bm25(notes_fts, 0.0, 8.0, 2.0, 1.0, 1.0), n.updated_at DESC
             LIMIT ?2",
        )?;
        let rows = statement.query_map(params![fts_query, limit as i64], summary_from_row)?;
        rows.map(|row| row.map_err(Error::from)).collect()
    }

    pub fn backlinks(&self, note_id: Uuid) -> Result<Vec<Backlink>> {
        let connection = self.connect()?;
        let mut statement = connection.prepare(
            "SELECT n.id, n.title, n.path
             FROM links l JOIN notes n ON n.id = l.source_id
             WHERE l.target_id = ?1 ORDER BY n.title COLLATE NOCASE",
        )?;
        let rows = statement.query_map([note_id.to_string()], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?;
        rows.map(|row| {
            let (id, title, path) = row?;
            Ok(Backlink {
                source_id: Uuid::parse_str(&id)?,
                source_title: title,
                source_path: path,
            })
        })
        .collect()
    }

    pub fn resolve_target(&self, target: &str) -> Result<Option<Uuid>> {
        let target = normalize_link_target(target);
        let connection = self.connect()?;
        let mut statement = connection.prepare(
            "SELECT id FROM notes
             WHERE lower(title) = ?1 OR lower(path_without_extension) = ?1
             ORDER BY CASE WHEN lower(path_without_extension) = ?1 THEN 0 ELSE 1 END
             LIMIT 1",
        )?;
        let id = statement
            .query_row([target], |row| row.get::<_, String>(0))
            .optional()?;
        id.map(|id| Uuid::parse_str(&id).map_err(Error::from))
            .transpose()
    }

    pub fn delete_note(&self, id: Uuid) -> Result<()> {
        self.delete_notes(&[id])
    }

    pub fn delete_notes(&self, ids: &[Uuid]) -> Result<()> {
        if ids.is_empty() {
            return Ok(());
        }
        let trash = self.root.join(TRASH_DIRECTORY);
        fs::create_dir_all(&trash)?;
        let notes = ids
            .iter()
            .map(|id| self.load_note(*id))
            .collect::<Result<Vec<_>>>()?;
        let timestamp = Utc::now().format("%Y%m%d-%H%M%S").to_string();
        for note in &notes {
            let source = self.root.join(&note.relative_path);
            let file_name = source
                .file_name()
                .and_then(|name| name.to_str())
                .ok_or(Error::InvalidPath)?;
            let mut destination = trash.join(format!("{timestamp}-{file_name}"));
            let mut suffix = 2;
            while destination.exists() {
                destination = trash.join(format!("{timestamp}-{suffix}-{file_name}"));
                suffix += 1;
            }
            fs::rename(source, destination)?;
        }
        let mut connection = self.connect()?;
        let transaction = connection.transaction()?;
        for id in ids {
            transaction.execute("DELETE FROM notes_fts WHERE note_id = ?1", [id.to_string()])?;
            transaction.execute("DELETE FROM notes WHERE id = ?1", [id.to_string()])?;
            transaction.execute(
                "DELETE FROM note_order WHERE note_id = ?1",
                [id.to_string()],
            )?;
        }
        rebuild_links_from_index(&transaction)?;
        transaction.commit()?;
        Ok(())
    }

    pub fn delete_folders(&self, folders: &[String]) -> Result<()> {
        let mut folders = folders
            .iter()
            .map(|folder| validate_folder_path(folder))
            .collect::<Result<Vec<_>>>()?;
        folders.retain(|folder| !folder.as_os_str().is_empty());
        folders.sort_by_key(|folder| folder.components().count());
        let mut roots = Vec::<PathBuf>::new();
        for folder in folders {
            if !roots.iter().any(|root| folder.starts_with(root)) {
                roots.push(folder);
            }
        }
        if roots.is_empty() {
            return Ok(());
        }

        let trash = self.root.join(TRASH_DIRECTORY);
        fs::create_dir_all(&trash)?;
        let timestamp = Utc::now().format("%Y%m%d-%H%M%S").to_string();
        let mut removed = Vec::new();
        for folder in roots {
            let source = self.root.join(&folder);
            if !source.is_dir() {
                continue;
            }
            let name = folder
                .file_name()
                .and_then(|name| name.to_str())
                .ok_or(Error::InvalidPath)?;
            let mut destination = trash.join(format!("{timestamp}-{name}"));
            let mut suffix = 2;
            while destination.exists() {
                destination = trash.join(format!("{timestamp}-{suffix}-{name}"));
                suffix += 1;
            }
            fs::rename(source, destination)?;
            removed.push(path_text(&folder)?);
        }

        let _notes = self.reindex()?;
        let mut connection = self.connect()?;
        let transaction = connection.transaction()?;
        for folder in removed {
            transaction.execute(
                "DELETE FROM folder_order WHERE path = ?1 OR instr(path, ?1 || '/') = 1",
                [folder],
            )?;
        }
        transaction.execute(
            "DELETE FROM note_order WHERE note_id NOT IN (SELECT id FROM notes)",
            [],
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub fn graph(&self) -> Result<KnowledgeGraph> {
        let connection = self.connect()?;
        let mut node_statement = connection.prepare(
            "SELECT n.id, n.title, n.path, n.tags_text,
                    COALESCE(p.x, 0), COALESCE(p.y, 0),
                    (SELECT count(*) FROM links WHERE source_id = n.id OR target_id = n.id)
             FROM notes n LEFT JOIN graph_positions p ON p.note_id = n.id
             ORDER BY n.title COLLATE NOCASE",
        )?;
        let nodes = node_statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, f32>(4)?,
                    row.get::<_, f32>(5)?,
                    row.get::<_, i64>(6)?,
                ))
            })?
            .map(|row| {
                let (id, title, path, tags, x, y, degree) = row?;
                Ok(GraphNode {
                    id: Uuid::parse_str(&id)?,
                    title,
                    relative_path: path,
                    tags: split_tags(&tags),
                    x,
                    y,
                    degree: degree.max(0) as usize,
                })
            })
            .collect::<Result<Vec<_>>>()?;

        let mut edge_statement = connection.prepare(
            "SELECT source_id, target_id, target_raw FROM links ORDER BY source_id, target_raw",
        )?;
        let edges = edge_statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })?
            .map(|row| {
                let (source, target, target_raw) = row?;
                let target_id = target.as_deref().map(Uuid::parse_str).transpose()?;
                Ok(GraphEdge {
                    source: Uuid::parse_str(&source)?,
                    target: target_id,
                    unresolved_target: if target_id.is_none() {
                        Some(target_raw)
                    } else {
                        None
                    },
                })
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(KnowledgeGraph { nodes, edges })
    }

    pub fn save_graph_positions(&self, graph: &KnowledgeGraph) -> Result<()> {
        let mut connection = self.connect()?;
        let transaction = connection.transaction()?;
        for node in &graph.nodes {
            transaction.execute(
                "INSERT INTO graph_positions (note_id, x, y) VALUES (?1, ?2, ?3)
                 ON CONFLICT(note_id) DO UPDATE SET x = excluded.x, y = excluded.y",
                params![node.id.to_string(), node.x, node.y],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }

    fn write_note_file(&self, note: &Note) -> Result<()> {
        validate_relative_path(&note.relative_path)?;
        let path = self.root.join(&note.relative_path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let file_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or(Error::InvalidPath)?;
        let temporary = path.with_file_name(format!(".{file_name}.{}.tmp", Uuid::new_v4()));
        let mut file = fs::File::create(&temporary)?;
        file.write_all(note.to_markdown().as_bytes())?;
        file.sync_all()?;
        fs::rename(&temporary, &path)?;
        Ok(())
    }

    fn index_note(&self, note: &Note) -> Result<()> {
        let mut connection = self.connect()?;
        let transaction = connection.transaction()?;
        transaction.execute(
            "DELETE FROM notes_fts WHERE note_id = ?1",
            [note.id.to_string()],
        )?;
        transaction.execute("DELETE FROM notes WHERE id = ?1", [note.id.to_string()])?;
        insert_note(&transaction, note)?;
        rebuild_links_from_index(&transaction)?;
        transaction.commit()?;
        Ok(())
    }

    fn rename_note_file_for_title(&self, note: &mut Note) -> Result<()> {
        let current = self.root.join(&note.relative_path);
        if !current.exists() {
            return Ok(());
        }
        let current_stem = current
            .file_stem()
            .and_then(|stem| stem.to_str())
            .ok_or(Error::InvalidPath)?;
        let desired_stem = slugify(&note.title);
        if current_stem == desired_stem {
            return Ok(());
        }
        let parent = Path::new(&note.relative_path)
            .parent()
            .unwrap_or_else(|| Path::new(""));
        let mut relative = parent.join(format!("{desired_stem}.md"));
        let mut suffix = 2;
        while self.root.join(&relative).exists() {
            relative = parent.join(format!("{desired_stem}-{suffix}.md"));
            suffix += 1;
        }
        let destination = self.root.join(&relative);
        fs::rename(&current, &destination)?;
        note.relative_path = path_text(&relative)?;
        Ok(())
    }

    fn connect(&self) -> rusqlite::Result<Connection> {
        let connection = Connection::open(self.index_path.as_ref())?;
        connection.busy_timeout(Duration::from_secs(2))?;
        connection.execute_batch("PRAGMA foreign_keys = ON;")?;
        Ok(connection)
    }

    fn initialize_index(&self) -> Result<()> {
        let connection = self.connect()?;
        connection.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;
             CREATE TABLE IF NOT EXISTS notes (
                 id TEXT PRIMARY KEY NOT NULL,
                 title TEXT NOT NULL,
                 path TEXT UNIQUE NOT NULL,
                 path_without_extension TEXT NOT NULL,
                 body TEXT NOT NULL,
                 tags_text TEXT NOT NULL DEFAULT '',
                 created_at TEXT NOT NULL,
                 updated_at TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS tags (
                 note_id TEXT NOT NULL REFERENCES notes(id) ON DELETE CASCADE,
                 tag TEXT NOT NULL,
                 PRIMARY KEY (note_id, tag)
             );
             CREATE TABLE IF NOT EXISTS links (
                 source_id TEXT NOT NULL REFERENCES notes(id) ON DELETE CASCADE,
                 target_raw TEXT NOT NULL,
                 target_id TEXT REFERENCES notes(id) ON DELETE SET NULL,
                 label TEXT NOT NULL,
                 PRIMARY KEY (source_id, target_raw, label)
             );
             CREATE TABLE IF NOT EXISTS graph_positions (
                 note_id TEXT PRIMARY KEY NOT NULL,
                 x REAL NOT NULL,
                 y REAL NOT NULL
             );
             CREATE TABLE IF NOT EXISTS folder_order (
                 path TEXT PRIMARY KEY NOT NULL,
                 position INTEGER NOT NULL
             );
             CREATE TABLE IF NOT EXISTS note_order (
                 note_id TEXT PRIMARY KEY NOT NULL,
                 position INTEGER NOT NULL
             );
             CREATE VIRTUAL TABLE IF NOT EXISTS notes_fts USING fts5(
                 note_id UNINDEXED,
                 title,
                 body,
                 tags,
                 path,
                 tokenize = 'unicode61 remove_diacritics 2'
             );",
        )?;
        Ok(())
    }
}

fn insert_note(transaction: &Transaction<'_>, note: &Note) -> rusqlite::Result<()> {
    let tags_text = note.tags.join(" ");
    transaction.execute(
        "INSERT INTO notes
            (id, title, path, path_without_extension, body, tags_text, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            note.id.to_string(),
            note.title,
            note.relative_path,
            path_without_extension(&note.relative_path),
            note.body,
            tags_text,
            note.created_at.to_rfc3339(),
            note.updated_at.to_rfc3339(),
        ],
    )?;
    for tag in &note.tags {
        transaction.execute(
            "INSERT OR IGNORE INTO tags (note_id, tag) VALUES (?1, ?2)",
            params![note.id.to_string(), tag],
        )?;
    }
    transaction.execute(
        "INSERT INTO notes_fts (note_id, title, body, tags, path)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            note.id.to_string(),
            note.title,
            note.body,
            note.tags.join(" "),
            note.relative_path,
        ],
    )?;
    Ok(())
}

fn resolve_and_insert_links(transaction: &Transaction<'_>, notes: &[Note]) -> Result<()> {
    let mut targets = HashMap::new();
    for note in notes {
        targets
            .entry(normalize_link_target(&note.title))
            .or_insert(note.id);
        targets
            .entry(normalize_link_target(&path_without_extension(
                &note.relative_path,
            )))
            .or_insert(note.id);
    }
    for note in notes {
        for link in note.links() {
            let target_id = targets.get(&normalize_link_target(&link.target)).copied();
            transaction.execute(
                "INSERT OR IGNORE INTO links (source_id, target_raw, target_id, label)
                 VALUES (?1, ?2, ?3, ?4)",
                params![
                    note.id.to_string(),
                    link.target,
                    target_id.map(|id| id.to_string()),
                    link.label,
                ],
            )?;
        }
    }
    Ok(())
}

fn rebuild_links_from_index(transaction: &Transaction<'_>) -> Result<()> {
    let indexed_notes = {
        let mut statement = transaction.prepare("SELECT id, title, path, body FROM notes")?;
        statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?
    };
    let mut targets = HashMap::new();
    for (id, title, path, _) in &indexed_notes {
        let id = Uuid::parse_str(id)?;
        targets.entry(normalize_link_target(title)).or_insert(id);
        targets
            .entry(normalize_link_target(&path_without_extension(path)))
            .or_insert(id);
    }
    transaction.execute("DELETE FROM links", [])?;
    for (source, _, _, body) in indexed_notes {
        for link in notiz_core::extract_wikilinks(&body) {
            let target_id = targets.get(&normalize_link_target(&link.target)).copied();
            transaction.execute(
                "INSERT OR IGNORE INTO links (source_id, target_raw, target_id, label)
                 VALUES (?1, ?2, ?3, ?4)",
                params![
                    source,
                    link.target,
                    target_id.map(|id| id.to_string()),
                    link.label,
                ],
            )?;
        }
    }
    Ok(())
}

fn summary_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<NoteSummary> {
    let id = row.get::<_, String>(0)?;
    let updated_at = row.get::<_, String>(4)?;
    let body = row.get::<_, String>(5)?;
    Ok(NoteSummary {
        id: Uuid::parse_str(&id).map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
                0,
                rusqlite::types::Type::Text,
                Box::new(error),
            )
        })?,
        title: row.get(1)?,
        relative_path: row.get(2)?,
        tags: split_tags(&row.get::<_, String>(3)?),
        updated_at: chrono::DateTime::parse_from_rfc3339(&updated_at)
            .map(|timestamp| timestamp.with_timezone(&Utc))
            .map_err(|error| {
                rusqlite::Error::FromSqlConversionFailure(
                    4,
                    rusqlite::types::Type::Text,
                    Box::new(error),
                )
            })?,
        excerpt: body
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .chars()
            .take(140)
            .collect(),
    })
}

fn markdown_files(root: &Path) -> Result<Vec<PathBuf>> {
    fn visit(root: &Path, directory: &Path, files: &mut Vec<PathBuf>) -> Result<()> {
        for entry in fs::read_dir(directory)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                if path.file_name().and_then(|name| name.to_str()) != Some(TRASH_DIRECTORY) {
                    visit(root, &path, files)?;
                }
            } else if path
                .extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| extension.eq_ignore_ascii_case("md"))
                && path.starts_with(root)
            {
                files.push(path);
            }
        }
        Ok(())
    }

    let mut files = Vec::new();
    visit(root, root, &mut files)?;
    files.sort();
    Ok(files)
}

fn validate_relative_path(path: &str) -> Result<()> {
    let path = Path::new(path);
    if path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
        || path.extension().and_then(|extension| extension.to_str()) != Some("md")
    {
        return Err(Error::UnsafePath(path.display().to_string()));
    }
    Ok(())
}

fn validate_folder_path(path: &str) -> Result<PathBuf> {
    let path = PathBuf::from(path.trim().replace('\\', "/"));
    if path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(Error::UnsafePath(path.display().to_string()));
    }
    Ok(path)
}

fn path_text(path: &Path) -> Result<String> {
    path.to_str()
        .map(|path| path.replace('\\', "/"))
        .ok_or(Error::InvalidPath)
}

fn path_without_extension(path: &str) -> String {
    Path::new(path)
        .with_extension("")
        .to_string_lossy()
        .replace('\\', "/")
}

fn normalize_link_target(target: &str) -> String {
    target
        .trim()
        .trim_end_matches(".md")
        .replace('\\', "/")
        .to_lowercase()
}

fn split_tags(tags: &str) -> Vec<String> {
    tags.split_whitespace().map(str::to_owned).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temporary_vault() -> (PathBuf, PathBuf) {
        let root = std::env::temp_dir().join(format!("notiz-vault-{}", Uuid::new_v4()));
        let index = std::env::temp_dir().join(format!("notiz-index-{}.sqlite3", Uuid::new_v4()));
        (root, index)
    }

    fn cleanup(root: &Path, index: &Path) {
        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_file(index);
        let _ = fs::remove_file(index.with_extension("sqlite3-shm"));
        let _ = fs::remove_file(index.with_extension("sqlite3-wal"));
    }

    #[test]
    fn creates_saves_and_loads_markdown_notes() {
        let (root, index) = temporary_vault();
        let vault = Vault::open(&root, &index).expect("vault should open");
        let mut note = vault
            .create_note("Projeto Atlas")
            .expect("note should create");
        note.body = "Primeiro registro".into();
        note.tags = vec!["projeto".into()];
        note.updated_at = Utc::now();
        vault.save_note(&mut note).expect("note should save");

        let loaded = vault.load_note(note.id).expect("note should load");
        assert_eq!(loaded.body, "Primeiro registro");
        assert_eq!(loaded.tags, vec!["projeto"]);
        assert!(root.join("projeto-atlas.md").exists());

        note.title = "Atlas renovado".into();
        vault
            .save_note(&mut note)
            .expect("renamed note should save");
        assert_eq!(note.relative_path, "atlas-renovado.md");
        assert!(root.join("atlas-renovado.md").exists());
        assert!(!root.join("projeto-atlas.md").exists());
        cleanup(&root, &index);
    }

    #[test]
    fn creates_real_folders_and_moves_markdown_notes() {
        let (root, index) = temporary_vault();
        let vault = Vault::open(&root, &index).expect("vault should open");
        vault
            .create_folder("Projetos/Atlas")
            .expect("nested folder should create");
        let note = vault
            .create_note_in("Roadmap", "Projetos/Atlas")
            .expect("note should create inside folder");
        assert!(root.join("Projetos/Atlas/roadmap.md").exists());

        let moved = vault
            .move_note(note.id, "Arquivo")
            .expect("note should move");
        assert_eq!(moved.relative_path, "Arquivo/roadmap.md");
        assert!(root.join("Arquivo/roadmap.md").exists());
        assert!(!root.join("Projetos/Atlas/roadmap.md").exists());
        assert!(
            vault
                .folders()
                .expect("folders should list")
                .contains(&"Arquivo".into())
        );
        cleanup(&root, &index);
    }

    #[test]
    fn index_resolves_links_search_and_backlinks() {
        let (root, index) = temporary_vault();
        let vault = Vault::open(&root, &index).expect("vault should open");
        let atlas = vault.create_note("Projeto Atlas").expect("atlas");
        let mut meeting = vault.create_note("Reunião").expect("meeting");
        meeting.body = "Decisões sobre [[Projeto Atlas|Atlas]]".into();
        vault.save_note(&mut meeting).expect("meeting should save");

        let results = vault.search("decisões atlas", 20).expect("search");
        assert_eq!(results[0].id, meeting.id);
        let backlinks = vault.backlinks(atlas.id).expect("backlinks");
        assert_eq!(backlinks[0].source_id, meeting.id);
        let graph = vault.graph().expect("graph");
        assert_eq!(graph.nodes.len(), 2);
        assert_eq!(graph.edges[0].target, Some(atlas.id));
        cleanup(&root, &index);
    }

    #[test]
    fn imports_plain_markdown_and_adds_portable_metadata() {
        let (root, index) = temporary_vault();
        fs::create_dir_all(&root).expect("root");
        fs::write(root.join("legado.md"), "# Legado\n\nConteúdo").expect("fixture");
        let vault = Vault::open(&root, &index).expect("vault should open");
        let notes = vault.reindex().expect("reindex");
        assert_eq!(notes[0].title, "Legado");
        let normalized = fs::read_to_string(root.join("legado.md")).expect("normalized");
        assert!(normalized.starts_with("---\nid:"));
        cleanup(&root, &index);
    }

    #[test]
    fn deletion_moves_note_to_recoverable_trash() {
        let (root, index) = temporary_vault();
        let vault = Vault::open(&root, &index).expect("vault should open");
        let note = vault.create_note("Descartável").expect("note");
        vault.delete_note(note.id).expect("delete");
        assert!(vault.list_notes().expect("list").is_empty());
        assert_eq!(
            fs::read_dir(root.join(TRASH_DIRECTORY))
                .expect("trash")
                .count(),
            1
        );
        cleanup(&root, &index);
    }

    #[test]
    fn folder_deletion_moves_the_whole_tree_to_recoverable_trash() {
        let (root, index) = temporary_vault();
        let vault = Vault::open(&root, &index).expect("vault should open");
        vault.create_folder("Projects/Atlas").expect("folder");
        let note = vault
            .create_note_in("Roadmap", "Projects/Atlas")
            .expect("note");
        vault
            .delete_folders(&["Projects".into(), "Projects/Atlas".into()])
            .expect("delete folder");
        assert!(!root.join("Projects").exists());
        assert!(vault.load_note(note.id).is_err());
        assert!(vault.list_notes().expect("notes").is_empty());
        assert_eq!(
            fs::read_dir(root.join(TRASH_DIRECTORY))
                .expect("trash")
                .count(),
            1
        );
        cleanup(&root, &index);
    }

    #[test]
    fn empty_folder_deletion_is_recoverable() {
        let (root, index) = temporary_vault();
        let vault = Vault::open(&root, &index).expect("vault should open");
        vault.create_folder("Empty").expect("folder");
        vault
            .delete_folders(&["Empty".into()])
            .expect("delete empty folder");
        assert!(!root.join("Empty").exists());
        assert_eq!(
            fs::read_dir(root.join(TRASH_DIRECTORY))
                .expect("trash")
                .count(),
            1
        );
        cleanup(&root, &index);
    }

    #[test]
    fn bulk_deletion_moves_every_note_in_one_operation() {
        let (root, index) = temporary_vault();
        let vault = Vault::open(&root, &index).expect("vault should open");
        let first = vault.create_note("First").expect("first note");
        let second = vault.create_note("Second").expect("second note");
        vault
            .delete_notes(&[first.id, second.id])
            .expect("bulk delete");
        assert!(vault.list_notes().expect("list").is_empty());
        assert_eq!(
            fs::read_dir(root.join(TRASH_DIRECTORY))
                .expect("trash")
                .count(),
            2
        );
        cleanup(&root, &index);
    }

    #[test]
    fn folder_order_is_persisted_in_the_local_index() {
        let (root, index) = temporary_vault();
        let vault = Vault::open(&root, &index).expect("vault should open");
        vault.create_folder("Alpha").expect("alpha");
        vault.create_folder("Beta").expect("beta");
        vault
            .save_folder_order(&["Beta".into(), "Alpha".into()])
            .expect("folder order");
        assert_eq!(vault.folders().expect("folders"), vec!["Beta", "Alpha"]);
        cleanup(&root, &index);
    }

    #[test]
    fn note_order_is_persisted_independently_from_markdown_paths() {
        let (root, index) = temporary_vault();
        let vault = Vault::open(&root, &index).expect("vault should open");
        let first = vault.create_note("First").expect("first");
        let second = vault.create_note("Second").expect("second");
        vault
            .save_note_order(&[second.id, first.id])
            .expect("note order");
        let notes = vault.list_notes().expect("notes");
        assert_eq!(notes[0].id, second.id);
        assert_eq!(notes[1].id, first.id);
        cleanup(&root, &index);
    }

    #[test]
    fn moving_a_folder_updates_nested_markdown_paths_without_changing_ids() {
        let (root, index) = temporary_vault();
        let vault = Vault::open(&root, &index).expect("vault should open");
        vault.create_folder("Work/Project").expect("project");
        vault.create_folder("Archive").expect("archive");
        let note = vault
            .create_note_in("Roadmap", "Work/Project")
            .expect("note");
        let moved = vault
            .move_folder("Work/Project", "Archive")
            .expect("move folder");
        assert_eq!(moved, "Archive/Project");
        let loaded = vault.load_note(note.id).expect("same note id");
        assert_eq!(loaded.relative_path, "Archive/Project/roadmap.md");
        assert!(root.join("Archive/Project/roadmap.md").exists());
        assert!(!root.join("Work/Project").exists());
        cleanup(&root, &index);
    }
}
