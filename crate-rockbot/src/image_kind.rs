/// Image formats the bot can display in chat.
///
/// The classification happens once at the tool boundary; downstream code
/// trusts it. SVG is the only vector format: it is displayed through the
/// share's `/download` endpoint when Nextcloud serves it as `image/svg+xml`,
/// with `/preview` as the raster fallback (issue #122).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageKind {
    Png,
    Jpeg,
    Gif,
    Webp,
    Svg,
}

impl ImageKind {
    /// Classify a path by its extension. Returns `None` for unsupported or
    /// extension-less paths.
    pub fn from_path(path: &str) -> Option<Self> {
        let ext = path.rsplit('.').next()?.to_ascii_lowercase();
        match ext.as_str() {
            "png" => Some(Self::Png),
            "jpg" | "jpeg" => Some(Self::Jpeg),
            "gif" => Some(Self::Gif),
            "webp" => Some(Self::Webp),
            "svg" => Some(Self::Svg),
            _ => None,
        }
    }

    pub const fn is_vector(self) -> bool {
        matches!(self, Self::Svg)
    }

    pub const fn mime(self) -> &'static str {
        match self {
            Self::Png => "image/png",
            Self::Jpeg => "image/jpeg",
            Self::Gif => "image/gif",
            Self::Webp => "image/webp",
            Self::Svg => "image/svg+xml",
        }
    }

    pub const fn extension(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Jpeg => "jpg",
            Self::Gif => "gif",
            Self::Webp => "webp",
            Self::Svg => "svg",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_supported_extensions() {
        assert_eq!(ImageKind::from_path("photo.png"), Some(ImageKind::Png));
        assert_eq!(ImageKind::from_path("photo.JPG"), Some(ImageKind::Jpeg));
        assert_eq!(ImageKind::from_path("photo.jpeg"), Some(ImageKind::Jpeg));
        assert_eq!(ImageKind::from_path("anim.gif"), Some(ImageKind::Gif));
        assert_eq!(ImageKind::from_path("photo.webp"), Some(ImageKind::Webp));
        assert_eq!(ImageKind::from_path("diagram.svg"), Some(ImageKind::Svg));
        assert_eq!(
            ImageKind::from_path("/r-room/images/Diagram.SVG"),
            Some(ImageKind::Svg)
        );
    }

    #[test]
    fn rejects_unsupported_or_extensionless_paths() {
        assert_eq!(ImageKind::from_path("notes.txt"), None);
        assert_eq!(ImageKind::from_path("README"), None);
        assert_eq!(ImageKind::from_path("imagepng"), None);
        assert_eq!(ImageKind::from_path(""), None);
    }

    #[test]
    fn maps_mime_and_extension() {
        assert_eq!(ImageKind::Png.mime(), "image/png");
        assert_eq!(ImageKind::Jpeg.mime(), "image/jpeg");
        assert_eq!(ImageKind::Gif.mime(), "image/gif");
        assert_eq!(ImageKind::Webp.mime(), "image/webp");
        assert_eq!(ImageKind::Svg.mime(), "image/svg+xml");

        assert_eq!(ImageKind::Png.extension(), "png");
        assert_eq!(ImageKind::Jpeg.extension(), "jpg");
        assert_eq!(ImageKind::Gif.extension(), "gif");
        assert_eq!(ImageKind::Webp.extension(), "webp");
        assert_eq!(ImageKind::Svg.extension(), "svg");
    }

    #[test]
    fn only_svg_is_vector() {
        assert!(ImageKind::Svg.is_vector());
        for kind in [
            ImageKind::Png,
            ImageKind::Jpeg,
            ImageKind::Gif,
            ImageKind::Webp,
        ] {
            assert!(!kind.is_vector());
        }
    }
}
