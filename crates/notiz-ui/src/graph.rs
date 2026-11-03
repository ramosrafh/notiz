use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use iced::advanced::text;
use iced::mouse;
use iced::widget::canvas;
use iced::{Color, Element, Font, Length, Point, Rectangle, Renderer, Theme, Vector, alignment};
use notiz_core::{GraphNode, KnowledgeGraph};
use uuid::Uuid;

use crate::theme::{BACKGROUND, DIM, INDIGO, MUTED, TEXT};

#[derive(Debug, Clone, PartialEq)]
pub enum Message {
    SelectNode(Uuid),
    OpenNote(Uuid),
    NodeMoved(Uuid, f32, f32),
}

#[derive(Debug, Clone, Copy)]
struct NodeDrag {
    id: Uuid,
    origin: Point,
    grab_offset: Vector,
    position: Point,
    moved: bool,
}

#[derive(Debug, Default)]
pub struct State {
    scale: f32,
    offset: Vector,
    panning: bool,
    last_cursor: Option<Point>,
    node_drag: Option<NodeDrag>,
    last_click: Option<(Uuid, Instant)>,
    view_revision: u64,
}

pub fn view<'a>(
    graph: &'a KnowledgeGraph,
    selected: Option<Uuid>,
    view_revision: u64,
) -> Element<'a, Message> {
    canvas(GraphProgram {
        graph,
        selected,
        view_revision,
    })
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

struct GraphProgram<'a> {
    graph: &'a KnowledgeGraph,
    selected: Option<Uuid>,
    view_revision: u64,
}

impl State {
    fn synchronize(&mut self, revision: u64) {
        if self.view_revision != revision {
            *self = Self {
                view_revision: revision,
                ..Self::default()
            };
        }
        if self.scale == 0.0 {
            self.scale = 1.0;
        }
    }

