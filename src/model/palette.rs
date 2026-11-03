//! Palette data and color roles (Rebuild Prompt, "Exact Palette Data").
//!
//! The twelve palettes are static data in the specified selector order. Window
//! colors are chosen from a building or window *tone byte* that is stored
//! independently of the palette, so switching palettes never regenerates
//! geometry.

/// An 8-bit-per-channel sRGB color.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Rgb(pub [u8; 3]);

impl Rgb {
    /// Builds a color from a `0xRRGGBB` literal.
    pub const fn hex(value: u32) -> Self {
        Self([(value >> 16) as u8, (value >> 8) as u8, value as u8])
    }

    /// Returns the `#rrggbb` form used by the specification.
    pub fn to_hex_string(self) -> String {
        let [r, g, b] = self.0;
        format!("#{r:02x}{g:02x}{b:02x}")
    }
}

/// Number of building body colors in every palette.
pub const BODY_COUNT: usize = 6;

/// Depth fades for layers 0 (farthest) to 3 (nearest).
pub const DEPTH_FADES: [f64; 4] = [0.3, 0.2, 0.1, 0.0];

/// Bodies blend toward the background by this multiple of the depth fade.
pub const BODY_FADE_MULTIPLIER: f64 = 1.5;

/// Default endless layer tint targets (farthest to nearest), used when a
/// palette has no `layer_tint` override.
pub const LAYER_TINT_TARGETS: [Rgb; 4] = [
    Rgb([70, 94, 130]),
    Rgb([130, 76, 74]),
    Rgb([74, 110, 96]),
    Rgb([128, 112, 86]),
];

/// Fraction by which endless bodies blend toward their layer tint target.
pub const ENDLESS_LAYER_TINT_BLEND: f64 = 0.1;

/// One weighted window color.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowColor {
    /// Lit window color.
    pub color: Rgb,
    /// Relative selection weight, greater than zero.
    pub weight: u32,
}

const fn w(color: u32, weight: u32) -> WindowColor {
    WindowColor {
        color: Rgb::hex(color),
        weight,
    }
}

/// Roadside tree colors for a palette (Starry Journey extension).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TreePalette {
    /// Leaf tones, darkest first. Conifers use the first; broadleaf trees
    /// pick one by their tint byte.
    pub leaves: [Rgb; 3],
    /// Shaded-leaf color.
    pub shade: Rgb,
    /// Trunk color.
    pub trunk: Rgb,
}

const fn trees(leaves: [u32; 3], shade: u32, trunk: u32) -> Option<TreePalette> {
    Some(TreePalette {
        leaves: [
            Rgb::hex(leaves[0]),
            Rgb::hex(leaves[1]),
            Rgb::hex(leaves[2]),
        ],
        shade: Rgb::hex(shade),
        trunk: Rgb::hex(trunk),
    })
}

/// A complete palette definition.
#[derive(Debug, Clone, PartialEq)]
pub struct Palette {
    /// Stable key used on the command line and in configuration.
    pub key: &'static str,
    /// Display label.
    pub label: &'static str,
    /// Sky/background color.
    pub bg: Rgb,
    /// Mast/antenna color; also the roadside tree trunk color.
    pub antenna: Rgb,
    /// Star color; also message text and cloud tint source.
    pub star: Rgb,
    /// Mast beacon color.
    pub beacon: Rgb,
    /// Six unique building body colors.
    pub bodies: [Rgb; BODY_COUNT],
    /// Weighted lit-window colors.
    pub windows: &'static [WindowColor],
    /// Optional building accent color.
    pub accent: Option<Rgb>,
    /// Chance of applying `accent`; 0 when there is no accent.
    pub accent_chance: f64,
    /// Road surface color.
    pub road: Rgb,
    /// Lane dash and curb color.
    pub lane: Rgb,
    /// Vehicle body colors.
    pub vehicles: &'static [Rgb],
    /// Vehicle headlight color.
    pub headlight: Rgb,
    /// Vehicle taillight color.
    pub taillight: Rgb,
    /// HSL lightness (percent) for generated landmark decoration colors.
    pub decor_lightness: u8,
    /// Fixed endless layer tint replacing [`LAYER_TINT_TARGETS`].
    pub layer_tint: Option<Rgb>,
    /// Fixed decoration color replacing generated landmark HSL.
    pub decor: Option<Rgb>,
    /// Tree colors; `None` uses the spec's parchment rule (body colors
    /// faded toward the ink color).
    pub trees: Option<TreePalette>,
}

