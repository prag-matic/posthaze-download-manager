//! Composite asset routing for host-supplied embedded assets and `gpui-kit` icons.

use std::borrow::Cow;

use gpui_kit::assets::AllAssets;
use gpui_kit::{AssetSource, Result, SharedString};

/// An [`AssetSource`] that checks a caller-supplied static asset table before
/// delegating to `gpui-kit`'s [`AllAssets`] Lucide icon catalog.
#[derive(Debug, Clone, Copy)]
pub struct CompositeAssetSource {
    entries: &'static [(&'static str, &'static [u8])],
}

impl Default for CompositeAssetSource {
    fn default() -> Self {
        Self::empty()
    }
}

impl CompositeAssetSource {
    /// Creates a [`CompositeAssetSource`] backed by `entries`, checked in order
    /// before falling back to [`AllAssets`].
    pub const fn new(entries: &'static [(&'static str, &'static [u8])]) -> Self {
        Self { entries }
    }

    /// Creates a [`CompositeAssetSource`] with no custom entries, delegating
    /// exclusively to [`AllAssets`].
    pub const fn empty() -> Self {
        Self { entries: &[] }
    }
}

impl AssetSource for CompositeAssetSource {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if let Some((_, bytes)) = self.entries.iter().find(|(entry_path, _)| *entry_path == path) {
            return Ok(Some(Cow::Borrowed(*bytes)));
        }
        AllAssets.load(path)
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut items = AllAssets.list(path)?;
        for (entry_path, _) in self.entries {
            if entry_path.starts_with(path) && !items.iter().any(|item| item.as_ref() == *entry_path)
            {
                items.push((*entry_path).into());
            }
        }
        Ok(items)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::assets::IconName;

    const CUSTOM_SVG: &[u8] = b"<svg xmlns=\"http://www.w3.org/2000/svg\"></svg>";
    const CUSTOM_PNG: &[u8] = b"\x89PNG\r\n\x1a\n";
    const TEST_ASSETS: &[(&str, &[u8])] = &[
        ("brand/logo.svg", CUSTOM_SVG),
        ("brand/app_icon.png", CUSTOM_PNG),
    ];

    #[test]
    fn loads_custom_assets_before_all_assets() {
        let source = CompositeAssetSource::new(TEST_ASSETS);

        let logo = source
            .load("brand/logo.svg")
            .expect("custom svg load should succeed")
            .expect("custom svg bytes should exist");
        assert_eq!(logo.as_ref(), CUSTOM_SVG);

        let icon = source
            .load("brand/app_icon.png")
            .expect("custom png load should succeed")
            .expect("custom png bytes should exist");
        assert_eq!(icon.as_ref(), CUSTOM_PNG);
    }

    #[test]
    fn delegates_unmatched_paths_to_all_assets_and_returns_none_for_missing() {
        let source = CompositeAssetSource::new(TEST_ASSETS);
        let lucide_path = IconName::Check.path();

        let lucide = source
            .load(lucide_path.as_ref())
            .expect("lucide icon load should succeed")
            .expect("lucide icon bytes should exist");
        assert!(!lucide.is_empty());

        let missing = source.load("brand/does_not_exist.svg");
        assert!(missing.is_err());
    }

    #[test]
    fn lists_custom_entries_matching_prefix_alongside_all_assets() {
        let source = CompositeAssetSource::new(TEST_ASSETS);

        let brand_items = source.list("brand/").expect("list brand/ should succeed");
        assert_eq!(brand_items.len(), 2);
        assert!(brand_items.iter().any(|p| p.as_ref() == "brand/logo.svg"));
        assert!(brand_items.iter().any(|p| p.as_ref() == "brand/app_icon.png"));

        let all_icons = source.list("icons/").expect("list icons/ should succeed");
        assert!(!all_icons.is_empty());
    }
}
