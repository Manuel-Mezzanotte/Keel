//! The welcome of Keel's first start. Over the first window, a sky above
//! the sea: the stars or the clouds come out, the sea rises to the horizon,
//! Keel's boat sails in, its name appears, and then «Sali a bordo». Pressed,
//! the sea rises over the whole window and clears, and under it is the IDE.
//!
//! Only once, ever: a mark in the data folder remembers it, written when
//! the welcome ends. Help → Show the Welcome Again plays it again.
//!
//! The scene is painted with quads only (sky, stars, clouds, the waves as
//! thin columns) and the boat with small vector sprites: no paths, which
//! would make GPUI keep textures as large as the window.

use std::path::Path;
use std::time::Instant;

use gpui::{
    AnyElement, Bounds, FocusHandle, Hsla, KeyDownEvent, MouseButton, Pixels,
    StatefulInteractiveElement as _, Transformation, canvas, linear_color_stop, linear_gradient,
    point, radians, rgb, size, svg,
};
use gpui_component::{Icon, Sizable as _};
use gpui_kit_assets::IconName;

use super::*;

/// The file in the data folder that says the welcome was seen.
const SEEN_MARK: &str = "welcome-seen";

/// How long each part takes, in seconds from the start.
const SEA_IN: (f32, f32) = (0.25, 1.5);
const BOAT_IN: (f32, f32) = (0.7, 2.6);
const NAME_IN: (f32, f32) = (1.6, 0.9);
const LINE_IN: (f32, f32) = (2.0, 0.9);
const BUTTON_IN: (f32, f32) = (2.7, 0.7);
/// When everything has come in.
const ALL_IN: f32 = 3.4;
/// The way out, from the press of the button.
const OUT: f32 = 1.5;

/// Whether this is Keel's very first start on this Mac: nothing in the data
/// folder says Keel ran before, and the welcome was never seen.
pub(crate) fn first_start(data: &Path) -> bool {
    if std::env::var_os("KEEL_WELCOME").is_some() {
        return true;
    }
    !data.join(SEEN_MARK).exists()
        && !crate::paths::user_settings_file(data).exists()
        && !crate::paths::recent_projects_file(data).exists()
        && !data.join("windows.json").exists()
}

/// Remembers that the welcome was seen, so it never comes again.
fn remember_seen(data: &Path) {
    let _ = std::fs::create_dir_all(data);
    let _ = std::fs::write(data.join(SEEN_MARK), crate::version::VERSION);
}

/// Set at start when the first window is to open with the welcome.
pub(crate) struct WelcomePending;

impl gpui::Global for WelcomePending {}

/// The welcome on screen.
pub(super) struct Intro {
    started: Instant,
    /// When «Sali a bordo» was pressed.
    leaving: Option<Instant>,
    focus: FocusHandle,
    /// A moment held still, for the snapshots: from the start, and from
    /// the press of the button.
    held: Option<(f32, Option<f32>)>,
    /// The notices that came while it showed, shown when it ends.
    pub(super) waiting: Vec<super::notices::Notice>,
    /// The next frame, asked for a sixtieth of a second later.
    tick: Option<Task<()>>,
}

impl Intro {
    /// Seconds since the start, and since the button, if pressed.
    fn clock(&self) -> (f32, Option<f32>) {
        if let Some(held) = self.held {
            return held;
        }
        (
            self.started.elapsed().as_secs_f32(),
            self.leaving.map(|at| at.elapsed().as_secs_f32()),
        )
    }
}

/// How far along `(start, length)` the moment `t` is, from 0 to 1.
fn along(t: f32, (start, length): (f32, f32)) -> f32 {
    ((t - start) / length).clamp(0., 1.)
}

fn ease_out(x: f32) -> f32 {
    1. - (1. - x).powi(3)
}

fn ease_in_out(x: f32) -> f32 {
    if x < 0.5 {
        4. * x * x * x
    } else {
        1. - (-2. * x + 2.).powi(3) / 2.
    }
}

fn mix(a: f32, b: f32, x: f32) -> f32 {
    a + (b - a) * x
}

