//! The colors a terminal draws with: the 16 ANSI colors of the theme, its
//! text and background, and the 256-color table programs pick from.

use alacritty_terminal::term::color::Colors;
use alacritty_terminal::vte::ansi::{Color, NamedColor};

/// A color as red, green and blue.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb {
    pub const fn hex(value: u32) -> Self {
        Self {
            r: (value >> 16) as u8,
            g: (value >> 8) as u8,
            b: value as u8,
        }
    }

    /// The faint version of a color, for text programs ask to dim.
    pub fn dim(self) -> Self {
        let scale = |value: u8| (value as f32 * 0.66).round() as u8;
        Self {
            r: scale(self.r),
            g: scale(self.g),
            b: scale(self.b),
        }
    }
}

impl From<Rgb> for alacritty_terminal::vte::ansi::Rgb {
    fn from(color: Rgb) -> Self {
        Self {
            r: color.r,
            g: color.g,
            b: color.b,
        }
    }
}

impl From<alacritty_terminal::vte::ansi::Rgb> for Rgb {
    fn from(color: alacritty_terminal::vte::ansi::Rgb) -> Self {
        Self {
            r: color.r,
            g: color.g,
            b: color.b,
        }
    }
}

/// The theme's colors for a terminal.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Palette {
    /// Black, red, green, yellow, blue, magenta, cyan, white, then the
    /// bright eight in the same order.
    pub ansi: [Rgb; 16],
    pub foreground: Rgb,
    pub background: Rgb,
    pub cursor: Rgb,
}

impl Palette {
    /// The ANSI colors of VS Code's dark themes.
    pub fn dark(foreground: Rgb, background: Rgb) -> Self {
        Self {
            ansi: [
                Rgb::hex(0x000000),
                Rgb::hex(0xcd3131),
                Rgb::hex(0x0dbc79),
                Rgb::hex(0xe5e510),
                Rgb::hex(0x2472c8),
                Rgb::hex(0xbc3fbc),
                Rgb::hex(0x11a8cd),
                Rgb::hex(0xe5e5e5),
                Rgb::hex(0x666666),
                Rgb::hex(0xf14c4c),
                Rgb::hex(0x23d18b),
                Rgb::hex(0xf5f543),
                Rgb::hex(0x3b8eea),
                Rgb::hex(0xd670d6),
                Rgb::hex(0x29b8db),
                Rgb::hex(0xe5e5e5),
            ],
            foreground,
            background,
            cursor: foreground,
        }
    }

    /// The ANSI colors of VS Code's light themes.
    pub fn light(foreground: Rgb, background: Rgb) -> Self {
        Self {
            ansi: [
                Rgb::hex(0x000000),
                Rgb::hex(0xcd3131),
                Rgb::hex(0x00bc00),
                Rgb::hex(0x949800),
                Rgb::hex(0x0451a5),
                Rgb::hex(0xbc05bc),
                Rgb::hex(0x0598bc),
                Rgb::hex(0x555555),
                Rgb::hex(0x666666),
                Rgb::hex(0xcd3131),
                Rgb::hex(0x14ce14),
                Rgb::hex(0xb5ba00),
                Rgb::hex(0x0451a5),
                Rgb::hex(0xbc05bc),
                Rgb::hex(0x0598bc),
                Rgb::hex(0xa5a5a5),
            ],
            foreground,
            background,
            cursor: foreground,
        }
    }

    /// The color at a place of the 269-color table, for programs that ask
    /// which color they are drawing with.
    pub fn indexed(&self, index: usize) -> Rgb {
        match index {
            0..16 => self.ansi[index],
            16..232 => {
                let index = index - 16;
                let level = |value: usize| {
                    if value == 0 {
                        0
                    } else {
                        (55 + value * 40) as u8
                    }
                };
                Rgb {
                    r: level(index / 36),
                    g: level((index / 6) % 6),
                    b: level(index % 6),
                }
            }
            232..256 => {
                let value = (8 + (index - 232) * 10) as u8;
                Rgb {
                    r: value,
                    g: value,
                    b: value,
                }
            }
            256 | 267 => self.foreground,
            257 => self.background,
            258 => self.cursor,
            259..267 => self.ansi[index - 259].dim(),
            268 => self.foreground.dim(),
            _ => self.foreground,
        }
    }

    /// A cell's color, with the colors a program changed (OSC 4, 10, 11)
    /// taking the place of the theme's.
    pub(crate) fn resolve(&self, color: Color, changed: &Colors) -> Rgb {
        let index = match color {
            Color::Spec(rgb) => return rgb.into(),
            Color::Indexed(index) => index as usize,
            Color::Named(named) => named as usize,
        };
        match changed[index] {
            Some(rgb) => rgb.into(),
            None => self.indexed(index),
        }
    }

    pub(crate) fn is_default_background(color: Color) -> bool {
        matches!(color, Color::Named(NamedColor::Background))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_256_color_table_follows_xterm() {
        let palette = Palette::dark(Rgb::hex(0xffffff), Rgb::hex(0x101010));
        assert_eq!(palette.indexed(1), Rgb::hex(0xcd3131));
        assert_eq!(palette.indexed(16), Rgb::hex(0x000000));
        assert_eq!(palette.indexed(21), Rgb::hex(0x0000ff));
        assert_eq!(palette.indexed(196), Rgb::hex(0xff0000));
        assert_eq!(palette.indexed(232), Rgb::hex(0x080808));
        assert_eq!(palette.indexed(255), Rgb::hex(0xeeeeee));
        assert_eq!(palette.indexed(257), Rgb::hex(0x101010));
    }

    #[test]
    fn changed_colors_win() {
        let palette = Palette::dark(Rgb::hex(0xffffff), Rgb::hex(0x101010));
        let mut changed = Colors::default();
        changed[NamedColor::Red] = Some(Rgb::hex(0x123456).into());
        assert_eq!(
            palette.resolve(Color::Named(NamedColor::Red), &changed),
            Rgb::hex(0x123456)
        );
        assert_eq!(
            palette.resolve(Color::Indexed(2), &changed),
            Rgb::hex(0x0dbc79)
        );
    }
}