impl Palette {
    /// Sum of all window weights.
    pub fn total_window_weight(&self) -> u32 {
        self.windows.iter().map(|entry| entry.weight).sum()
    }

    /// Maps a tone byte to a lit-window color.
    ///
    /// Selects the weighted entry containing `(tone + 0.5) / 256 * totalWeight`,
    /// so tones are spread evenly across the weights. O(number of entries).
    pub fn window_color(&self, tone: u8) -> Rgb {
        let target = (f64::from(tone) + 0.5) / 256.0 * f64::from(self.total_window_weight());
        let mut cumulative = 0.0;
        for entry in self.windows {
            cumulative += f64::from(entry.weight);
            if target < cumulative {
                return entry.color;
            }
        }
        // Unreachable for valid data because target < totalWeight; the last
        // entry is the safe answer if rounding ever lands exactly on the sum.
        self.windows[self.windows.len() - 1].color
    }

    /// Endless layer tint target for `layer` (0..=3), honoring `layer_tint`.
    ///
    /// # Panics
    ///
    /// Panics if `layer > 3`.
    pub fn layer_tint_target(&self, layer: usize) -> Rgb {
        self.layer_tint.unwrap_or(LAYER_TINT_TARGETS[layer])
    }
}

/// Palette keys in selector order.
pub const PALETTE_KEYS: [&str; 12] = [
    "classic",
    "artdeco",
    "blackonwhite",
    "cga",
    "cga2",
    "ega",
    "vga",
    "c64",
    "appleii",
    "gameboy",
    "amber",
    "green",
];

/// Key of the default palette.
pub const DEFAULT_PALETTE_KEY: &str = "classic";

/// Returns all palettes in selector order.
pub fn palettes() -> &'static [Palette] {
    &PALETTES
}

/// Looks up a palette by key (case-sensitive, as in the original). O(n), n = 12.
pub fn palette_by_key(key: &str) -> Option<&'static Palette> {
    PALETTES.iter().find(|palette| palette.key == key)
}

/// Returns the palette after `current` in selector order, wrapping around.
/// Unknown keys return the first palette.
pub fn next_palette(current: &str) -> &'static Palette {
    let index = PALETTES
        .iter()
        .position(|palette| palette.key == current)
        .map_or(0, |index| (index + 1) % PALETTES.len());
    &PALETTES[index]
}

const fn bodies(colors: [u32; BODY_COUNT]) -> [Rgb; BODY_COUNT] {
    [
        Rgb::hex(colors[0]),
        Rgb::hex(colors[1]),
        Rgb::hex(colors[2]),
        Rgb::hex(colors[3]),
        Rgb::hex(colors[4]),
        Rgb::hex(colors[5]),
    ]
}

