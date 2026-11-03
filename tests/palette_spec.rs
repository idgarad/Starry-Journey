//! Cross-checks the Rust palette table against the JSON block in
//! `docs/Rebuild Prompt.md`, so a transcription error in any color, weight,
//! chance, or lightness fails the build instead of shipping silently.

use serde_json::Value;
use starry_journey::model::palette::{palettes, Rgb, PALETTE_KEYS};

fn spec_palettes() -> serde_json::Map<String, Value> {
    let doc = include_str!("../docs/Rebuild Prompt.md");
    let section = doc
        .split("## Exact Palette Data")
        .nth(1)
        .expect("spec has an Exact Palette Data section");
    let json = section
        .split("```json")
        .nth(1)
        .and_then(|rest| rest.split("```").next())
        .expect("palette section has a json block");
    match serde_json::from_str(json).expect("palette json parses") {
        Value::Object(map) => map,
        other => panic!("expected palette object, got {other}"),
    }
}

fn color(value: &Value) -> String {
    value.as_str().expect("color is a string").to_lowercase()
}

fn colors(values: &[Rgb]) -> Vec<String> {
    values.iter().map(|rgb| rgb.to_hex_string()).collect()
}

#[test]
fn palette_table_matches_rebuild_prompt_exactly() {
    let spec = spec_palettes();
    let spec_keys: Vec<_> = spec.keys().map(String::as_str).collect();
    // serde_json's default map is sorted, so compare key sets here; selector
    // order is checked against the PALETTE_KEYS constant.
    let mut expected_keys = PALETTE_KEYS.to_vec();
    expected_keys.sort_unstable();
    assert_eq!(spec_keys, expected_keys);

    for palette in palettes() {
        let entry = &spec[palette.key];
        let key = palette.key;
        assert_eq!(entry["label"], palette.label, "{key} label");
        for (role, rgb) in [
            ("bg", palette.bg),
            ("antenna", palette.antenna),
            ("star", palette.star),
            ("beacon", palette.beacon),
            ("road", palette.road),
            ("lane", palette.lane),
            ("headlight", palette.headlight),
            ("taillight", palette.taillight),
        ] {
            assert_eq!(color(&entry[role]), rgb.to_hex_string(), "{key} {role}");
        }

        let spec_bodies: Vec<_> = entry["bodies"]
            .as_array()
            .unwrap()
            .iter()
            .map(color)
            .collect();
        assert_eq!(spec_bodies, colors(&palette.bodies), "{key} bodies");

        let spec_vehicles: Vec<_> = entry["vehicles"]
            .as_array()
            .unwrap()
            .iter()
            .map(color)
            .collect();
        assert_eq!(spec_vehicles, colors(palette.vehicles), "{key} vehicles");

        let spec_windows: Vec<(String, u64)> = entry["windows"]
            .as_array()
            .unwrap()
            .iter()
            .map(|pair| (color(&pair[0]), pair[1].as_u64().unwrap()))
            .collect();
        let windows: Vec<(String, u64)> = palette
            .windows
            .iter()
            .map(|w| (w.color.to_hex_string(), u64::from(w.weight)))
            .collect();
        assert_eq!(spec_windows, windows, "{key} windows");

        assert_eq!(
            entry.get("accent").map(color),
            palette.accent.map(Rgb::to_hex_string),
            "{key} accent"
        );
        assert_eq!(
            entry
                .get("accentChance")
                .and_then(Value::as_f64)
                .unwrap_or(0.0),
            palette.accent_chance,
            "{key} accentChance"
        );
        assert_eq!(
            entry.get("layerTint").map(color),
            palette.layer_tint.map(Rgb::to_hex_string),
            "{key} layerTint"
        );
        assert_eq!(
            entry.get("decor").map(color),
            palette.decor.map(Rgb::to_hex_string),
            "{key} decor"
        );
        assert_eq!(
            entry["decorLightness"].as_u64(),
            Some(u64::from(palette.decor_lightness)),
            "{key} decorLightness"
        );
    }
}
