//! Embedded brand assets combined with `gpui-kit`'s Lucide icon catalog.

use std::borrow::Cow;

use gpui_kit::assets::AllAssets;
use gpui_kit::{AssetSource, Result, SharedString};

/// Asset path for the rounded-square Posthaze application icon.
pub const BRAND_APP_ICON: &str = "brand/posthaze_app_icon.png";

/// Asset path for the transparent Posthaze geometric logo mark.
pub const BRAND_LOGO_MARK: &str = "brand/posthaze_logo_no_back.png";

/// Asset path for the vector Posthaze geometric logo mark.
pub const BRAND_LOGO_SVG: &str = "brand/posthaze_logo.svg";

const APP_ICON_BYTES: &[u8] = include_bytes!("../../../assets/posthaze_app_icon.png");
const LOGO_MARK_BYTES: &[u8] = include_bytes!("../../../assets/posthaze_logo_no_back.png");
const LOGO_SVG_BYTES: &[u8] = include_bytes!("../../../assets/posthaze_logo.svg");

/// Composite [`AssetSource`] serving Posthaze brand assets and `gpui-kit` SVG icons.
pub struct PosthazeAssets;

impl AssetSource for PosthazeAssets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        match path {
            BRAND_APP_ICON => Ok(Some(Cow::Borrowed(APP_ICON_BYTES))),
            BRAND_LOGO_MARK => Ok(Some(Cow::Borrowed(LOGO_MARK_BYTES))),
            BRAND_LOGO_SVG => Ok(Some(Cow::Borrowed(LOGO_SVG_BYTES))),
            _ => AllAssets.load(path),
        }
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut items = AllAssets.list(path)?;
        for asset in [BRAND_APP_ICON, BRAND_LOGO_MARK, BRAND_LOGO_SVG] {
            if asset.starts_with(path) {
                items.push(asset.into());
            }
        }
        Ok(items)
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

        let logo_mark = PosthazeAssets
            .load(BRAND_LOGO_MARK)
            .expect("brand logo mark should load")
            .expect("brand logo mark bytes should be present");
        assert!(logo_mark.starts_with(b"\x89PNG\r\n\x1a\n"));

        let logo_svg = PosthazeAssets
            .load(BRAND_LOGO_SVG)
            .expect("brand logo svg should load")
            .expect("brand logo svg bytes should be present");
        assert!(logo_svg.starts_with(b"<svg"));

        let listed = PosthazeAssets.list("brand/").expect("list should succeed");
        assert_eq!(listed.len(), 3);
    }
}
