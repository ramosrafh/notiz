use std::path::Path;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const DEFAULT_NOTE_TITLE: &str = "Nova nota";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Note {
    pub id: Uuid,
    pub title: String,
    pub relative_path: String,
    pub body: String,
    pub tags: Vec<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Note {
    pub fn new(title: impl Into<String>, relative_path: impl Into<String>) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4(),
            title: normalize_title(&title.into()),
            relative_path: relative_path.into(),
            body: String::new(),
            tags: Vec::new(),
            created_at: now,
            updated_at: now,
        }
    }

    pub fn from_markdown(relative_path: impl Into<String>, markdown: &str) -> ParsedNote {
        let relative_path = relative_path.into();
        let fallback_title = Path::new(&relative_path)
            .file_stem()
            .and_then(|name| name.to_str())
            .unwrap_or(DEFAULT_NOTE_TITLE)
            .replace(['-', '_'], " ");
        let mut note = Self::new(fallback_title, relative_path);
        let mut had_frontmatter = false;

        if let Some((frontmatter, body)) = split_frontmatter(markdown) {
            had_frontmatter = true;
            let mut in_tags = false;
            for line in frontmatter.lines() {
                let trimmed = line.trim();
                if let Some(value) = trimmed.strip_prefix("id:") {
                    if let Ok(id) = Uuid::parse_str(unquote(value.trim())) {
                        note.id = id;
                    }
                    in_tags = false;
                } else if let Some(value) = trimmed.strip_prefix("title:") {
                    note.title = normalize_title(unquote(value.trim()));
                    in_tags = false;
                } else if let Some(value) = trimmed.strip_prefix("created_at:") {
                    if let Some(timestamp) = parse_timestamp(unquote(value.trim())) {
                        note.created_at = timestamp;
                    }
                    in_tags = false;
                } else if let Some(value) = trimmed.strip_prefix("updated_at:") {
                    if let Some(timestamp) = parse_timestamp(unquote(value.trim())) {
                        note.updated_at = timestamp;
                    }
                    in_tags = false;
                } else if let Some(value) = trimmed.strip_prefix("tags:") {
                    note.tags.clear();
                    in_tags = true;
                    let inline = value.trim();
                    if inline.starts_with('[') && inline.ends_with(']') {
                        note.tags = inline[1..inline.len() - 1]
                            .split(',')
                            .map(|tag| normalize_tag(unquote(tag.trim())))
                            .filter(|tag| !tag.is_empty())
                            .collect();
                        in_tags = false;
                    }
                } else if in_tags {
                    if let Some(tag) = trimmed.strip_prefix('-') {
                        let tag = normalize_tag(unquote(tag.trim()));
                        if !tag.is_empty() && !note.tags.contains(&tag) {
                            note.tags.push(tag);
                        }
                    } else if !trimmed.is_empty() {
                        in_tags = false;
                    }
                }
            }
            let body = body.trim_start_matches('\n');
            note.body = body.strip_suffix('\n').unwrap_or(body).to_owned();
        } else {
            note.body = markdown.to_owned();
            if let Some(heading) = markdown
                .lines()
                .find_map(|line| line.trim().strip_prefix("# "))
            {
                note.title = normalize_title(heading);
            }
        }

        ParsedNote {
            note,
            had_frontmatter,
        }
    }

    pub fn to_markdown(&self) -> String {
        let mut markdown = String::new();
        markdown.push_str("---\n");
        markdown.push_str(&format!("id: {}\n", self.id));
        markdown.push_str(&format!("title: {}\n", yaml_string(&self.title)));
        markdown.push_str(&format!("created_at: {}\n", self.created_at.to_rfc3339()));
        markdown.push_str(&format!("updated_at: {}\n", self.updated_at.to_rfc3339()));
        markdown.push_str("tags:\n");
        for tag in &self.tags {
            markdown.push_str(&format!("  - {}\n", yaml_string(tag)));
        }
        markdown.push_str("---\n\n");
        markdown.push_str(self.body.trim_end());
        markdown.push('\n');
        markdown
    }

    pub fn links(&self) -> Vec<WikiLink> {
        extract_wikilinks(&self.body)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedNote {
    pub note: Note,
    pub had_frontmatter: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NoteSummary {
    pub id: Uuid,
    pub title: String,
    pub relative_path: String,
    pub tags: Vec<String>,
    pub updated_at: DateTime<Utc>,
    pub excerpt: String,
}

impl From<&Note> for NoteSummary {
    fn from(note: &Note) -> Self {
        Self {
            id: note.id,
            title: note.title.clone(),
            relative_path: note.relative_path.clone(),
            tags: note.tags.clone(),
            updated_at: note.updated_at,
            excerpt: excerpt(&note.body, 140),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WikiLink {
    pub target: String,
    pub label: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct KnowledgeGraph {
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GraphNode {
    pub id: Uuid,
    pub title: String,
    pub relative_path: String,
    pub tags: Vec<String>,
    pub x: f32,
    pub y: f32,
    pub degree: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphEdge {
    pub source: Uuid,
    pub target: Option<Uuid>,
    pub unresolved_target: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Backlink {
    pub source_id: Uuid,
    pub source_title: String,
    pub source_path: String,
}

pub fn normalize_title(title: &str) -> String {
    let title = title.trim();
    if title.is_empty() {
        DEFAULT_NOTE_TITLE.to_owned()
    } else {
        title.to_owned()
    }
}

pub fn normalize_tag(tag: &str) -> String {
    tag.trim()
        .trim_start_matches('#')
        .to_lowercase()
        .replace(char::is_whitespace, "-")
}

pub fn slugify(title: &str) -> String {
    let mut slug = String::new();
    let mut separator = false;
    for character in title.trim().chars() {
        if character.is_alphanumeric() || matches!(character, '_' | '-') {
            if separator && !slug.is_empty() && !slug.ends_with('-') {
                slug.push('-');
            }
            slug.extend(character.to_lowercase());
            separator = false;
        } else if character.is_whitespace() || matches!(character, '.' | ',') {
            separator = true;
        }
    }
    let slug = slug.trim_matches('-');
    if slug.is_empty() {
        "nota".into()
    } else {
        slug.into()
    }
}

pub fn extract_wikilinks(markdown: &str) -> Vec<WikiLink> {
    let mut links = Vec::new();
    let mut remaining = markdown;
    while let Some(start) = remaining.find("[[") {
        let after_start = &remaining[start + 2..];
        let Some(end) = after_start.find("]]") else {
            break;
        };
        let raw = after_start[..end].trim();
        let mut parts = raw.splitn(2, '|');
        let target = parts
            .next()
            .unwrap_or_default()
            .split('#')
            .next()
            .unwrap_or_default()
            .trim();
        let label = parts.next().unwrap_or(target).trim();
        if !target.is_empty() {
            links.push(WikiLink {
                target: target.to_owned(),
                label: if label.is_empty() { target } else { label }.to_owned(),
            });
        }
        remaining = &after_start[end + 2..];
    }
    links
}

pub fn renderable_markdown(body: &str) -> String {
    let mut rendered = String::with_capacity(body.len());
    let mut remaining = body;
    while let Some(start) = remaining.find("[[") {
        rendered.push_str(&remaining[..start]);
        let after_start = &remaining[start + 2..];
        let Some(end) = after_start.find("]]") else {
            rendered.push_str(&remaining[start..]);
            return rendered;
        };
        let raw = after_start[..end].trim();
        let mut parts = raw.splitn(2, '|');
        let target = parts.next().unwrap_or_default().trim();
        let label = parts.next().unwrap_or(target).trim();
        rendered.push('[');
        rendered.push_str(label);
        rendered.push_str("](notiz://");
        rendered.push_str(&target.replace(' ', "%20"));
        rendered.push(')');
        remaining = &after_start[end + 2..];
    }
    rendered.push_str(remaining);
    rendered
}

fn split_frontmatter(markdown: &str) -> Option<(&str, &str)> {
    let markdown = markdown.strip_prefix("---\n")?;
    let end = markdown.find("\n---")?;
    let after = end + 4;
    Some((&markdown[..end], &markdown[after..]))
}

fn parse_timestamp(value: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|timestamp| timestamp.with_timezone(&Utc))
}

fn yaml_string(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}

fn unquote(value: &str) -> &str {
    value
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .or_else(|| {
            value
                .strip_prefix('\'')
                .and_then(|value| value.strip_suffix('\''))
        })
        .unwrap_or(value)
}

fn excerpt(body: &str, max_chars: usize) -> String {
    let normalized = body.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut excerpt = normalized.chars().take(max_chars).collect::<String>();
    if normalized.chars().count() > max_chars {
        excerpt.push('…');
    }
    excerpt
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markdown_round_trip_preserves_note_data() {
        let mut note = Note::new("Reunião semanal", "reuniao-semanal.md");
        note.body = "Falamos com [[João|João Silva]].".into();
        note.tags = vec!["trabalho".into(), "1-on-1".into()];

        let parsed = Note::from_markdown(note.relative_path.clone(), &note.to_markdown());
        assert!(parsed.had_frontmatter);
        assert_eq!(parsed.note.id, note.id);
        assert_eq!(parsed.note.title, note.title);
        assert_eq!(parsed.note.tags, note.tags);
        assert_eq!(parsed.note.body, note.body);
    }

    #[test]
    fn imports_plain_markdown_using_heading_as_title() {
        let parsed = Note::from_markdown("ideias.md", "# Uma ideia\n\nTexto");
        assert!(!parsed.had_frontmatter);
        assert_eq!(parsed.note.title, "Uma ideia");
    }

    #[test]
    fn extracts_aliases_and_heading_fragments_from_wikilinks() {
        let links = extract_wikilinks("[[Projeto Atlas#status|Atlas]] e [[João]]");
        assert_eq!(links[0].target, "Projeto Atlas");
        assert_eq!(links[0].label, "Atlas");
        assert_eq!(links[1].target, "João");
    }

    #[test]
    fn slug_keeps_unicode_and_removes_path_separators() {
        assert_eq!(slugify(" Reunião / Semanal "), "reunião-semanal");
    }

    #[test]
    fn preview_converts_wikilinks_to_internal_markdown_links() {
        assert_eq!(
            renderable_markdown("Veja [[Projeto Atlas|Atlas]]."),
            "Veja [Atlas](notiz://Projeto%20Atlas)."
        );
    }
}
