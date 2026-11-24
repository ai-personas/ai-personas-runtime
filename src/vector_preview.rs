//! Render the same SVG geometry and embedded fonts for the desktop and persona image input.
use std::path::Path;

pub struct Preview {
    pub pixels: resvg::tiny_skia::Pixmap,
    pub omitted_external_images: usize,
}

pub fn render(bytes: &[u8], maximum_upscale: f32, resources: Option<&Path>) -> Result<Preview, String> {
    Ok(render_with_tree(bytes, maximum_upscale, resources)?.0)
}

/// The desktop can inspect the same resolved paths it rendered. Inspection
/// presentation stays separate from the image supplied to a persona.
pub fn render_with_tree(bytes: &[u8], maximum_upscale: f32, resources: Option<&Path>) -> Result<(Preview, resvg::usvg::Tree), String> {
    let omitted = std::sync::atomic::AtomicUsize::new(0);
    let mut options = resvg::usvg::Options {
        font_family: "Inter Variable".into(),
        ..Default::default()
    };
    options.fontdb_mut().load_font_data(include_bytes!("desktop/fonts/Inter-Regular.ttf").to_vec());
    options.fontdb_mut().load_font_data(include_bytes!("desktop/fonts/DejaVuSansMono.ttf").to_vec());
    options.fontdb_mut().set_sans_serif_family("Inter Variable");
    options.fontdb_mut().set_serif_family("Inter Variable");
    options.fontdb_mut().set_monospace_family("DejaVu Sans Mono");
    if let Some(directory) = resources {
        options.resources_dir = Some(directory.to_owned());
    } else {
        options.image_href_resolver.resolve_string = Box::new(|_, _| {
            omitted.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            None
        });
    }
    let tree = resvg::usvg::Tree::from_data(bytes, &options).map_err(|error| error.to_string())?;
    let size = tree.size();
    let scale = (2400.0 / size.width().max(size.height())).min(maximum_upscale);
    let width = (size.width() * scale).ceil().max(1.0) as u32;
    let height = (size.height() * scale).ceil().max(1.0) as u32;
    let mut pixmap = resvg::tiny_skia::Pixmap::new(width, height).ok_or("SVG preview size is invalid")?;
    resvg::render(&tree, resvg::tiny_skia::Transform::from_scale(scale, scale), &mut pixmap.as_mut());
    Ok((Preview {
        pixels: pixmap,
        omitted_external_images: omitted.load(std::sync::atomic::Ordering::Relaxed),
    }, tree))
}
