use std::borrow::Cow;

use iced::widget::{column, container, markdown, scrollable};
use iced::{Color, Element, Font, Length, Theme};
use iced_selection::{rich_text, text};

const MATCH_BACKGROUND: Color = Color::from_rgba(0.96, 0.72, 0.20, 0.34);

#[derive(Debug, Clone)]
struct SearchViewer {
    query: String,
}

pub fn view<'a>(
    items: &'a [markdown::Item],
    settings: impl Into<markdown::Settings>,
    query: &str,
) -> Element<'a, markdown::Uri> {
    markdown::view_with(
        items,
        settings,
        &SearchViewer {
            query: query.trim().to_owned(),
        },
    )
}

impl<'a> markdown::Viewer<'a, markdown::Uri, Theme, iced::Renderer> for SearchViewer {
    fn on_link_click(url: markdown::Uri) -> markdown::Uri {
        url
    }

    fn heading(
        &self,
        settings: markdown::Settings,
        level: &'a markdown::HeadingLevel,
        source: &'a markdown::Text,
        index: usize,
    ) -> Element<'a, markdown::Uri> {
        let size = match level {
            markdown::HeadingLevel::H1 => settings.h1_size,
            markdown::HeadingLevel::H2 => settings.h2_size,
            markdown::HeadingLevel::H3 => settings.h3_size,
            markdown::HeadingLevel::H4 => settings.h4_size,
            markdown::HeadingLevel::H5 => settings.h5_size,
            markdown::HeadingLevel::H6 => settings.h6_size,
        };
        container(
            rich_text(highlighted_spans(source, settings.style, &self.query))
                .on_link_click(Self::on_link_click)
                .size(size),
        )
        .padding(iced::Padding {
            top: if index > 0 {
                settings.text_size.0 / 2.0
            } else {
                0.0
            },
            ..iced::Padding::ZERO
        })
        .into()
    }

    fn paragraph(
        &self,
        settings: markdown::Settings,
        source: &markdown::Text,
    ) -> Element<'a, markdown::Uri> {
        rich_text(highlighted_spans(source, settings.style, &self.query))
            .on_link_click(Self::on_link_click)
            .size(settings.text_size)
            .into()
    }

    fn unordered_list(
        &self,
        settings: markdown::Settings,
        bullets: &'a [markdown::Bullet],
    ) -> Element<'a, markdown::Uri> {
        iced_selection::markdown::unordered_list(self, settings, bullets)
    }

    fn ordered_list(
        &self,
        settings: markdown::Settings,
        start: u64,
        bullets: &'a [markdown::Bullet],
    ) -> Element<'a, markdown::Uri> {
        iced_selection::markdown::ordered_list(self, settings, start, bullets)
    }

    fn code_block(
        &self,
        settings: markdown::Settings,
        _language: Option<&'a str>,
        _code: &'a str,
        lines: &'a [markdown::Text],
    ) -> Element<'a, markdown::Uri> {
        container(
            scrollable(
                container(column(lines.iter().map(|line| {
                    rich_text(highlighted_spans(line, settings.style, &self.query))
                        .on_link_click(Self::on_link_click)
                        .font(Font::MONOSPACE)
                        .size(settings.code_size)
                        .into()
                })))
                .padding(settings.code_size),
            )
            .direction(scrollable::Direction::Horizontal(
                scrollable::Scrollbar::default()
                    .width(settings.code_size / 2)
                    .scroller_width(settings.code_size / 2),
            )),
        )
        .width(Length::Fill)
        .padding(settings.code_size / 4)
        .into()
    }
}

fn highlighted_spans(
    source: &markdown::Text,
    style: markdown::Style,
    query: &str,
) -> Vec<text::Span<'static, markdown::Uri, Font>> {
    source
        .spans(style)
        .iter()
        .flat_map(|span| {
            let value = span.text.as_ref();
            let ranges = case_insensitive_ranges(value, query);
            if ranges.is_empty() {
                return vec![span.clone()];
            }
            let mut pieces = Vec::with_capacity(ranges.len() * 2 + 1);
            let mut cursor = 0;
            for range in ranges {
                if cursor < range.start {
                    let mut piece = span.clone();
                    piece.text = Cow::Owned(value[cursor..range.start].to_owned());
                    pieces.push(piece);
                }
                let mut matched = span.clone();
                matched.text = Cow::Owned(value[range.clone()].to_owned());
                pieces.push(matched.background(MATCH_BACKGROUND));
                cursor = range.end;
            }
            if cursor < value.len() {
                let mut piece = span.clone();
                piece.text = Cow::Owned(value[cursor..].to_owned());
                pieces.push(piece);
            }
            pieces
        })
        .collect()
}

pub fn case_insensitive_ranges(value: &str, query: &str) -> Vec<std::ops::Range<usize>> {
    let query = query.trim();
    if query.is_empty() {
        return Vec::new();
    }
    let query_lower = query.to_lowercase();
    let query_chars = query.chars().count();
    let boundaries = value
        .char_indices()
        .map(|(index, _)| index)
        .chain(std::iter::once(value.len()))
        .collect::<Vec<_>>();
    let mut ranges = Vec::new();
    let mut start = 0;
    while start + query_chars < boundaries.len() {
        let end = start + query_chars;
        let range = boundaries[start]..boundaries[end];
        if value[range.clone()].to_lowercase() == query_lower {
            ranges.push(range);
            start = end;
        } else {
            start += 1;
        }
    }
    ranges
}

#[cfg(test)]
mod tests {
    use super::case_insensitive_ranges;

    #[test]
    fn finds_every_case_insensitive_occurrence() {
        assert_eq!(
            case_insensitive_ranges("Notiz keeps NOTIZ notes", "notiz"),
            vec![0..5, 12..17]
        );
    }

    #[test]
    fn returns_utf8_safe_byte_ranges() {
        let value = "ação e AÇÃO";
        let matches = case_insensitive_ranges(value, "ação");
        assert_eq!(matches, vec![0..6, 9..15]);
        assert_eq!(&value[matches[0].clone()], "ação");
        assert_eq!(&value[matches[1].clone()], "AÇÃO");
    }

    #[test]
    fn ignores_an_empty_query() {
        assert!(case_insensitive_ranges("anything", "  ").is_empty());
    }
}