/// The colors of the scene: the night with a dark theme, the day with a
/// light one, as the README's banner.
#[derive(Clone, Copy)]
struct Palette {
    sky: [Hsla; 3],
    sea: [Hsla; 4],
    foam: Hsla,
    glow: Hsla,
    cloud: (Hsla, Hsla),
    stars: bool,
    gull: Hsla,
    name: Hsla,
    line: Hsla,
    rust: Hsla,
    button: Hsla,
    button_text: Hsla,
}

impl Palette {
    fn new(dark: bool) -> Self {
        let c = |hex: u32| Hsla::from(rgb(hex));
        if dark {
            Palette {
                sky: [c(0x050a1a), c(0x0c1a42), c(0x1c3575)],
                sea: [c(0x24489f), c(0x17307a), c(0x0f2157), c(0x050a1c)],
                foam: c(0xb9d2ff),
                glow: c(0x5f8cf5).opacity(0.22),
                cloud: (c(0x1d3266), c(0x152753)),
                stars: true,
                gull: c(0x9db4e6).opacity(0.8),
                name: c(0xffffff),
                line: c(0xc3d0ee),
                rust: c(0xff9d72),
                button: c(0xffffff),
                button_text: c(0x0c1a42),
            }
        } else {
            Palette {
                sky: [c(0x5c9bef), c(0xa7cdfb), c(0xedf5ff)],
                sea: [c(0x6da4f3), c(0x3f76dd), c(0x1f4bb0), c(0x0c1d52)],
                foam: c(0xffffff),
                glow: c(0xfffdf4).opacity(0.5),
                cloud: (c(0xffffff), c(0xd6e5fb)),
                stars: false,
                gull: c(0x33507f).opacity(0.75),
                name: c(0x0c1220),
                line: c(0x2b3a5c),
                rust: c(0xb83d10),
                button: c(0x0c1a42),
                button_text: c(0xffffff),
            }
        }
    }
}

/// A wave of the sea: how high, how long, how fast and where it starts.
#[derive(Clone, Copy)]
struct Wave {
    /// Below the sea's level, in points at the window's scale.
    depth: f32,
    amplitude: f32,
    /// Waves in the window's width.
    count: f32,
    /// Widths per second, leftwards when negative.
    speed: f32,
    phase: f32,
}

const BACK: Wave = Wave {
    depth: -12.,
    amplitude: 9.,
    count: 3.,
    speed: 0.048,
    phase: 0.,
};
const MIDDLE: Wave = Wave {
    depth: 2.,
    amplitude: 8.,
    count: 4.,
    speed: 0.077,
    phase: 1.2,
};
const FRONT: Wave = Wave {
    depth: 22.,
    amplitude: 8.,
    count: 2.,
    speed: -0.059,
    phase: 2.1,
};

impl Wave {
    /// The water's height at `x`, at `t` seconds, in a window `width` wide.
    fn at(self, x: f32, t: f32, width: f32, level: f32, scale: f32) -> f32 {
        let turn = std::f32::consts::TAU * (x / width * self.count - self.speed * t * self.count)
            + self.phase;
        let amplitude = self.amplitude * scale;
        level
            + self.depth * scale
            + amplitude * turn.sin()
            + amplitude * 0.4 * (2. * turn + self.phase).sin()
    }
}

/// A number from 0 to 1 that is always the same for `seed`.
fn noise(seed: u32) -> f32 {
    let mut x = seed.wrapping_mul(747_796_405).wrapping_add(2_891_336_453);
    x = ((x >> ((x >> 28) + 4)) ^ x).wrapping_mul(277_803_737);
    ((x >> 22) ^ x) as f32 / u32::MAX as f32
}

/// Where everything is at a moment: the numbers both canvases and the
/// elements over them read.
#[derive(Clone, Copy)]
struct Moment {
    t: f32,
    width: f32,
    height: f32,
    scale: f32,
    level: f32,
    /// How much the sky's lights and clouds are out.
    lights: f32,
}

