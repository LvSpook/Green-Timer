//! Font discovery and loading.
//!
//! egui ships a single embedded font, so to honour the user's chosen family the
//! matching font file is located through `font-kit` (which reads the Windows
//! font registry) and registered under a named family.

use std::sync::Arc;

/// Ordered fallbacks that exist on essentially every Windows install.
pub const FALLBACKS: [&str; 5] = [
    "Segoe UI",
    "Consolas",
    "Cascadia Mono",
    "Arial",
    "DejaVu Sans",
];

/// Every installed font family, sorted case-insensitively.
pub fn available_families() -> Vec<String> {
    let mut families: Vec<String> = font_kit::source::SystemSource::new()
        .all_families()
        .unwrap_or_default();
    families.sort_by_key(|name| name.to_lowercase());
    families.dedup();
    families
}

/// Return `requested` when installed, otherwise the best available fallback.
pub fn resolve_family(requested: &str) -> String {
    let families = available_families();
    if families.iter().any(|name| name == requested) {
        return requested.to_string();
    }
    for candidate in FALLBACKS {
        if families.iter().any(|name| name == candidate) {
            return candidate.to_string();
        }
    }
    families
        .first()
        .cloned()
        .unwrap_or_else(|| "Segoe UI".to_string())
}

/// Load the bytes of `family` for registration with egui.
///
/// A regular and a bold face are returned separately because the timer uses a
/// bold weight and egui needs the bold file registered as its own family.
pub fn load_family(family: &str) -> Option<(Vec<u8>, Option<Vec<u8>>)> {
    let source = font_kit::source::SystemSource::new();

    let read = |handle: &font_kit::handle::Handle| -> Option<Vec<u8>> {
        handle.load().ok()?.copy_font_data().map(|data| data.to_vec())
    };

    let regular = source
        .select_family_by_name(family)
        .ok()?
        .fonts()
        .first()
        .and_then(read)?;

    let bold = source
        .select_best_match(
            &[font_kit::family_name::FamilyName::Title(family.to_string())],
            font_kit::properties::Properties::new().weight(font_kit::properties::Weight::BOLD),
        )
        .ok()
        .and_then(|handle| read(&handle));

    Some((regular, bold))
}

/// Register `family` with egui under two stable names: one for the regular
/// face and one for the bold face the timer uses. A previously registered
/// family is replaced wholesale, so a live font change never leaks font data.
pub fn install(ctx: &egui::Context, family: &str) {
    let resolved = resolve_family(family);
    let Some((regular, bold)) = load_family(&resolved) else {
        log::warn!("Font {resolved:?} could not be loaded; using the built-in font");
        return;
    };

    let mut fonts = egui::FontDefinitions::default();
    let regular_key = format!("user-{resolved}");
    fonts
        .font_data
        .insert(regular_key.clone(), egui::FontData::from_owned(regular));

    let bold_name = bold_family_name(&resolved);
    let bold_key = bold.map(|bytes| {
        let key = format!("user-{resolved}-bold");
        fonts
            .font_data
            .insert(key.clone(), egui::FontData::from_owned(bytes));
        key
    });

    let regular_family = egui::FontFamily::Name(Arc::from(resolved.as_str()));
    fonts
        .families
        .entry(regular_family)
        .or_default()
        .insert(0, regular_key.clone());

    if let Some(key) = bold_key {
        fonts
            .families
            .entry(egui::FontFamily::Name(Arc::from(bold_name.as_str())))
            .or_default()
            .insert(0, key);
    }

    // Settings widgets ask for the proportional family; put the chosen font
    // first so the whole app shares one typeface.
    if let Some(proportional) = fonts.families.get_mut(&egui::FontFamily::Proportional) {
        proportional.insert(0, regular_key);
    }

    ctx.set_fonts(fonts);
}

/// The egui family name used for the bold face of `family`.
pub fn bold_family_name(family: &str) -> String {
    format!("{family} Bold")
}

/// The family the overlay should draw with, preferring the bold face when the
/// system provided one.
pub fn timer_family(ctx: &egui::Context, family: &str) -> egui::FontFamily {
    let resolved = resolve_family(family);
    let bold = bold_family_name(&resolved);
    let bold_family = egui::FontFamily::Name(Arc::from(bold.as_str()));

    let has_bold = ctx.fonts(|fonts| fonts.families().contains(&bold_family));
    if has_bold {
        bold_family
    } else {
        egui::FontFamily::Name(Arc::from(resolved.as_str()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fallback_list_is_ordered() {
        assert_eq!(FALLBACKS[0], "Segoe UI");
    }

    #[test]
    fn bold_name_is_stable() {
        assert_eq!(bold_family_name("Consolas"), "Consolas Bold");
    }

    #[test]
    fn resolved_family_is_never_empty() {
        assert!(!resolve_family("Definitely Not A Real Font").is_empty());
    }
}