static PALETTES: [Palette; 12] = [
    Palette {
        key: "classic",
        label: "Classic",
        bg: Rgb::hex(0x000000),
        antenna: Rgb::hex(0x7a7a70),
        star: Rgb::hex(0xffffff),
        beacon: Rgb::hex(0xff2a2a),
        bodies: bodies([0x141412, 0x201f1b, 0x111419, 0x1c1814, 0x171b15, 0x26231c]),
        windows: &[w(0xffff55, 6), w(0xffd75f, 2), w(0xffffff, 1)],
        accent: None,
        accent_chance: 0.0,
        road: Rgb::hex(0x101010),
        lane: Rgb::hex(0x5a5a5a),
        vehicles: &[
            Rgb::hex(0xe03030),
            Rgb::hex(0x3060e0),
            Rgb::hex(0xd8d8d8),
            Rgb::hex(0xe0c020),
        ],
        headlight: Rgb::hex(0xffffc0),
        taillight: Rgb::hex(0xff2020),
        decor_lightness: 55,
        layer_tint: None,
        decor: None,
        trees: trees([0x2c5a2a, 0x3a6b32, 0x4b7a3c], 0x1f3f1d, 0x5a4a36),
    },
    Palette {
        key: "artdeco",
        label: "ArtDeco",
        bg: Rgb::hex(0x000000),
        antenna: Rgb::hex(0x8a8478),
        star: Rgb::hex(0xfff6e0),
        beacon: Rgb::hex(0xff3020),
        bodies: bodies([0x181510, 0x261f15, 0x13151b, 0x221814, 0x191d16, 0x2c251a]),
        windows: &[
            w(0xffffff, 4),
            w(0xfff0c8, 3),
            w(0xf0c050, 3),
            w(0xe89040, 1),
        ],
        accent: Some(Rgb::hex(0xd83a2a)),
        accent_chance: 0.08,
        road: Rgb::hex(0x0c0c0c),
        lane: Rgb::hex(0x6a5a3a),
        vehicles: &[
            Rgb::hex(0xd83a2a),
            Rgb::hex(0xd83a2a),
            Rgb::hex(0x3a6fd9),
            Rgb::hex(0xf0e6c8),
        ],
        headlight: Rgb::hex(0xfff6d0),
        taillight: Rgb::hex(0xff2a1a),
        decor_lightness: 58,
        layer_tint: None,
        decor: None,
        trees: trees([0x3d5638, 0x4e6a44, 0x627a4c], 0x2a3c27, 0x6a5a3a),
    },
    Palette {
        key: "blackonwhite",
        label: "Ink on Parchment",
        bg: Rgb::hex(0xefe4c8),
        antenna: Rgb::hex(0x5b5549),
        star: Rgb::hex(0x24221e),
        beacon: Rgb::hex(0x24221e),
        bodies: bodies([0xb7a16e, 0xb88870, 0x899777, 0xa59382, 0xaa7971, 0xa6a071]),
        windows: &[w(0x1b1916, 1)],
        accent: None,
        accent_chance: 0.0,
        road: Rgb::hex(0xdbceb2),
        lane: Rgb::hex(0x7a705d),
        vehicles: &[Rgb::hex(0x24221e), Rgb::hex(0x4b453a)],
        headlight: Rgb::hex(0x82745a),
        taillight: Rgb::hex(0x24221e),
        decor_lightness: 30,
        layer_tint: Some(Rgb::hex(0xa3977d)),
        decor: Some(Rgb::hex(0x24221e)),
        trees: None,
    },
    Palette {
        key: "cga",
        label: "CGA Cyan/Magenta",
        bg: Rgb::hex(0x000000),
        antenna: Rgb::hex(0xaa00aa),
        star: Rgb::hex(0xffffff),
        beacon: Rgb::hex(0xff55ff),
        bodies: bodies([0x092729, 0x290929, 0x112025, 0x251124, 0x102f2f, 0x301230]),
        windows: &[w(0x55ffff, 4), w(0xff55ff, 4), w(0xffffff, 2)],
        accent: None,
        accent_chance: 0.0,
        road: Rgb::hex(0x100c18),
        lane: Rgb::hex(0x55ffff),
        vehicles: &[Rgb::hex(0xff55ff), Rgb::hex(0x55ffff), Rgb::hex(0xffffff)],
        headlight: Rgb::hex(0xffffff),
        taillight: Rgb::hex(0xff55ff),
        decor_lightness: 60,
        layer_tint: None,
        decor: None,
        trees: trees([0x0e6a6e, 0x128a8f, 0x19a9ae], 0x0a4a4d, 0xaa00aa),
    },
    Palette {
        key: "cga2",
        label: "CGA Green/Red",
        bg: Rgb::hex(0x000000),
        antenna: Rgb::hex(0xaa5500),
        star: Rgb::hex(0xffff55),
        beacon: Rgb::hex(0xff5555),
        bodies: bodies([0x102810, 0x2b1110, 0x29260c, 0x17321a, 0x331a12, 0x34300f]),
        windows: &[w(0x55ff55, 4), w(0xff5555, 3), w(0xffff55, 3)],
        accent: None,
        accent_chance: 0.0,
        road: Rgb::hex(0x12140c),
        lane: Rgb::hex(0xffff55),
        vehicles: &[Rgb::hex(0x55ff55), Rgb::hex(0xff5555), Rgb::hex(0xffff55)],
        headlight: Rgb::hex(0xffff55),
        taillight: Rgb::hex(0xff5555),
        decor_lightness: 60,
        layer_tint: None,
        decor: None,
        trees: trees([0x1e7a1e, 0x2a9a2a, 0x3cb83c], 0x145214, 0xaa5500),
    },
    Palette {
        key: "ega",
        label: "EGA",
        bg: Rgb::hex(0x000000),
        antenna: Rgb::hex(0xaaaaaa),
        star: Rgb::hex(0xffffff),
        beacon: Rgb::hex(0xff5555),
        bodies: bodies([0x10102c, 0x102c2c, 0x2c102c, 0x102c10, 0x2c1010, 0x2c2210]),
        windows: &[
            w(0x55ffff, 2),
            w(0xff55ff, 2),
            w(0xffff55, 3),
            w(0x55ff55, 2),
            w(0xffffff, 1),
        ],
        accent: Some(Rgb::hex(0x5555ff)),
        accent_chance: 0.12,
        road: Rgb::hex(0x111111),
        lane: Rgb::hex(0xaaaaaa),
        vehicles: &[
            Rgb::hex(0x5555ff),
            Rgb::hex(0xff5555),
            Rgb::hex(0x55ff55),
            Rgb::hex(0xaa5500),
        ],
        headlight: Rgb::hex(0xffffff),
        taillight: Rgb::hex(0xff5555),
        decor_lightness: 60,
        layer_tint: None,
        decor: None,
        trees: trees([0x006e00, 0x008800, 0x00aa00], 0x004400, 0xaa5500),
    },
    Palette {
        key: "vga",
        label: "VGA",
        bg: Rgb::hex(0x060614),
        antenna: Rgb::hex(0x8a92b8),
        star: Rgb::hex(0xe8f4ff),
        beacon: Rgb::hex(0xff4060),
        bodies: bodies([0x171c3a, 0x301631, 0x10332e, 0x302712, 0x20243d, 0x351c25]),
        windows: &[
            w(0x40c8ff, 3),
            w(0xff70c8, 2),
            w(0xffc850, 3),
            w(0x70f0b0, 2),
            w(0xe8f4ff, 1),
        ],
        accent: Some(Rgb::hex(0xa080ff)),
        accent_chance: 0.14,
        road: Rgb::hex(0x101020),
        lane: Rgb::hex(0x6478a8),
        vehicles: &[
            Rgb::hex(0x3868e8),
            Rgb::hex(0xe84878),
            Rgb::hex(0x38b890),
            Rgb::hex(0xe8a838),
        ],
        headlight: Rgb::hex(0xe8f4ff),
        taillight: Rgb::hex(0xff4060),
        decor_lightness: 65,
        layer_tint: None,
        decor: None,
        trees: trees([0x2a7a50, 0x339160, 0x40a872], 0x1d5438, 0x7a5a3a),
    },
    Palette {
        key: "c64",
        label: "Commodore 64",
        bg: Rgb::hex(0x100e20),
        antenna: Rgb::hex(0x6c6c6c),
        star: Rgb::hex(0xb8c76f),
        beacon: Rgb::hex(0x9a6759),
        bodies: bodies([0x1c1738, 0x2d1929, 0x142927, 0x2b2318, 0x22263e, 0x302116]),
        windows: &[
            w(0x70a4b2, 3),
            w(0xb8c76f, 3),
            w(0x6f3d86, 2),
            w(0x9a6759, 2),
            w(0xffffff, 1),
        ],
        accent: Some(Rgb::hex(0x6f3d86)),
        accent_chance: 0.12,
        road: Rgb::hex(0x211b2e),
        lane: Rgb::hex(0x6c6c6c),
        vehicles: &[
            Rgb::hex(0x352879),
            Rgb::hex(0x68372b),
            Rgb::hex(0x588d43),
            Rgb::hex(0x6f4f25),
        ],
        headlight: Rgb::hex(0xb8c76f),
        taillight: Rgb::hex(0x9a6759),
        decor_lightness: 55,
        layer_tint: None,
        decor: None,
        trees: trees([0x3e6b2e, 0x588d43, 0x6fa85a], 0x2c4d21, 0x6f4f25),
    },
    Palette {
        key: "appleii",
        label: "Apple II",
        bg: Rgb::hex(0x000000),
        antenna: Rgb::hex(0x808080),
        star: Rgb::hex(0xffffff),
        beacon: Rgb::hex(0xff6600),
        bodies: bodies([0x24102d, 0x102b15, 0x301a0a, 0x10242f, 0x291e32, 0x163120]),
        windows: &[
            w(0xdd22dd, 3),
            w(0x22dd22, 3),
            w(0xff6600, 2),
            w(0x11aaff, 2),
            w(0xffffff, 1),
        ],
        accent: None,
        accent_chance: 0.0,
        road: Rgb::hex(0x101410),
        lane: Rgb::hex(0x808080),
        vehicles: &[
            Rgb::hex(0xdd22dd),
            Rgb::hex(0x22dd22),
            Rgb::hex(0xff6600),
            Rgb::hex(0x11aaff),
        ],
        headlight: Rgb::hex(0xffffff),
        taillight: Rgb::hex(0xff6600),
        decor_lightness: 58,
        layer_tint: None,
        decor: None,
        trees: trees([0x147a14, 0x1a9a1a, 0x22b822], 0x0d500d, 0x7a4a10),
    },
    Palette {
        key: "gameboy",
        label: "Game Boy",
        bg: Rgb::hex(0x0f380f),
        antenna: Rgb::hex(0x306230),
        star: Rgb::hex(0x9bbc0f),
        beacon: Rgb::hex(0x8bac0f),
        bodies: bodies([0x173f16, 0x204b1b, 0x2a5520, 0x346127, 0x3d6c2b, 0x47782f]),
        windows: &[w(0x9bbc0f, 3), w(0x8bac0f, 2)],
        accent: None,
        accent_chance: 0.0,
        road: Rgb::hex(0x0b2b0b),
        lane: Rgb::hex(0x306230),
        vehicles: &[Rgb::hex(0x8bac0f), Rgb::hex(0x306230), Rgb::hex(0x9bbc0f)],
        headlight: Rgb::hex(0x9bbc0f),
        taillight: Rgb::hex(0x8bac0f),
        decor_lightness: 50,
        layer_tint: Some(Rgb::hex(0x306230)),
        decor: Some(Rgb::hex(0x9bbc0f)),
        trees: trees([0x306230, 0x306230, 0x8bac0f], 0x0f380f, 0x8bac0f),
    },
    Palette {
        key: "amber",
        label: "Amber Terminal",
        bg: Rgb::hex(0x100900),
        antenna: Rgb::hex(0x805000),
        star: Rgb::hex(0xffb000),
        beacon: Rgb::hex(0xffd070),
        bodies: bodies([0x241500, 0x301d02, 0x3b2503, 0x472c04, 0x523305, 0x603c08]),
        windows: &[w(0xffb000, 4), w(0xffd070, 1)],
        accent: None,
        accent_chance: 0.0,
        road: Rgb::hex(0x160d00),
        lane: Rgb::hex(0x805000),
        vehicles: &[Rgb::hex(0xa06a00), Rgb::hex(0xd09000), Rgb::hex(0xffb000)],
        headlight: Rgb::hex(0xffd070),
        taillight: Rgb::hex(0xffb000),
        decor_lightness: 55,
        layer_tint: Some(Rgb::hex(0x805000)),
        decor: Some(Rgb::hex(0xffb000)),
        trees: trees([0x805000, 0xa06a00, 0xc08000], 0x523305, 0x603c08),
    },
    Palette {
        key: "green",
        label: "Green Terminal",
        bg: Rgb::hex(0x001005),
        antenna: Rgb::hex(0x007028),
        star: Rgb::hex(0x33ff66),
        beacon: Rgb::hex(0x99ffb0),
        bodies: bodies([0x00240b, 0x03300e, 0x043b12, 0x054716, 0x07521b, 0x096021]),
        windows: &[w(0x33ff66, 4), w(0x99ffb0, 1)],
        accent: None,
        accent_chance: 0.0,
        road: Rgb::hex(0x001607),
        lane: Rgb::hex(0x007028),
        vehicles: &[Rgb::hex(0x008830), Rgb::hex(0x10b848), Rgb::hex(0x33ff66)],
        headlight: Rgb::hex(0x99ffb0),
        taillight: Rgb::hex(0x33ff66),
        decor_lightness: 55,
        layer_tint: Some(Rgb::hex(0x007028)),
        decor: Some(Rgb::hex(0x33ff66)),
        trees: trees([0x008830, 0x10b848, 0x20d058], 0x054716, 0x07521b),
    },
];

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn palette_keys_are_in_selector_order() {
        let keys: Vec<_> = palettes().iter().map(|palette| palette.key).collect();
        assert_eq!(keys, PALETTE_KEYS);
        assert!(palette_by_key(DEFAULT_PALETTE_KEY).is_some());
    }

    #[test]
    fn legacy_parchment_key_displays_ink_on_parchment() {
        let palette = palette_by_key("blackonwhite").expect("legacy key must exist");
        assert_eq!(palette.label, "Ink on Parchment");
        assert!(
            palette_by_key("Classic").is_none(),
            "keys are case-sensitive"
        );
        assert!(palette_by_key("").is_none());
    }

    #[test]
    fn every_palette_has_six_unique_bodies_and_valid_roles() {
        for palette in palettes() {
            let unique: HashSet<_> = palette.bodies.iter().collect();
            assert_eq!(unique.len(), BODY_COUNT, "{} bodies", palette.key);
            assert!(!palette.windows.is_empty(), "{} windows", palette.key);
            assert!(palette.windows.iter().all(|entry| entry.weight > 0));
            assert!(!palette.vehicles.is_empty(), "{} vehicles", palette.key);
            assert_eq!(
                palette.accent.is_some(),
                palette.accent_chance > 0.0,
                "{} accent and chance must agree",
                palette.key
            );
            assert!((0.0..=1.0).contains(&palette.accent_chance));
            assert!(palette.decor_lightness <= 100);
        }
    }

    #[test]
    fn window_color_spreads_tones_by_weight() {
        let classic = palette_by_key("classic").unwrap();
        // Total weight 9: tones whose (b + 0.5) / 256 * 9 < 6 map to yellow.
        assert_eq!(classic.window_color(0), Rgb::hex(0xffff55));
        assert_eq!(classic.window_color(170), Rgb::hex(0xffff55)); // 5.994
        assert_eq!(classic.window_color(171), Rgb::hex(0xffd75f)); // 6.029
        assert_eq!(classic.window_color(227), Rgb::hex(0xffd75f)); // 7.998
        assert_eq!(classic.window_color(228), Rgb::hex(0xffffff)); // 8.033
        assert_eq!(classic.window_color(255), Rgb::hex(0xffffff));

        let counts = (0..=255u8).fold([0u32; 3], |mut counts, tone| {
            let color = classic.window_color(tone);
            let index = classic
                .windows
                .iter()
                .position(|entry| entry.color == color)
                .unwrap();
            counts[index] += 1;
            counts
        });
        assert_eq!(counts, [171, 57, 28]);
    }

    #[test]
    fn single_window_palette_always_uses_ink() {
        let parchment = palette_by_key("blackonwhite").unwrap();
        for tone in [0, 127, 255] {
            assert_eq!(parchment.window_color(tone), Rgb::hex(0x1b1916));
        }
    }

    #[test]
    fn layer_tint_target_honors_override() {
        let classic = palette_by_key("classic").unwrap();
        assert_eq!(classic.layer_tint_target(0), Rgb([70, 94, 130]));
        assert_eq!(classic.layer_tint_target(3), Rgb([128, 112, 86]));
        let gameboy = palette_by_key("gameboy").unwrap();
        assert_eq!(gameboy.layer_tint_target(1), Rgb::hex(0x306230));
    }

    #[test]
    fn every_dark_palette_has_tree_colors_that_stand_out_from_the_sky() {
        for palette in palettes() {
            let Some(trees) = palette.trees else {
                assert_eq!(
                    palette.key, "blackonwhite",
                    "only parchment uses the spec rule"
                );
                continue;
            };
            for (role, color) in [
                ("leaf", trees.leaves[0]),
                ("leaf", trees.leaves[1]),
                ("leaf", trees.leaves[2]),
                ("trunk", trees.trunk),
            ] {
                let ratio = contrast(color, palette.bg);
                assert!(
                    ratio >= 1.8,
                    "{} {role} {color:?}: contrast {ratio:.2}",
                    palette.key
                );
            }
        }
    }

    /// WCAG contrast ratio between two colors.
    fn contrast(a: Rgb, b: Rgb) -> f64 {
        fn luminance(c: Rgb) -> f64 {
            let channel = |v: u8| {
                let v = f64::from(v) / 255.0;
                if v <= 0.04045 {
                    v / 12.92
                } else {
                    ((v + 0.055) / 1.055).powf(2.4)
                }
            };
            0.2126 * channel(c.0[0]) + 0.7152 * channel(c.0[1]) + 0.0722 * channel(c.0[2])
        }
        let (la, lb) = (luminance(a), luminance(b));
        (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
    }

    #[test]
    fn next_palette_wraps_and_recovers_from_unknown_keys() {
        assert_eq!(next_palette("classic").key, "artdeco");
        assert_eq!(next_palette("green").key, "classic");
        assert_eq!(next_palette("nope").key, "classic");
    }

    #[test]
    fn rgb_hex_round_trips() {
        assert_eq!(Rgb::hex(0x0a0b0c), Rgb([10, 11, 12]));
        assert_eq!(Rgb::hex(0xefe4c8).to_hex_string(), "#efe4c8");
    }
}
