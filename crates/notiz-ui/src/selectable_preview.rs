use iced::{Color, Element, Fill, Font, font};
use iced_selection::{rich_text, span, text};
use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd};

use crate::searchable_markdown::case_insensitive_ranges;
use crate::theme::{DIM, INDIGO_LIGHT, MUTED, TEXT};

const MATCH_BACKGROUND: Color = Color::from_rgba(1.0, 0.78, 0.60, 0.34);
const CODE_BACKGROUND: Color = Color::from_rgba(0.14, 0.14, 0.14, 0.72);

#[derive(Debug, Clone, Default)]
pub struct Document {
    spans: Vec<PreviewSpan>,
}

#[derive(Debug, Clone)]
struct PreviewSpan {
    text: String,
    heading: Option<u8>,
    strong: bool,
    emphasis: bool,
    strikethrough: bool,
    code: bool,
    link: Option<String>,
    muted: bool,
}

#[derive(Debug, Clone, Default)]
struct SpanStyle {
    heading: Option<u8>,
    strong: bool,
    emphasis: bool,
    strikethrough: bool,
    code: bool,
    link: Option<String>,
    muted: bool,
}

#[derive(Debug, Clone, Copy)]
struct ListState {
    next: Option<u64>,
}

impl Document {
    pub fn parse(markdown: &str) -> Self {
        let mut options = Options::empty();
        options.insert(Options::ENABLE_TABLES);
        options.insert(Options::ENABLE_STRIKETHROUGH);
        options.insert(Options::ENABLE_TASKLISTS);

        let mut document = Self::default();
        let mut style = SpanStyle::default();
        let mut lists = Vec::<ListState>::new();
        let mut table_cell = 0usize;

        for event in Parser::new_ext(markdown, options) {
            match event {
                Event::Start(tag) => match tag {
                    Tag::Heading { level, .. } => style.heading = Some(heading_level(level)),
                    Tag::Strong => style.strong = true,
                    Tag::Emphasis => style.emphasis = true,
                    Tag::Strikethrough => style.strikethrough = true,
                    Tag::CodeBlock(_) => style.code = true,
                    Tag::Link { dest_url, .. } | Tag::Image { dest_url, .. } => {
                        style.link = Some(dest_url.into_string());
                    }
                    Tag::List(start) => lists.push(ListState { next: start }),
                    Tag::Item => {
                        let indent = "  ".repeat(lists.len().saturating_sub(1));
                        let marker = if let Some(list) = lists.last_mut() {
                            if let Some(next) = list.next.as_mut() {
                                let marker = format!("{next}. ");
                                *next += 1;
                                marker
                            } else {
                                "• ".to_owned()
                            }
                        } else {
                            "• ".to_owned()
                        };
                        document.push_plain(format!("{indent}{marker}"));
                    }
                    Tag::BlockQuote(_) => document.push_muted("│ "),
                    Tag::TableRow => table_cell = 0,
                    Tag::TableCell => {
                        if table_cell > 0 {
                            document.push_muted("  │  ");
                        }
                        table_cell += 1;
                    }
                    Tag::FootnoteDefinition(label) => {
                        document.push_muted(format!("[{label}] "));
                    }
                    Tag::DefinitionListDefinition => document.push_muted("  "),
                    Tag::Paragraph
                    | Tag::HtmlBlock
                    | Tag::Table(_)
                    | Tag::TableHead
                    | Tag::MetadataBlock(_)
                    | Tag::DefinitionList
                    | Tag::DefinitionListTitle => {}
                },
                Event::End(tag) => match tag {
                    TagEnd::Paragraph => document.newlines(2),
                    TagEnd::Heading(_) => {
                        document.newlines(2);
                        style.heading = None;
                    }
                    TagEnd::Item | TagEnd::TableRow => document.newlines(1),
                    TagEnd::CodeBlock => {
                        document.newlines(2);
                        style.code = false;
                    }
                    TagEnd::Table | TagEnd::BlockQuote(_) => {
                        document.newlines(2);
                    }
                    TagEnd::Strong => style.strong = false,
                    TagEnd::Emphasis => style.emphasis = false,
                    TagEnd::Strikethrough => style.strikethrough = false,
                    TagEnd::Link | TagEnd::Image => style.link = None,
                    TagEnd::List(_) => {
                        lists.pop();
                        document.newlines(1);
                    }
                    TagEnd::FootnoteDefinition
                    | TagEnd::DefinitionListDefinition
                    | TagEnd::DefinitionListTitle => document.newlines(1),
                    TagEnd::HtmlBlock
                    | TagEnd::TableHead
                    | TagEnd::TableCell
                    | TagEnd::MetadataBlock(_)
                    | TagEnd::DefinitionList => {}
                },
                Event::Text(value) => document.push(value.into_string(), &style),
                Event::Code(value) => {
                    let mut inline = style.clone();
                    inline.code = true;
                    document.push(value.into_string(), &inline);
                }
                Event::InlineMath(value) => document.push(format!("${value}$"), &style),
                Event::DisplayMath(value) => {
                    document.push(format!("$${value}$$"), &style);
                    document.newlines(2);
                }
                Event::Html(value) | Event::InlineHtml(value) => {
                    document.push(value.into_string(), &style);
                }
                Event::FootnoteReference(label) => {
                    document.push_muted(format!("[{label}]"));
                }
                Event::SoftBreak => document.push_plain(" "),
                Event::HardBreak => document.newlines(1),
                Event::Rule => {
                    document.push_muted("────────────────────────────────");
                    document.newlines(2);
                }
                Event::TaskListMarker(done) => {
                    document.push_muted(if done { "☑ " } else { "☐ " });
                }
            }
        }

        if document.spans.is_empty() {
            document.push_muted("Empty note.");
        }
        document
    }