/// The sky, the stars, the clouds and the two waves behind the boat.
fn paint_back(moment: Moment, palette: Palette, bounds: Bounds<Pixels>, window: &mut Window) {
    let Moment {
        t,
        width,
        height,
        scale,
        level,
        lights,
    } = moment;
    let origin = bounds.origin;
    let rect = |x: f32, y: f32, w: f32, h: f32| {
        Bounds::new(
            point(origin.x + px(x), origin.y + px(y)),
            size(px(w.max(0.)), px(h.max(0.))),
        )
    };
    // The sky, in two gradients down to the horizon and below it.
    let middle = height * 0.5;
    window.paint_quad(gpui::fill(
        rect(0., 0., width, middle + 1.),
        linear_gradient(
            180.,
            linear_color_stop(palette.sky[0], 0.),
            linear_color_stop(palette.sky[1], 1.),
        ),
    ));
    window.paint_quad(gpui::fill(
        rect(0., middle, width, height - middle),
        linear_gradient(
            180.,
            linear_color_stop(palette.sky[1], 0.),
            linear_color_stop(palette.sky[2], 0.45),
        ),
    ));
    // A light over the horizon.
    window.paint_quad(gpui::fill(
        rect(0., level - height * 0.3, width, height * 0.3),
        linear_gradient(
            180.,
            linear_color_stop(palette.glow.opacity(0.), 0.),
            linear_color_stop(palette.glow.opacity(palette.glow.a * lights), 1.),
        ),
    ));
    // Stars that twinkle.
    if palette.stars {
        for index in 0..90u32 {
            let x = noise(index * 3) * width;
            let y = noise(index * 3 + 1) * height * 0.55;
            let r = [0.8, 1.1, 1.5][(noise(index * 3 + 2) * 2.99) as usize];
            let twinkle = 0.65 + 0.35 * (t * (0.8 + noise(index) * 1.6) + index as f32).sin();
            let alpha = (0.35 + 0.6 * noise(index * 7)) * twinkle * lights;
            window.paint_quad(
                gpui::fill(
                    rect(x - r, y - r, r * 2., r * 2.),
                    gpui::white().opacity(alpha),
                )
                .corner_radii(px(r)),
            );
        }
    }
    // Clouds drifting left, the far ones slower and fainter.
    let clouds: [(f32, f32, f32, f32, f32); 7] = [
        (0.05, 0.20, 0.62, 0.006, 0.7),
        (0.37, 0.12, 0.50, 0.005, 0.7),
        (0.71, 0.22, 0.66, 0.006, 0.7),
        (0.90, 0.11, 0.46, 0.005, 0.7),
        (0.03, 0.50, 0.95, 0.011, 1.),
        (0.33, 0.62, 0.55, 0.011, 1.),
        (0.83, 0.52, 1.00, 0.011, 1.),
    ];
    let span = width + 400. * scale;
    for (x, y, size_of, speed, strength) in clouds {
        let s = size_of * scale;
        let cx = (x * span - t * speed * width).rem_euclid(span) - 200. * scale;
        let cy = y * level;
        let alpha = lights * strength;
        let puffs = [
            (0., 0., 34.),
            (36., -18., 42.),
            (84., -8., 38.),
            (122., 4., 28.),
            (-32., 8., 24.),
        ];
        for (layer, (color, drop)) in [(palette.cloud.1, 7.), (palette.cloud.0, 0.)]
            .into_iter()
            .enumerate()
        {
            let color = color.opacity(color.a * alpha);
            for (px_, py_, r) in puffs {
                let r = r * s;
                window.paint_quad(
                    gpui::fill(
                        rect(cx + px_ * s - r, cy + (py_ + drop) * s - r, r * 2., r * 2.),
                        color,
                    )
                    .corner_radii(px(r)),
                );
            }
            let bar = if layer == 0 { 34. } else { 32. };
            window.paint_quad(
                gpui::fill(rect(cx - 56. * s, cy + drop * s, 206. * s, bar * s), color)
                    .corner_radii(px(bar * s / 2.)),
            );
        }
    }
    // The two waves behind the boat.
    paint_wave(BACK, palette.sea[0], None, moment, origin, window);
    paint_wave(MIDDLE, palette.sea[1], None, moment, origin, window);
}

