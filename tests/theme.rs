//! Public-API coverage for theme parsing and terminal color downgrades.

use commet::config::{ThemeName, Ui};
use commet::tui::theme::{
    ColorCap, DEFAULT, DRACULA, MONO, SOLARIZED_DARK, SOLARIZED_LIGHT, Theme, parse_color,
};
use ratatui::style::Color;

fn ui_with_theme(theme: ThemeName) -> Ui {
    Ui {
        theme,
        ..Ui::default()
    }
}

fn resolve(theme: ThemeName, cap: ColorCap) -> Theme {
    Theme::from_config(&ui_with_theme(theme), cap).expect("builtin theme should resolve")
}

fn roles(theme: Theme) -> [Color; 11] {
    [
        theme.fg,
        theme.bg,
        theme.accent,
        theme.success,
        theme.warning,
        theme.error,
        theme.muted,
        theme.diff_add,
        theme.diff_del,
        theme.diff_meta,
        theme.border,
    ]
}

#[test]
fn hex_parser_accepts_valid_and_rejects_invalid_values() {
    assert_eq!(
        parse_color(" #7Aa2F7 ").unwrap(),
        Color::Rgb(0x7a, 0xa2, 0xf7)
    );
    assert_eq!(parse_color("#000000").unwrap(), Color::Rgb(0, 0, 0));
    assert_eq!(
        parse_color("#FFFFFF").unwrap(),
        Color::Rgb(0xff, 0xff, 0xff)
    );

    for invalid in ["#xyz", "#7aa2f", "#7aa2f7a", "7aa2f7"] {
        let error = parse_color(invalid).unwrap_err();
        assert_eq!(error.input, invalid);
    }
}

#[test]
fn truecolor_keeps_rgb_values() {
    assert_eq!(
        resolve(ThemeName::Default, ColorCap::TrueColor).accent,
        Color::Rgb(0x7a, 0xa2, 0xf7)
    );
}

#[test]
fn ansi256_maps_rgb_to_known_palette_index() {
    assert_eq!(
        resolve(ThemeName::Default, ColorCap::Ansi256).accent,
        Color::Indexed(111)
    );
}

#[test]
fn ansi16_maps_rgb_to_known_named_color() {
    assert_eq!(
        resolve(ThemeName::Default, ColorCap::Ansi16).accent,
        Color::LightBlue
    );
}

#[test]
fn no_color_resets_every_role_in_every_builtin_theme() {
    for name in [
        ThemeName::Default,
        ThemeName::Mono,
        ThemeName::Dracula,
        ThemeName::SolarizedDark,
        ThemeName::SolarizedLight,
    ] {
        let theme = resolve(name, ColorCap::None);
        assert!(
            roles(theme).into_iter().all(|color| color == Color::Reset),
            "{name:?} retained a color after downgrade: {theme:?}"
        );
    }
}

#[test]
fn every_builtin_theme_round_trips_at_truecolor() {
    assert_eq!(resolve(ThemeName::Default, ColorCap::TrueColor), DEFAULT);
    assert_eq!(resolve(ThemeName::Mono, ColorCap::TrueColor), MONO);
    assert_eq!(resolve(ThemeName::Dracula, ColorCap::TrueColor), DRACULA);
    assert_eq!(
        resolve(ThemeName::SolarizedDark, ColorCap::TrueColor),
        SOLARIZED_DARK
    );
    assert_eq!(
        resolve(ThemeName::SolarizedLight, ColorCap::TrueColor),
        SOLARIZED_LIGHT
    );
}