    fn scale(&self) -> f32 {
        if self.scale == 0.0 { 1.0 } else { self.scale }
    }
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
        state.synchronize(self.view_revision);
        let position = cursor.position_in(bounds);
        match event {
            canvas::Event::Mouse(mouse::Event::WheelScrolled { delta }) if position.is_some() => {
                // Do not change the transform halfway through a drag.
                if state.node_drag.is_some() || state.panning {
                    return Some(canvas::Action::capture());
                }
                let exponent = match delta {
                    mouse::ScrollDelta::Lines { y, .. } => *y * 0.14,
                    mouse::ScrollDelta::Pixels { y, .. } => *y * 0.004,
                };
                let old_scale = state.scale();
                let new_scale = (old_scale * exponent.exp()).clamp(0.30, 5.0);
                let cursor = position?;
                let from_center = Vector::new(
                    cursor.x - bounds.width / 2.0,
                    cursor.y - bounds.height / 2.0,
                );
                state.offset = from_center - (from_center - state.offset) * (new_scale / old_scale);
                state.scale = new_scale;
                Some(canvas::Action::request_redraw().and_capture())
            }
            canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                let position = position?;
                if let Some(id) = hit_node(self.graph, state, bounds, position) {
                    let node = self.graph.nodes.iter().find(|node| node.id == id)?;
                    let world = world_position(position, bounds, state.offset, state.scale());
                    state.node_drag = Some(NodeDrag {
                        id,
                        origin: position,
                        grab_offset: Point::new(node.x, node.y) - world,
                        position: Point::new(node.x, node.y),
                        moved: false,
                    });
                } else {
                    state.panning = true;
                    state.last_cursor = Some(position);
                    state.last_click = None;
                }
                Some(canvas::Action::request_redraw().and_capture())
            }
            canvas::Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                // position_from keeps a drag alive outside the canvas until release.
                let current = cursor.position_from(Point::new(bounds.x, bounds.y));
                if let Some(mut drag) = state.node_drag {
                    let current = current?;
                    drag.moved |= current.distance(drag.origin) > 4.0;
                    if drag.moved {
                        drag.position =
                            world_position(current, bounds, state.offset, state.scale())
                                + drag.grab_offset;
                    }
                    state.node_drag = Some(drag);
                    Some(canvas::Action::request_redraw().and_capture())
                } else if state.panning {
                    let current = current?;
                    if let Some(previous) = state.last_cursor {
                        state.offset += current - previous;
                    }
                    state.last_cursor = Some(current);
                    Some(canvas::Action::request_redraw().and_capture())
                } else if position.is_some() {
                    Some(canvas::Action::request_redraw())
                } else {
                    None
                }
            }
            canvas::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                if let Some(drag) = state.node_drag.take() {
                    if drag.moved {
                        state.last_click = None;
                        return Some(
                            canvas::Action::publish(Message::NodeMoved(
                                drag.id,
                                drag.position.x,
                                drag.position.y,
                            ))
                            .and_capture(),
                        );
                    }
                    let now = Instant::now();
                    let double_click = state.last_click.is_some_and(|(id, time)| {
                        id == drag.id && now.duration_since(time) < Duration::from_millis(400)
                    });
                    state.last_click = if double_click {
                        None
                    } else {
                        Some((drag.id, now))
                    };
                    return Some(
                        canvas::Action::publish(if double_click {
                            Message::OpenNote(drag.id)
                        } else {
                            Message::SelectNode(drag.id)
                        })
                        .and_capture(),
                    );
                }
                if state.panning {
                    state.panning = false;
                    state.last_cursor = None;
                    return Some(canvas::Action::request_redraw().and_capture());
                }
                None
            }
            canvas::Event::Window(iced::window::Event::Unfocused) => {
                state.panning = false;
                state.last_cursor = None;
                state.node_drag = None;
                state.last_click = None;
                Some(canvas::Action::request_redraw())
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
        let reset = State::default();
        let state = if state.view_revision == self.view_revision {
            state
        } else {
            &reset
        };
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        frame.fill_rectangle(Point::ORIGIN, bounds.size(), BACKGROUND);
        let positions = self
            .graph
            .nodes
            .iter()
            .map(|node| {
                let world = displayed_position(node, state);
                (
                    node.id,
                    node_position(world.x, world.y, bounds, state.offset, state.scale()),
                )
            })
            .collect::<HashMap<_, _>>();
        let hovered = state.node_drag.map(|drag| drag.id).or_else(|| {
            cursor
                .position_in(bounds)
                .and_then(|position| hit_node(self.graph, state, bounds, position))
        });
        let focus = hovered.or(self.selected);
        let neighbors = focus
            .map(|id| neighbors(self.graph, id))
            .unwrap_or_default();
        for edge in &self.graph.edges {
            let Some(target_id) = edge.target else {
                continue;
            };
            let (Some(source), Some(target)) =
                (positions.get(&edge.source), positions.get(&target_id))
            else {
                continue;
            };
            let highlighted = focus.is_some_and(|id| id == edge.source || id == target_id);
            let color = if highlighted {
                Color { a: 0.9, ..INDIGO }
            } else {
                Color {
                    a: if focus.is_some() { 0.12 } else { 0.50 },
                    ..MUTED
                }
            };
            frame.stroke(
                &canvas::Path::line(*source, *target),
                canvas::Stroke::default()
                    .with_color(color)
                    .with_width(if highlighted { 2.2 } else { 1.3 }),
            );
            // Arrowheads communicate the direction of a [[link]], without hiding reciprocal links.
            if highlighted && source.distance(*target) > 30.0 {
                let delta = *target - *source;
                let length = (delta.x * delta.x + delta.y * delta.y).sqrt();
                let unit = delta * (1.0 / length);
                let tip = *source + delta * 0.72;
                let base = tip - unit * 8.0;
                let normal = Vector::new(-unit.y, unit.x) * 4.0;
                frame.stroke(
                    &canvas::Path::new(|path| {
                        path.move_to(base + normal);
                        path.line_to(tip);
                        path.line_to(base - normal);
                    }),
                    canvas::Stroke::default().with_color(color).with_width(1.8),
                );
            }
        }
        for node in &self.graph.nodes {
            let position = positions[&node.id];
            let focused = focus == Some(node.id);
            let related = neighbors.contains(&node.id);
            let selected = self.selected == Some(node.id);
            let radius = node_radius(node, state.scale());
            let dimmed = focus.is_some() && !focused && !related;
            if focused || selected {
                frame.fill(
                    &canvas::Path::circle(position, radius + 6.0 * state.scale()),
                    Color { a: 0.12, ..INDIGO },
                );
            }
            let color = if focused || selected {
                INDIGO
            } else if related {
                TEXT
            } else if dimmed {
                DIM
            } else {
                MUTED
            };
            frame.fill(&canvas::Path::circle(position, radius), color);
            if focused || related || selected || self.graph.nodes.len() <= 100 {
                frame.fill_text(canvas::Text {
                    content: node.title.clone(),
                    position: Point::new(position.x, position.y + radius + 8.0),
                    color: if focused || related || selected {
                        TEXT
                    } else if dimmed {
                        DIM
                    } else {
                        MUTED
                    },
                    size: (12.0 * state.scale().sqrt()).clamp(10.0, 18.0).into(),
                    line_height: text::LineHeight::default(),
                    font: Font::with_name("Inter"),
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
        if state.panning || state.node_drag.is_some() {
            return mouse::Interaction::Grabbing;
        }
        if cursor.position_in(bounds).is_some() {
            mouse::Interaction::Grab
        } else {
            mouse::Interaction::default()
        }
    }
}

pub fn neighbors(graph: &KnowledgeGraph, id: Uuid) -> HashSet<Uuid> {
    graph
        .edges
        .iter()
        .filter_map(|edge| {
            if edge.source == id {
                edge.target
            } else if edge.target == Some(id) {
                Some(edge.source)
            } else {
                None
            }
        })
        .collect()
}

fn displayed_position(node: &GraphNode, state: &State) -> Point {
    state
        .node_drag
        .filter(|drag| drag.id == node.id)
        .map(|drag| drag.position)
        .unwrap_or(Point::new(node.x, node.y))
}

fn node_radius(node: &GraphNode, scale: f32) -> f32 {
    (6.0 + (node.degree as f32 + 1.0).sqrt() * 1.8).min(22.0) * scale
}

fn spread(bounds: Rectangle, scale: f32) -> f32 {
    (bounds.width.min(bounds.height) * 0.46 * scale).max(1.0)
}

fn node_position(x: f32, y: f32, bounds: Rectangle, offset: Vector, scale: f32) -> Point {
    let spread = spread(bounds, scale);
    Point::new(
        bounds.width / 2.0 + offset.x + x * spread,
        bounds.height / 2.0 + offset.y + y * spread,
    )
}

fn world_position(point: Point, bounds: Rectangle, offset: Vector, scale: f32) -> Point {
    let spread = spread(bounds, scale);
    Point::new(
        (point.x - bounds.width / 2.0 - offset.x) / spread,
        (point.y - bounds.height / 2.0 - offset.y) / spread,
    )
}

fn hit_node(
    graph: &KnowledgeGraph,
    state: &State,
    bounds: Rectangle,
    position: Point,
) -> Option<Uuid> {
    graph.nodes.iter().rev().find_map(|node| {
        let world = displayed_position(node, state);
        let center = node_position(world.x, world.y, bounds, state.offset, state.scale());
        // Keep the extra hit tolerance in screen pixels at every zoom level.
        (center.distance(position) <= node_radius(node, state.scale()) + 5.0).then_some(node.id)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::widget::canvas::Program;
    use notiz_core::GraphEdge;

    fn fixture() -> KnowledgeGraph {
        let nodes = (0..3)
            .map(|index| GraphNode {
                id: Uuid::new_v4(),
                title: format!("Note {index}"),
                relative_path: format!("note-{index}.md"),
                tags: vec![],
                x: index as f32 * 0.5,
                y: 0.0,
                degree: 1,
            })
            .collect::<Vec<_>>();
        let edges = vec![
            GraphEdge {
                source: nodes[0].id,
                target: Some(nodes[1].id),
                unresolved_target: None,
            },
            GraphEdge {
                source: nodes[2].id,
                target: Some(nodes[0].id),
                unresolved_target: None,
            },
            GraphEdge {
                source: nodes[0].id,
                target: None,
                unresolved_target: Some("Missing".into()),
            },
        ];
        KnowledgeGraph { nodes, edges }
    }

    fn bounds() -> Rectangle {
        Rectangle {
            x: 20.0,
            y: 30.0,
            width: 800.0,
            height: 600.0,
        }
    }

    fn event(
        program: &GraphProgram<'_>,
        state: &mut State,
        event: mouse::Event,
        local: Point,
    ) -> Option<Message> {
        let bounds = bounds();
        let cursor = mouse::Cursor::Available(Point::new(local.x + bounds.x, local.y + bounds.y));
        program
            .update(state, &canvas::Event::Mouse(event), bounds, cursor)
            .and_then(|action| action.into_inner().0)
    }

    #[test]
    fn click_selects_on_release_and_double_click_opens_note() {
        let graph = fixture();
        let program = GraphProgram {
            graph: &graph,
            selected: None,
            view_revision: 0,
        };
        let mut state = State::default();
        let center = Point::new(400.0, 300.0);
        assert_eq!(
            event(
                &program,
                &mut state,
                mouse::Event::ButtonPressed(mouse::Button::Left),
                center
            ),
            None
        );
        assert_eq!(
            event(
                &program,
                &mut state,
                mouse::Event::ButtonReleased(mouse::Button::Left),
                center
            ),
            Some(Message::SelectNode(graph.nodes[0].id))
        );
        let _ = event(
            &program,
            &mut state,
            mouse::Event::ButtonPressed(mouse::Button::Left),
            center,
        );
        assert_eq!(
            event(
                &program,
                &mut state,
                mouse::Event::ButtonReleased(mouse::Button::Left),
                center
            ),
            Some(Message::OpenNote(graph.nodes[0].id))
        );
    }

    #[test]
    fn drag_moves_node_in_world_coordinates_without_opening_it() {
        let graph = fixture();
        let program = GraphProgram {
            graph: &graph,
            selected: None,
            view_revision: 0,
        };
        let mut state = State {
            scale: 2.0,
            offset: Vector::new(30.0, -20.0),
            ..State::default()
        };
        let center = node_position(0.0, 0.0, bounds(), state.offset, state.scale());
        let _ = event(
            &program,
            &mut state,
            mouse::Event::ButtonPressed(mouse::Button::Left),
            center,
        );
        let destination = center + Vector::new(100.0, 50.0);
        let _ = event(
            &program,
            &mut state,
            mouse::Event::CursorMoved {
                position: destination,
            },
            destination,
        );
        assert_eq!(
            hit_node(&graph, &state, bounds(), destination),
            Some(graph.nodes[0].id)
        );
        let message = event(
            &program,
            &mut state,
            mouse::Event::ButtonReleased(mouse::Button::Left),
            destination,
        );
        let Some(Message::NodeMoved(id, x, y)) = message else {
            panic!("drag must only publish a move")
        };
        assert_eq!(id, graph.nodes[0].id);
        assert!((x - 100.0 / 552.0).abs() < 0.0001);
        assert!((y - 50.0 / 552.0).abs() < 0.0001);
        assert!(state.node_drag.is_none());
        assert!(state.last_click.is_none());
    }

    #[test]
    fn panning_background_does_not_move_nodes_and_release_outside_stops_drag() {
        let graph = fixture();
        let program = GraphProgram {
            graph: &graph,
            selected: None,
            view_revision: 0,
        };
        let mut state = State::default();
        let _ = event(
            &program,
            &mut state,
            mouse::Event::ButtonPressed(mouse::Button::Left),
            Point::new(10.0, 10.0),
        );
        let _ = event(
            &program,
            &mut state,
            mouse::Event::CursorMoved {
                position: Point::ORIGIN,
            },
            Point::new(60.0, 30.0),
        );
        assert_eq!(state.offset, Vector::new(50.0, 20.0));
        assert!(state.node_drag.is_none());
        program.update(
            &mut state,
            &canvas::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
            bounds(),
            mouse::Cursor::Unavailable,
        );
        assert!(!state.panning);
    }

    #[test]
    fn zoom_keeps_world_point_under_cursor_and_reset_restores_view() {
        let graph = fixture();
        let mut program = GraphProgram {
            graph: &graph,
            selected: None,
            view_revision: 0,
        };
        let mut state = State::default();
        let cursor = Point::new(100.0, 90.0);
        let before = world_position(cursor, bounds(), state.offset, state.scale());
        let _ = event(
            &program,
            &mut state,
            mouse::Event::WheelScrolled {
                delta: mouse::ScrollDelta::Lines { x: 0.0, y: 2.0 },
            },
            cursor,
        );
        let after = world_position(cursor, bounds(), state.offset, state.scale());
        assert!(before.distance(after) < 0.0001);
        program.view_revision += 1;
        state.synchronize(program.view_revision);
        assert_eq!(state.scale(), 1.0);
        assert_eq!(state.offset, Vector::ZERO);
    }

    #[test]
    fn node_radius_and_hit_area_follow_zoom_with_screen_pixel_tolerance() {
        let mut graph = fixture();
        graph.nodes.truncate(1);
        for degree in [1, 1000] {
            graph.nodes[0].degree = degree;
            let node = &graph.nodes[0];
            let base_radius = node_radius(node, State::default().scale());
            if degree == 1000 {
                assert_eq!(base_radius, 22.0);
            }
            for scale in [0.20, 0.5, 1.0, 2.0, 7.0] {
                let state = State {
                    scale,
                    offset: Vector::new(30.0, -20.0),
                    ..State::default()
                };
                let center = node_position(node.x, node.y, bounds(), state.offset, state.scale());
                let radius = node_radius(node, state.scale());
                assert!((radius - base_radius * scale).abs() < 0.0001);
                for distance in [radius - 0.25, radius + 4.75] {
                    assert_eq!(
                        hit_node(
                            &graph,
                            &state,
                            bounds(),
                            center + Vector::new(distance, 0.0)
                        ),
                        Some(node.id),
                        "visible node and screen-pixel tolerance must hit at scale {scale}"
                    );
                }
                assert_eq!(
                    hit_node(
                        &graph,
                        &state,
                        bounds(),
                        center + Vector::new(radius + 5.25, 0.0)
                    ),
                    None,
                    "hit tolerance must stay at five screen pixels at scale {scale}"
                );
            }
        }
    }

    #[test]
    fn neighbors_include_incoming_and_outgoing_but_not_unresolved_links() {
        let graph = fixture();
        assert_eq!(
            neighbors(&graph, graph.nodes[0].id),
            HashSet::from([graph.nodes[1].id, graph.nodes[2].id])
        );
    }
}
