//! Embedded brand assets combined with `gpui-kit`'s Lucide icon catalog via
//! [`chronophoto_ui::assets::CompositeAssetSource`].

use std::borrow::Cow;

use chronophoto_ui::assets::CompositeAssetSource;
use gpui_kit::{AssetSource, Result, SharedString};

/// Asset path for the rounded-square Posthaze application icon (PNG).
pub const BRAND_APP_ICON: &str = "brand/app_icon.png";

/// Asset path for the rounded-square Posthaze application icon (SVG).
pub const BRAND_APP_ICON_SVG: &str = "brand/app_icon.svg";

/// Asset path for the transparent light Posthaze geometric logo mark (PNG).
pub const BRAND_LOGO_MARK: &str = "brand/logo_light.png";

/// Asset path for the transparent dark Posthaze geometric logo mark (PNG).
pub const BRAND_LOGO_MARK_DARK: &str = "brand/logo_dark.png";

/// Asset path for the vector Posthaze geometric logo mark (SVG).
pub const BRAND_LOGO_SVG: &str = "brand/logo.svg";

const APP_ICON_BYTES: &[u8] =
    include_bytes!("../../../assets/App Icon/App Icon 256x256.png");
const APP_ICON_SVG_BYTES: &[u8] =
    include_bytes!("../../../assets/App Icon Flat SVG/App Icon Flat 256x256.svg");
const LOGO_MARK_BYTES: &[u8] = include_bytes!("../../../assets/Logo Light/Logo Light 256x256.png");
const LOGO_MARK_DARK_BYTES: &[u8] =
    include_bytes!("../../../assets/Logo Dark/Logo Dark 256x256.png");
const LOGO_SVG_BYTES: &[u8] =
    include_bytes!("../../../assets/Logo Light SVG/Logo Light 32x32.svg");

const BRAND_ASSETS: &[(&str, &[u8])] = &[
    (BRAND_APP_ICON, APP_ICON_BYTES),
    (BRAND_APP_ICON_SVG, APP_ICON_SVG_BYTES),
    (BRAND_LOGO_MARK, LOGO_MARK_BYTES),
    (BRAND_LOGO_MARK_DARK, LOGO_MARK_DARK_BYTES),
    (BRAND_LOGO_SVG, LOGO_SVG_BYTES),
];

/// Composite [`AssetSource`] serving Posthaze brand assets and `gpui-kit` SVG icons.
pub struct PosthazeAssets;

impl PosthazeAssets {
    const fn source() -> CompositeAssetSource {
        CompositeAssetSource::new(BRAND_ASSETS)
    }
}

impl AssetSource for PosthazeAssets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        Self::source().load(path)
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        Self::source().list(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_embedded_brand_assets_and_delegates_icons() {
        let app_icon = PosthazeAssets
            .load(BRAND_APP_ICON)
            .expect("brand app icon should load")
            .expect("brand app icon bytes should be present");
        assert!(app_icon.starts_with(b"\x89PNG\r\n\x1a\n"));

        let app_icon_svg = PosthazeAssets
            .load(BRAND_APP_ICON_SVG)
            .expect("brand app icon svg should load")
            .expect("brand app icon svg bytes should be present");
        assert!(app_icon_svg.starts_with(b"<svg"));

        let logo_mark = PosthazeAssets
            .load(BRAND_LOGO_MARK)
            .expect("brand logo mark should load")
            .expect("brand logo mark bytes should be present");
        assert!(logo_mark.starts_with(b"\x89PNG\r\n\x1a\n"));

        let logo_mark_dark = PosthazeAssets
            .load(BRAND_LOGO_MARK_DARK)
            .expect("brand dark logo mark should load")
            .expect("brand dark logo mark bytes should be present");
        assert!(logo_mark_dark.starts_with(b"\x89PNG\r\n\x1a\n"));

        let logo_svg = PosthazeAssets
            .load(BRAND_LOGO_SVG)
            .expect("brand logo svg should load")
            .expect("brand logo svg bytes should be present");
        assert!(logo_svg.starts_with(b"<svg"));

        let listed = PosthazeAssets.list("brand/").expect("list should succeed");
        assert_eq!(listed.len(), 5);
    }
}
