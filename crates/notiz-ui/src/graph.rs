use std::collections::HashMap;

use iced::advanced::text;
use iced::mouse;
use iced::widget::canvas;
use iced::{Color, Element, Font, Length, Point, Rectangle, Renderer, Theme, Vector, alignment};
use notiz_core::KnowledgeGraph;
use uuid::Uuid;

use crate::theme::{BACKGROUND, BORDER, INDIGO_LIGHT, MUTED, SURFACE, TEXT};

#[derive(Debug, Clone)]
pub enum Message {
    OpenNote(Uuid),
}

#[derive(Debug, Default)]
pub struct State {
    scale: f32,
    offset: Vector,
    dragging: bool,
    last_cursor: Option<Point>,
}

pub fn view<'a>(graph: &'a KnowledgeGraph, active: Option<Uuid>) -> Element<'a, Message> {
    canvas(GraphProgram { graph, active })
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

struct GraphProgram<'a> {
    graph: &'a KnowledgeGraph,
    active: Option<Uuid>,
}

impl canvas::Program<Message> for GraphProgram<'_> {
    type State = State;

    fn update(
        &self,
        state: &mut State,
        event: &canvas::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<canvas::Action<Message>> {
        if state.scale == 0.0 {
            state.scale = 1.0;
        }
        let position = cursor.position_in(bounds);
        match event {
            canvas::Event::Mouse(mouse::Event::WheelScrolled { delta }) if position.is_some() => {
                let y = match delta {
                    mouse::ScrollDelta::Lines { y, .. } => *y,
                    mouse::ScrollDelta::Pixels { y, .. } => *y / 40.0,
                };
                state.scale = (state.scale * (1.0 + y * 0.10)).clamp(0.35, 4.0);
                Some(canvas::Action::capture())
            }
            canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                let position = position?;
                if let Some(id) = hit_node(self.graph, state, bounds, position) {
                    return Some(canvas::Action::publish(Message::OpenNote(id)).and_capture());
                }
                state.dragging = true;
                state.last_cursor = Some(position);
                Some(canvas::Action::capture())
            }
            canvas::Event::Mouse(mouse::Event::CursorMoved { .. }) if state.dragging => {
                let position = position?;
                if let Some(previous) = state.last_cursor {
                    state.offset += position - previous;
                }
                state.last_cursor = Some(position);
                Some(canvas::Action::capture())
            }
            canvas::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left))
                if state.dragging =>
            {
                state.dragging = false;
                state.last_cursor = None;
                Some(canvas::Action::capture())
            }
            _ => None,
        }
    }

    fn draw(
        &self,
        state: &State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let scale = if state.scale == 0.0 { 1.0 } else { state.scale };
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        frame.fill_rectangle(Point::ORIGIN, bounds.size(), BACKGROUND);
        let positions = self
            .graph
            .nodes
            .iter()
            .map(|node| {
                (
                    node.id,
                    node_position(node.x, node.y, bounds, state.offset, scale),
                )
            })
            .collect::<HashMap<_, _>>();

        for edge in &self.graph.edges {
            let Some(target) = edge.target else { continue };
            let (Some(source), Some(target)) =
                (positions.get(&edge.source), positions.get(&target))
            else {
                continue;
            };
            let line = canvas::Path::line(*source, *target);
            frame.stroke(
                &line,
                canvas::Stroke::default()
                    .with_color(Color::from_rgba(0.55, 0.52, 0.75, 0.32))
                    .with_width(1.2),
            );
        }

        let hovered = cursor
            .position_in(bounds)
            .and_then(|position| hit_node(self.graph, state, bounds, position));
        for node in &self.graph.nodes {
            let position = positions[&node.id];
            let active = self.active == Some(node.id);
            let hovered = hovered == Some(node.id);
            let radius =
                (6.0 + (node.degree as f32 + 1.0).sqrt() * 1.8) * if active { 1.25 } else { 1.0 };
            let halo = canvas::Path::circle(position, radius + 5.0);
            if active || hovered {
                frame.fill(&halo, Color::from_rgba(0.40, 0.35, 0.95, 0.18));
            }
            let circle = canvas::Path::circle(position, radius);
            frame.fill(&circle, if active { INDIGO_LIGHT } else { SURFACE });
            frame.stroke(
                &circle,
                canvas::Stroke::default()
                    .with_color(if active { INDIGO_LIGHT } else { BORDER })
                    .with_width(if active { 2.0 } else { 1.0 }),
            );

            if active || hovered || self.graph.nodes.len() <= 80 {
                frame.fill_text(canvas::Text {
                    content: node.title.clone(),
                    position: Point::new(position.x, position.y + radius + 7.0),
                    color: if active { TEXT } else { MUTED },
                    size: 12.0.into(),
                    line_height: text::LineHeight::default(),
                    font: Font::DEFAULT,
                    align_x: text::Alignment::Center,
                    align_y: alignment::Vertical::Top,
                    shaping: text::Shaping::Advanced,
                    max_width: 180.0,
                });
            }
        }
        vec![frame.into_geometry()]
    }

    fn mouse_interaction(
        &self,
        state: &State,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        let Some(position) = cursor.position_in(bounds) else {
            return mouse::Interaction::default();
        };
        if state.dragging {
            mouse::Interaction::Grabbing
        } else if hit_node(self.graph, state, bounds, position).is_some() {
            mouse::Interaction::Pointer
        } else {
            mouse::Interaction::Grab
        }
    }
}

fn node_position(x: f32, y: f32, bounds: Rectangle, offset: Vector, scale: f32) -> Point {
    let spread = bounds.width.min(bounds.height) * 0.42 * scale;
    Point::new(
        bounds.width / 2.0 + offset.x + x * spread,
        bounds.height / 2.0 + offset.y + y * spread,
    )
}

fn hit_node(
    graph: &KnowledgeGraph,
    state: &State,
    bounds: Rectangle,
    position: Point,
) -> Option<Uuid> {
    let scale = if state.scale == 0.0 { 1.0 } else { state.scale };
    graph.nodes.iter().find_map(|node| {
        let center = node_position(node.x, node.y, bounds, state.offset, scale);
        let dx = center.x - position.x;
        let dy = center.y - position.y;
        let radius = 11.0 + (node.degree as f32 + 1.0).sqrt();
        (dx * dx + dy * dy <= radius * radius).then_some(node.id)
    })
}