/// The wave in front of the boat, with its foam, down to the window's foot.
fn paint_front(moment: Moment, palette: Palette, bounds: Bounds<Pixels>, window: &mut Window) {
    paint_wave(
        FRONT,
        palette.sea[2],
        Some((palette.sea[3], palette.foam)),
        moment,
        bounds.origin,
        window,
    );
}

/// A wave as thin columns from its crest to the window's foot: quads only.
fn paint_wave(
    wave: Wave,
    color: Hsla,
    deep: Option<(Hsla, Hsla)>,
    moment: Moment,
    origin: gpui::Point<Pixels>,
    window: &mut Window,
) {
    let Moment {
        t,
        width,
        height,
        scale,
        level,
        ..
    } = moment;
    let column = 2.;
    let columns = (width / column).ceil() as usize + 1;
    for index in 0..columns {
        let x = index as f32 * column;
        let top = wave.at(x + column / 2., t, width, level, scale);
        if top >= height {
            continue;
        }
        let bounds = Bounds::new(
            point(origin.x + px(x), origin.y + px(top)),
            size(px(column + 0.5), px(height - top)),
        );
        match deep {
            Some((bottom, foam)) => {
                window.paint_quad(gpui::fill(
                    bounds,
                    linear_gradient(
                        180.,
                        linear_color_stop(color.opacity(0.94), 0.55),
                        linear_color_stop(bottom, 1.),
                    ),
                ));
                window.paint_quad(gpui::fill(
                    Bounds::new(
                        point(origin.x + px(x), origin.y + px(top - 1.5)),
                        size(px(column + 0.5), px(3.)),
                    ),
                    foam.opacity(0.85),
                ));
            }
            None => window.paint_quad(gpui::fill(bounds, color)),
        }
    }
}

/// A calm sea in `color`, three still waves at the foot of `bounds`: the
/// welcome page's, a reminder of the first start.
pub(super) fn paint_calm_sea(bounds: Bounds<Pixels>, color: Hsla, window: &mut Window) {
    let width = f32::from(bounds.size.width);
    let height = f32::from(bounds.size.height);
    let moment = Moment {
        t: 0.,
        width,
        height,
        scale: 0.8,
        level: height * 0.42,
        lights: 1.,
    };
    for (wave, alpha) in [(BACK, 0.05), (MIDDLE, 0.08), (FRONT, 0.11)] {
        paint_wave(
            wave,
            color.opacity(alpha),
            None,
            moment,
            bounds.origin,
            window,
        );
    }
}

/// The boat's layers, one color each, from the back.
const BOAT: [(&str, u32); 9] = [
    ("intro/boat-sail.svg", 0xffffff),
    ("intro/boat-sail-shade.svg", 0xc7d6f6),
    ("intro/boat-hull.svg", 0xf4f8ff),
    ("intro/boat-hull-shade.svg", 0x9fb6e8),
    ("intro/boat-ink-0.svg", 0xff4f9a),
    ("intro/boat-ink-1.svg", 0x2f74f5),
    ("intro/boat-ink-2.svg", 0x14a37f),
    ("intro/boat-ink-3.svg", 0x7c4df2),
    ("intro/boat-ink-4.svg", 0x2a3f72),
];

/// The boat, `width` wide, with its middle at `x` and its waterline on
/// the water there, leaning by `lean` radians.
fn boat(x: f32, water: f32, width: f32, lean: f32) -> AnyElement {
    let height = width * 558. / 744.;
    // The waterline is 85% down the drawing, across the hull.
    let top = water - height * 0.85;
    div()
        .absolute()
        .left(px(x - width / 2.))
        .top(px(top))
        .w(px(width))
        .h(px(height))
        .children(BOAT.iter().map(|(path, color)| {
            svg()
                .path(*path)
                .absolute()
                .inset_0()
                .size_full()
                .text_color(Hsla::from(rgb(*color)))
                .with_transformation(Transformation::rotate(radians(lean)))
        }))
        .into_any_element()
}