    pub fn view(&self, query: &str) -> Element<'_, String> {
        let query = query.trim();
        let spans = self
            .spans
            .iter()
            .flat_map(|source| styled_spans(source, query))
            .collect::<Vec<_>>();
        rich_text(spans)
            .on_link_click(|url| url)
            .size(14)
            .line_height(1.55)
            .width(Fill)
            .into()
    }

    fn push(&mut self, text: impl Into<String>, style: &SpanStyle) {
        let text = text.into();
        if text.is_empty() {
            return;
        }
        self.spans.push(PreviewSpan {
            text,
            heading: style.heading,
            strong: style.strong,
            emphasis: style.emphasis,
            strikethrough: style.strikethrough,
            code: style.code,
            link: style.link.clone(),
            muted: style.muted,
        });
    }

    fn push_plain(&mut self, text: impl Into<String>) {
        self.push(text, &SpanStyle::default());
    }

    fn push_muted(&mut self, text: impl Into<String>) {
        self.push(
            text,
            &SpanStyle {
                muted: true,
                ..SpanStyle::default()
            },
        );
    }

    fn newlines(&mut self, count: usize) {
        let trailing = self.spans.last().map_or(0, |span| {
            span.text.chars().rev().take_while(|c| *c == '\n').count()
        });
        if trailing < count {
            self.push_plain("\n".repeat(count - trailing));
        }
    }
}

fn styled_spans(source: &PreviewSpan, query: &str) -> Vec<text::Span<'static, String, Font>> {
    let ranges = case_insensitive_ranges(&source.text, query);
    if ranges.is_empty() {
        return vec![styled_span(source.text.clone(), source, false)];
    }

    let mut spans = Vec::with_capacity(ranges.len() * 2 + 1);
    let mut cursor = 0;
    for range in ranges {
        if cursor < range.start {
            spans.push(styled_span(
                source.text[cursor..range.start].to_owned(),
                source,
                false,
            ));
        }
        spans.push(styled_span(
            source.text[range.clone()].to_owned(),
            source,
            true,
        ));
        cursor = range.end;
    }
    if cursor < source.text.len() {
        spans.push(styled_span(source.text[cursor..].to_owned(), source, false));
    }
    spans
}

fn styled_span(
    value: String,
    source: &PreviewSpan,
    highlighted: bool,
) -> text::Span<'static, String, Font> {
    let mut font = if source.code {
        Font::MONOSPACE
    } else {
        Font::DEFAULT
    };
    font.weight = if source.strong || source.heading.is_some() {
        font::Weight::Bold
    } else {
        font::Weight::Normal
    };
    font.style = if source.emphasis {
        font::Style::Italic
    } else {
        font::Style::Normal
    };

    let mut result = span(value)
        .font(font)
        .color(if source.link.is_some() {
            INDIGO_LIGHT
        } else if source.muted {
            MUTED
        } else {
            TEXT
        })
        .strikethrough(source.strikethrough);
    if let Some(level) = source.heading {
        result = result.size(match level {
            1 => 27,
            2 => 23,
            3 => 20,
            4 => 17,
            _ => 15,
        });
    }
    if source.code {
        result = result.background(CODE_BACKGROUND).padding([1, 3]);
    }
    if highlighted {
        result = result.background(MATCH_BACKGROUND);
    }
    if let Some(link) = &source.link {
        result = result.link(link.clone()).underline(true);
    }
    if source.text.trim().is_empty() {
        result = result.color(DIM);
    }
    result
}

fn heading_level(level: HeadingLevel) -> u8 {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

#[cfg(test)]
mod tests {
    use super::Document;

    #[test]
    fn keeps_a_table_in_one_selectable_document() {
        let document = Document::parse("| A | B |\n|---|---|\n| one | two |");
        let text = document
            .spans
            .iter()
            .map(|span| span.text.as_str())
            .collect::<String>();
        assert!(text.contains("A"));
        assert!(text.contains("│"));
        assert!(text.contains("one"));
        assert!(text.contains("two"));
    }

    #[test]
    fn keeps_paragraphs_in_the_same_document() {
        let document = Document::parse("First paragraph.\n\nSecond paragraph.");
        let text = document
            .spans
            .iter()
            .map(|span| span.text.as_str())
            .collect::<String>();
        assert!(text.contains("First paragraph.\n\nSecond paragraph."));
    }
}
