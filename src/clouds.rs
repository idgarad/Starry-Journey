//! Cloud coverage presets (owner decisions, 2026-11-03: variety and
//! density for later rain and storms; `--clouds` and the `C` key).
//!
//! A [`Cover`] names how cloudy the sky is. The procedural generator
//! ([`crate::cloudgen`]) turns it into each layer's coverage and thickness,
//! and [`crate::skyclouds`] renders it in the app.

/// Cloud coverage presets, in the order `C` cycles them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Cover {
    /// No clouds.
    Clear,
    /// A few small puffs.
    Puffs,
    /// The default: scattered heaps and high streaks.
    #[default]
    Scattered,
    /// More and bigger heaps, some low scud.
    Broken,
    /// Heaps under a high sheet, low scud near the horizon.
    Overcast,
    /// Heavy, dark, full cover (lightning arrives in a later feature).
    Storm,
}

/// Every preset, in cycling order.
pub const COVERS: [Cover; 6] = [
    Cover::Clear,
    Cover::Puffs,
    Cover::Scattered,
    Cover::Broken,
    Cover::Overcast,
    Cover::Storm,
];

impl Cover {
    /// The preset after this one (wrapping), for the `C` key.
    pub fn next(self) -> Self {
        let index = COVERS.iter().position(|&c| c == self).unwrap_or(0);
        COVERS[(index + 1) % COVERS.len()]
    }

    /// Display name, shown in the message box when cycling.
    pub fn label(self) -> &'static str {
        match self {
            Cover::Clear => "Clear sky",
            Cover::Puffs => "Fair-weather puffs",
            Cover::Scattered => "Scattered clouds",
            Cover::Broken => "Broken clouds",
            Cover::Overcast => "Overcast",
            Cover::Storm => "Storm clouds",
        }
    }

    /// Command-line key.
    pub fn key(self) -> &'static str {
        match self {
            Cover::Clear => "clear",
            Cover::Puffs => "puffs",
            Cover::Scattered => "scattered",
            Cover::Broken => "broken",
            Cover::Overcast => "overcast",
            Cover::Storm => "storm",
        }
    }

    /// Parses a command-line key (case-insensitive).
    ///
    /// # Errors
    ///
    /// Returns a message listing the keys when `value` is not one.
    pub fn parse(value: &str) -> Result<Self, String> {
        let wanted = value.trim().to_ascii_lowercase();
        COVERS
            .iter()
            .copied()
            .find(|c| c.key() == wanted)
            .ok_or_else(|| {
                let keys: Vec<&str> = COVERS.iter().map(|c| c.key()).collect();
                format!("must be one of {}, got '{value}'", keys.join(", "))
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_cycle_and_parse() {
        let mut cover = Cover::Clear;
        for expected in COVERS.iter().skip(1).chain(&[Cover::Clear]) {
            cover = cover.next();
            assert_eq!(cover, *expected);
        }
        for cover in COVERS {
            assert_eq!(Cover::parse(cover.key()), Ok(cover));
            assert!(!cover.label().is_empty());
        }
        assert_eq!(Cover::parse(" Storm "), Ok(Cover::Storm));
        assert!(Cover::parse("drizzle").unwrap_err().contains("overcast"));
        assert_eq!(Cover::default(), Cover::Scattered);
    }
}