impl Workbench {
    /// Shows the welcome over the window, and takes the keys.
    pub(crate) fn start_intro(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let focus = cx.focus_handle();
        focus.focus(window, cx);
        self.intro = Some(Intro {
            started: Instant::now(),
            leaving: None,
            focus,
            held: None,
            waiting: Vec::new(),
            tick: None,
        });
        cx.notify();
    }

    /// Holds the welcome still at `seconds`, and at `out` seconds after
    /// the button if given, for the snapshots.
    #[cfg_attr(not(feature = "snapshots"), allow(dead_code))]
    pub(super) fn hold_intro(&mut self, seconds: f32, out: Option<f32>, cx: &mut Context<Self>) {
        if let Some(intro) = &mut self.intro {
            intro.held = Some((seconds, out));
        }
        cx.notify();
    }

    /// «Sali a bordo»: the sea rises over the window, or the welcome goes
    /// at once when nothing moves.
    fn leave_intro(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let still = self.look.duration(1000).is_zero();
        match &mut self.intro {
            Some(intro) if !still => {
                if intro.leaving.is_none() {
                    intro.leaving = Some(Instant::now());
                }
                cx.notify();
            }
            Some(_) => self.end_intro(window, cx),
            None => {}
        }
    }

    /// The welcome is over: the IDE takes the keys, and it never comes again.
    fn end_intro(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(intro) = self.intro.take() else {
            return;
        };
        if let Some(data) = &self.data_dir {
            remember_seen(data);
        }
        self.focus_handle.focus(window, cx);
        for notice in intro.waiting {
            self.show_notice(notice, window, cx);
        }
        cx.notify();
    }

    /// Whether the welcome is on screen.
    #[cfg(any(test, feature = "snapshots"))]
    pub(super) fn intro_shown(&self) -> bool {
        self.intro.is_some()
    }

