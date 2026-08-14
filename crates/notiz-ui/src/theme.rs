use iced::widget::{button, container, text_input};
use iced::{Background, Border, Color, Shadow, Theme};

pub const BACKGROUND: Color = Color::from_rgb(0.035, 0.035, 0.043);
pub const SIDEBAR: Color = Color::from_rgb(0.026, 0.026, 0.033);
pub const SURFACE: Color = Color::from_rgb(0.082, 0.082, 0.10);
pub const SURFACE_HOVER: Color = Color::from_rgb(0.12, 0.12, 0.145);
pub const BORDER: Color = Color::from_rgb(0.15, 0.15, 0.18);
pub const TEXT: Color = Color::from_rgb(0.90, 0.90, 0.93);
pub const MUTED: Color = Color::from_rgb(0.56, 0.56, 0.63);
pub const DIM: Color = Color::from_rgb(0.40, 0.40, 0.47);
pub const INDIGO: Color = Color::from_rgb(0.40, 0.35, 0.95);
pub const INDIGO_LIGHT: Color = Color::from_rgb(0.59, 0.62, 1.0);
pub const SUCCESS: Color = Color::from_rgb(0.20, 0.83, 0.60);
pub const DANGER: Color = Color::from_rgb(0.94, 0.30, 0.32);

pub fn panel(background: Color, border: Color, radius: f32) -> container::Style {
    container::Style {
        text_color: Some(TEXT),
        background: Some(Background::Color(background)),
        border: Border {
            color: border,
            width: if border == Color::TRANSPARENT {
                0.0
            } else {
                1.0
            },
            radius: radius.into(),
        },
        shadow: Shadow::default(),
        snap: true,
    }
}

pub fn note_button(active: bool, status: button::Status) -> button::Style {
    let background = if active {
        Color::from_rgba(0.40, 0.35, 0.95, 0.20)
    } else if matches!(status, button::Status::Hovered) {
        SURFACE_HOVER
    } else {
        Color::TRANSPARENT
    };
    button::Style {
        background: Some(Background::Color(background)),
        text_color: if active { TEXT } else { MUTED },
        border: Border {
            color: if active { INDIGO } else { Color::TRANSPARENT },
            width: if active { 1.0 } else { 0.0 },
            radius: 8.0.into(),
        },
        shadow: Shadow::default(),
        snap: true,
    }
}

pub fn note_surface(
    active: bool,
    hovered: bool,
    dragging: bool,
    drag_progress: f32,
) -> container::Style {
    panel(
        if dragging {
            Color::from_rgba(0.40, 0.35, 0.95, 0.10 + drag_progress * 0.14)
        } else if active {
            Color::from_rgba(0.40, 0.35, 0.95, 0.16)
        } else if hovered {
            SURFACE_HOVER
        } else {
            Color::TRANSPARENT
        },
        if dragging {
            Color {
                a: 0.35 + drag_progress * 0.65,
                ..INDIGO_LIGHT
            }
        } else {
            Color::TRANSPARENT
        },
        8.0,
    )
}

pub fn tab_surface(active: bool, dragging: bool) -> container::Style {
    panel(
        if active || dragging {
            SURFACE
        } else {
            BACKGROUND
        },
        if dragging {
            INDIGO_LIGHT
        } else if active {
            BORDER
        } else {
            Color::TRANSPARENT
        },
        12.0,
    )
}

pub fn transparent_button(_theme: &Theme, status: button::Status) -> button::Style {
    button::Style {
        background: Some(Background::Color(
            if matches!(status, button::Status::Hovered) {
                SURFACE_HOVER
            } else {
                Color::TRANSPARENT
            },
        )),
        text_color: MUTED,
        border: Border::default(),
        shadow: Shadow::default(),
        snap: true,
    }
}

pub fn palette_item(active: bool, status: button::Status) -> button::Style {
    button::Style {
        background: Some(Background::Color(if active {
            SURFACE
        } else if matches!(status, button::Status::Hovered) {
            SURFACE_HOVER
        } else {
            Color::TRANSPARENT
        })),
        text_color: TEXT,
        border: Border {
            color: if active {
                Color::from_rgb(0.24, 0.24, 0.29)
            } else {
                Color::TRANSPARENT
            },
            width: if active { 1.0 } else { 0.0 },
            radius: 8.0.into(),
        },
        shadow: Shadow::default(),
        snap: true,
    }
}

pub fn ghost_button(_theme: &Theme, status: button::Status) -> button::Style {
    button::Style {
        background: Some(Background::Color(
            if matches!(status, button::Status::Hovered) {
                SURFACE_HOVER
            } else {
                Color::TRANSPARENT
            },
        )),
        text_color: TEXT,
        border: Border {
            color: BORDER,
            width: 1.0,
            radius: 8.0.into(),
        },
        shadow: Shadow::default(),
        snap: true,
    }
}

pub fn primary_button(_theme: &Theme, status: button::Status) -> button::Style {
    button::Style {
        background: Some(Background::Color(
            if matches!(status, button::Status::Hovered) {
                INDIGO_LIGHT
            } else {
                INDIGO
            },
        )),
        text_color: Color::WHITE,
        border: Border {
            radius: 8.0.into(),
            ..Border::default()
        },
        shadow: Shadow::default(),
        snap: true,
    }
}

pub fn danger_button(_theme: &Theme, status: button::Status) -> button::Style {
    button::Style {
        background: Some(Background::Color(
            if matches!(status, button::Status::Hovered) {
                Color::from_rgba(0.94, 0.30, 0.32, 0.20)
            } else {
                Color::TRANSPARENT
            },
        )),
        text_color: DANGER,
        border: Border {
            color: Color::from_rgba(0.94, 0.30, 0.32, 0.45),
            width: 1.0,
            radius: 8.0.into(),
        },
        shadow: Shadow::default(),
        snap: true,
    }
}

pub fn input(_theme: &Theme, status: text_input::Status) -> text_input::Style {
    let focused = matches!(status, text_input::Status::Focused { .. });
    text_input::Style {
        background: Background::Color(SURFACE),
        border: Border {
            color: if focused { INDIGO } else { BORDER },
            width: 1.0,
            radius: 8.0.into(),
        },
        icon: MUTED,
        placeholder: DIM,
        value: TEXT,
        selection: Color::from_rgba(0.40, 0.35, 0.95, 0.45),
    }
}

pub fn palette_input(_theme: &Theme, status: text_input::Status) -> text_input::Style {
    let focused = matches!(status, text_input::Status::Focused { .. });
    text_input::Style {
        background: Background::Color(SURFACE),
        border: Border {
            color: if focused {
                Color::from_rgb(0.30, 0.30, 0.36)
            } else {
                BORDER
            },
            width: 1.0,
            radius: 8.0.into(),
        },
        icon: MUTED,
        placeholder: DIM,
        value: TEXT,
        selection: Color::from_rgba(0.45, 0.45, 0.52, 0.40),
    }
}
