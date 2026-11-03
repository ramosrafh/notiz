use iced::widget::{button, container, text_input};
use iced::{Background, Border, Color, Shadow, Theme};

pub const BACKGROUND: Color = Color::from_rgb8(0x10, 0x10, 0x10);
pub const SIDEBAR: Color = SURFACE;
pub const SURFACE: Color = Color::from_rgb8(0x16, 0x16, 0x16);
pub const SURFACE_HOVER: Color = Color::from_rgb8(0x23, 0x23, 0x23);
pub const SURFACE_ACTIVE: Color = Color::from_rgb8(0x28, 0x28, 0x28);
pub const BORDER: Color = Color::from_rgb8(0x34, 0x34, 0x34);
pub const TEXT: Color = Color::from_rgb8(0xFF, 0xFF, 0xFF);
pub const MUTED: Color = Color::from_rgb8(0xA0, 0xA0, 0xA0);
pub const DIM: Color = Color::from_rgb8(0x70, 0x70, 0x70);
pub const DISABLED: Color = Color::from_rgb8(0x50, 0x50, 0x50);
pub const INDIGO: Color = Color::from_rgb8(0xFF, 0xC7, 0x99);
pub const INDIGO_LIGHT: Color = INDIGO;
pub const SUCCESS: Color = Color::from_rgb8(0x99, 0xFF, 0xE4);
pub const DANGER: Color = Color::from_rgb8(0xFF, 0x80, 0x80);

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
        Color::from_rgba(1.0, 0.78, 0.60, 0.09)
    } else if matches!(status, button::Status::Hovered) {
        SURFACE_HOVER
    } else {
        Color::TRANSPARENT
    };
    button::Style {
        background: Some(Background::Color(background)),
        text_color: if active { TEXT } else { MUTED },
        border: Border {
            color: Color::TRANSPARENT,
            width: 0.0,
            radius: 6.0.into(),
        },
        shadow: Shadow::default(),
        snap: true,
    }
}

pub fn note_surface(
    active: bool,
    nested: bool,
    hovered: bool,
    dragging: bool,
    drag_progress: f32,
) -> container::Style {
    panel(
        if dragging {
            Color::from_rgba(1.0, 0.78, 0.60, 0.10 + drag_progress * 0.14)
        } else if active {
            SURFACE_ACTIVE
        } else if hovered {
            SURFACE_HOVER
        } else if nested {
            SURFACE
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

pub fn folder_surface(
    selected: bool,
    expanded: bool,
    hovered: bool,
    dragging: bool,
    drag_progress: f32,
) -> container::Style {
    panel(
        if dragging {
            Color::from_rgba(1.0, 0.78, 0.60, 0.08 + drag_progress * 0.12)
        } else if hovered {
            SURFACE_HOVER
        } else if selected {
            SURFACE_ACTIVE
        } else if expanded {
            SURFACE
        } else {
            Color::TRANSPARENT
        },
        if dragging {
            Color {
                a: 0.30 + drag_progress * 0.60,
                ..INDIGO_LIGHT
            }
        } else {
            Color::TRANSPARENT
        },
        7.0,
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
                SURFACE_ACTIVE
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
        background: matches!(status, button::Status::Hovered)
            .then_some(Background::Color(SURFACE_HOVER)),
        text_color: if matches!(status, button::Status::Hovered) {
            TEXT
        } else {
            MUTED
        },
        border: Border {
            radius: 10.0.into(),
            ..Border::default()
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
        text_color: BACKGROUND,
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
                Color::from_rgba(1.0, 0.50, 0.50, 0.20)
            } else {
                Color::TRANSPARENT
            },
        )),
        text_color: DANGER,
        border: Border {
            color: Color::from_rgba(1.0, 0.50, 0.50, 0.45),
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
        background: Background::Color(BACKGROUND),
        border: Border {
            color: if focused { INDIGO } else { BORDER },
            width: 1.0,
            radius: 8.0.into(),
        },
        icon: MUTED,
        placeholder: DISABLED,
        value: TEXT,
        selection: Color::from_rgba(1.0, 0.78, 0.60, 0.45),
    }
}

pub fn inline_input(_theme: &Theme, status: text_input::Status) -> text_input::Style {
    let focused = matches!(status, text_input::Status::Focused { .. });
    text_input::Style {
        background: Background::Color(Color::TRANSPARENT),
        border: Border {
            color: if focused { INDIGO } else { Color::TRANSPARENT },
            width: 1.0,
            radius: 7.0.into(),
        },
        icon: MUTED,
        placeholder: DISABLED,
        value: TEXT,
        selection: Color::from_rgba(1.0, 0.78, 0.60, 0.45),
    }
}

pub fn palette_input(_theme: &Theme, status: text_input::Status) -> text_input::Style {
    let focused = matches!(status, text_input::Status::Focused { .. });
    text_input::Style {
        background: Background::Color(BACKGROUND),
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
        placeholder: DISABLED,
        value: TEXT,
        selection: Color::from_rgba(1.0, 0.78, 0.60, 0.40),
    }
}
