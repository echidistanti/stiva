//! The program's icon for its windows. The one Explorer and shortcuts show comes from the
//! executable's resources instead (build.rs).

const PNG: &[u8] = include_bytes!("../assets/icon-128.png");

/// RGBA pixels, width and height.
pub fn rgba() -> Option<(Vec<u8>, u32, u32)> {
    let img = image::load_from_memory_with_format(PNG, image::ImageFormat::Png).ok()?.to_rgba8();
    let (w, h) = img.dimensions();
    Some((img.into_raw(), w, h))
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_embedded_icon_decodes_to_a_square_with_a_transparent_corner() {
        let (px, w, h) = super::rgba().expect("icon decodes");
        assert_eq!((w, h), (128, 128));
        assert_eq!(px.len(), 128 * 128 * 4);
        assert_eq!(px[3], 0, "the corner of the rounded tile is transparent");
        let mid = (64 * 128 + 64) * 4;
        assert_eq!(px[mid + 3], 255, "the middle is opaque");
    }
}