    /// The welcome, over everything, or nothing once it is gone.
    pub(super) fn render_intro(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let intro = self.intro.as_ref()?;
        let still = self.look.duration(1000).is_zero();
        let (mut t, out) = intro.clock();
        if still {
            t = ALL_IN;
        }
        let out = out.unwrap_or(0.);
        if out >= OUT {
            cx.defer_in(window, |this, window, cx| this.end_intro(window, cx));
            return None;
        }
        // The sea and the sky move while the window is in front, sixty
        // times a second: twice as light as every frame of a 120 Hz screen,
        // and as smooth to the eye. Behind other windows they wait.
        let moving = !still && intro.held.is_none() && (window.is_window_active() || out > 0.);
        if moving && intro.tick.is_none() {
            let tick = cx.spawn_in(window, async move |this, cx| {
                cx.background_executor()
                    .timer(std::time::Duration::from_micros(16_667))
                    .await;
                this.update(cx, |this, cx| {
                    if let Some(intro) = &mut this.intro {
                        intro.tick = None;
                    }
                    cx.notify();
                })
                .ok();
            });
            if let Some(intro) = &mut self.intro {
                intro.tick = Some(tick);
            }
        }
        let intro = self.intro.as_ref()?;
        let italian = self.language == UiLanguage::Italian;
        let palette = Palette::new(cx.theme().is_dark());
        let viewport = window.viewport_size();
        let (width, height) = (f32::from(viewport.width), f32::from(viewport.height));
        let scale = (height / 620.).clamp(0.85, 1.6);

        // The sea rises to the horizon, and then over the window.
        let horizon = height * 0.7;
        let rise = ease_out(along(t, SEA_IN));
        let flood = ease_in_out(along(out, (0.15, 0.95)));
        let level = mix(height + 40. * scale, horizon, rise) - flood * (horizon + 80. * scale);
        let moment = Moment {
            t,
            width,
            height,
            scale,
            level,
            lights: ease_out(along(t, (0., 1.4))),
        };
        // The words go first, then the whole scene clears.
        let words = 1. - ease_out(along(out, (0., 0.35)));
        let clear = 1. - ease_in_out(along(out, (0.95, 0.55)));

        // The boat sails in, rocks on the middle wave, and leaves to the right.
        let boat_width = (width * 0.11).clamp(118., 190.) * scale.min(1.25);
        let sail = ease_out(along(t, BOAT_IN));
        let away = along(out, (0., 1.1)).powi(2);
        let boat_x = mix(-boat_width, width * 0.5, sail)
            + away * (width * 0.6 + boat_width)
            + (t * 0.45).sin() * 6. * scale * sail;
        let water = MIDDLE.at(boat_x, t, width, level, scale);
        let slope = (MIDDLE.at(boat_x + 12., t, width, level, scale)
            - MIDDLE.at(boat_x - 12., t, width, level, scale))
            / 24.;
        let lean = slope.atan() * 0.8 + (t * 1.15).sin() * 0.035;

        // The name and the line under it rise into place.
        let name = ease_out(along(t, NAME_IN)) * words;
        let line = ease_out(along(t, LINE_IN)) * words;
        let button = ease_out(along(t, BUTTON_IN)) * words;
        let name_size = (width * 0.085).clamp(64., 128.);
        let line_size = (name_size * 0.24).clamp(17., 28.);
        let name_top = height * 0.15;

        let back = canvas(
            |_, _, _| (),
            move |bounds, _, window, _| paint_back(moment, palette, bounds, window),
        )
        .absolute()
        .inset_0()
        .size_full();
        let front = canvas(
            |_, _, _| (),
            move |bounds, _, window, _| paint_front(moment, palette, bounds, window),
        )
        .absolute()
        .inset_0()
        .size_full();

        let (tagline, rust) = if italian {
            ("L'IDE per Mac scritto in ", "Rust")
        } else {
            ("The Mac IDE written in ", "Rust")
        };
        let words_block = div()
            .absolute()
            .left_0()
            .right_0()
            .top(px(name_top))
            .flex()
            .flex_col()
            .items_center()
            .child(
                div()
                    .relative()
                    .top(px((1. - name) * 22.))
                    .opacity(name)
                    .text_size(px(name_size))
                    .line_height(px(name_size * 1.05))
                    .font_weight(FontWeight::BOLD)
                    .text_color(palette.name)
                    .child("Keel"),
            )
            .child(
                div()
                    .relative()
                    .top(px((1. - line) * 14.))
                    .opacity(line)
                    .mt(px(line_size * 0.4))
                    .flex()
                    .text_size(px(line_size))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(palette.line)
                    .child(tagline)
                    .child(
                        div()
                            .font_weight(FontWeight::BOLD)
                            .text_color(palette.rust)
                            .child(rust),
                    ),
            )
            .child(
                div()
                    .relative()
                    .top(px((1. - button) * 12.))
                    .opacity(button)
                    .mt(px(line_size * 1.6))
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(10.))
                    .child(
                        div()
                            .id("welcome-aboard")
                            .h(px(46.))
                            .px(px(24.))
                            .flex()
                            .items_center()
                            .gap(px(10.))
                            .rounded_full()
                            .bg(palette.button)
                            .text_color(palette.button_text)
                            .text_size(px(16.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .shadow_lg()
                            .cursor_pointer()
                            .when(button > 0.5, |pill| {
                                pill.hover(|pill| pill.opacity(0.9))
                                    .on_mouse_down(MouseButton::Left, |_, _, cx| {
                                        cx.stop_propagation()
                                    })
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.leave_intro(window, cx)
                                    }))
                            })
                            .child(if italian {
                                "Sali a bordo"
                            } else {
                                "Come aboard"
                            })
                            .child(Icon::new(IconName::ArrowRight).small()),
                    )
                    .child(
                        div()
                            .text_size(px(12.5))
                            .text_color(palette.line.opacity(0.7))
                            .child(if italian {
                                "oppure premi Invio"
                            } else {
                                "or press Return"
                            }),
                    ),
            );

        // Gulls cross the sky now and then.
        let gulls = (0..3).map(|index| {
            let period = 19.;
            let start = 2.5 + index as f32 * 0.9;
            let along_sky = ((t - start).rem_euclid(period)) / period;
            let x = mix(-60., width + 60., along_sky);
            let y = height * (0.12 + index as f32 * 0.035)
                + (along_sky * std::f32::consts::PI * 2.).sin() * 14.;
            let flap = 1. + (t * 5. + index as f32).sin() * 0.12;
            let wide = (26. - index as f32 * 4.) * scale.min(1.2);
            svg()
                .path("intro/gull.svg")
                .absolute()
                .left(px(x))
                .top(px(y))
                .w(px(wide))
                .h(px(wide * 0.4 * flap))
                .text_color(palette.gull.opacity(palette.gull.a * moment.lights))
        });

        let focus = intro.focus.clone();
        Some(
            div()
                .id("welcome")
                .absolute()
                .inset_0()
                .overflow_hidden()
                .occlude()
                .opacity(clear)
                .track_focus(&focus)
                .key_context("KeelWelcome")
                .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                    if matches!(event.keystroke.key.as_str(), "enter" | "escape" | "space") {
                        this.leave_intro(window, cx);
                        cx.stop_propagation();
                    }
                }))
                // The window moves from anywhere on the welcome, as from
                // its toolbar.
                .on_mouse_down(MouseButton::Left, |_, window, _| window.start_window_move())
                .bg(palette.sky[0])
                .child(back)
                .children(gulls)
                .child(words_block)
                .child(boat(boat_x, water, boat_width, lean))
                .child(front)
                .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use gpui::{TestAppContext, VisualTestContext};

    use super::*;

    #[test]
    fn only_a_folder_where_keel_never_ran_is_a_first_start() {
        let fresh = tempfile::tempdir().unwrap();
        assert!(first_start(fresh.path()), "nothing there yet");

        let used = tempfile::tempdir().unwrap();
        std::fs::write(crate::paths::user_settings_file(used.path()), "{}\n").unwrap();
        assert!(!first_start(used.path()), "Keel ran there before");

        let seen = tempfile::tempdir().unwrap();
        remember_seen(seen.path());
        assert!(!first_start(seen.path()), "the welcome was seen");
    }

    #[test]
    fn the_waves_stay_in_their_band() {
        for wave in [BACK, MIDDLE, FRONT] {
            for step in 0..200 {
                let x = step as f32 * 7.3;
                let y = wave.at(x, step as f32 * 0.37, 1280., 500., 1.);
                let reach = wave.amplitude * 1.4 + wave.depth.abs();
                assert!((y - 500.).abs() <= reach + 0.01, "{y}");
            }
        }
    }

    fn window(
        cx: &mut TestAppContext,
    ) -> (
        tempfile::TempDir,
        PathBuf,
        Entity<Workbench>,
        &mut VisualTestContext,
    ) {
        cx.update(|cx| {
            gpui_component::init(cx);
            language::register_grammars();
            crate::workbench_window::init(cx);
        });
        let folder = tempfile::tempdir().unwrap();
        let data = std::fs::canonicalize(folder.path()).unwrap();
        let settings = Settings::load(crate::paths::user_settings_file(&data), None);
        let opening = Opening::new(None, None).with_data_dir(data.clone());
        let (view, cx) =
            cx.add_window_view(|window, cx| Workbench::new(opening, settings, None, window, cx));
        (folder, data, view, cx)
    }

    #[gpui::test]
    fn return_goes_aboard_and_the_welcome_never_comes_again(cx: &mut TestAppContext) {
        let (_folder, data, view, cx) = window(cx);
        view.update_in(cx, |view, window, cx| view.start_intro(window, cx));
        cx.run_until_parked();
        assert!(view.read_with(cx, |view, _| view.intro_shown()));
        assert!(!data.join(SEEN_MARK).exists(), "not seen until it ends");

        // A notice that comes meanwhile waits for the welcome to end.
        view.update_in(cx, |view, window, cx| {
            view.notify_user(super::super::notices::Notice::info("Un avviso"), window, cx)
        });
        let waiting = view.read_with(cx, |view, _| {
            view.intro.as_ref().map_or(0, |intro| intro.waiting.len())
        });
        assert_eq!(waiting, 1);

        cx.simulate_keystrokes("enter");
        cx.run_until_parked();
        assert!(!view.read_with(cx, |view, _| view.intro_shown()), "aboard");
        assert!(data.join(SEEN_MARK).exists());
        assert!(!first_start(&data), "never again");
    }
}
