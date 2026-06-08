//! Use hayro_interpret to render a PDF to a collection of lines and text.

use std::collections::HashMap;

use hayro_interpret::{Context, InterpreterCache, InterpreterSettings, hayro_syntax::Pdf};

pub struct PDF<'a> {
    pdf: Pdf,
    cache: InterpreterCache<'a>,
}

impl<'a> PDF<'a> {
    pub fn parse(data: Vec<u8>) -> Self {
        let pdf = Pdf::new(data).unwrap();
        let cache = InterpreterCache::new();
        Self { pdf, cache }
    }
}

// We must use a separate type for rendering pages due to hayro lifetimes
pub struct Renderer<'a> {
    pdf: &'a PDF<'a>,
    context: Context<'a>,
}

impl<'a> Renderer<'a> {
    pub fn new(pdf: &'a PDF<'a>) -> Self {
        // https://github.com/LaurenzV/hayro/blob/main/hayro-interpret/examples/extract_html.rs
        let settings = InterpreterSettings::default();
        let context = Context::new(
            kurbo::Affine::IDENTITY,
            kurbo::Rect::new(0.0, 0.0, 1.0, 1.0),
            &pdf.cache,
            pdf.pdf.xref(),
            settings,
        );
        Renderer { pdf, context }
    }

    pub fn page(&mut self, page: usize) -> Render {
        let pdf_page = &self.pdf.pdf.pages()[page - 1];
        let mut device = Device::default();
        hayro_interpret::interpret_page(pdf_page, &mut self.context, &mut device);
        device.render
    }
}

// Convert floats to fixed-point coordinates to avoid floating-point comparison.
fn point_to_fixed(p: kurbo::Point) -> (u32, u32) {
    ((p.x * 10.0).round() as u32, (p.y * 10.0).round() as u32)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Font {
    Heading,
    SubHeading,
    TableHeading,
    Body,
    Code,
    Footer,
    Unknown,
}

#[derive(Debug, Clone)]
pub struct Fragment {
    pub x: u32,
    pub x2: u32,
    pub font: Font,
    pub text: String,
}

#[derive(Debug)]
pub struct TextLine {
    pub y: u32,
    pub frags: Vec<Fragment>,
}

#[derive(Debug, Clone)]
pub struct Line {
    pub x1: u32,
    //pub x2: u32,
    pub y1: u32,
    pub y2: u32,
}

#[derive(Default)]
pub struct Render {
    pub text_lines: Vec<TextLine>,

    /// y coords of any horizontal lines, so that we never merge text across table borders.
    pub horiz_lines: Vec<u32>,

    /// Coords of any vertical lines, for identifying figures.
    pub vert_lines: Vec<Line>,
}

#[derive(Default)]
struct Device {
    // It is pretty difficult to figure out the font of a glyph in hayro because it
    // resolves the font rather than giving you the font id from the raw PDF format.
    // This map is keyed off of the `(font_cache_key, scale)` of the glyphs.
    fonts: HashMap<(u128, u32), Font>,
    render: Render,
}

impl Device {
    fn font_id(
        &mut self,
        glyph_transform: &kurbo::Affine,
        glyph: &hayro_interpret::font::OutlineGlyph,
        text: &str,
    ) -> Font {
        let scale = {
            let c = glyph_transform.as_coeffs();
            let s = (c[0] * 1000.0).round() as u32;
            // sanity: x/y scale match
            // ...apparently not in figure on page 147
            // assert_eq!(s, (c[3] * 1000.0).round() as u32);
            s
        };

        let key = glyph.font_cache_key();
        match self.fonts.get(&(key, scale)) {
            Some(f) => return f.clone(),
            None => {}
        };

        let font_data = glyph.font_data();
        let name = match &font_data {
            Some(f) => Some(f.postscript_name.as_ref().unwrap().as_str()),
            None => None,
        };

        let font = match (name, scale) {
            (Some("NeoSansIntelMedium"), 12) => Font::Heading,
            (Some("NeoSansIntelMedium"), 10) => Font::SubHeading,
            (Some("NeoSansIntelMedium"), 9) => Font::TableHeading,
            (Some("NeoSansIntel"), 8) => Font::Footer,
            (Some("Verdana"), _) => Font::Body,
            (Some("Verdana,Italic"), _) => Font::Body,
            (Some("NeoSansIntel"), 9) => Font::Code,
            (Some("NeoSansIntel,Italic"), 9) => Font::Code,
            (Some("Arial"), 8) => Font::Unknown,
            (None, _) => Font::Unknown,
            _ => panic!("font {name:?}, {scale} in {text:?}"),
        };
        self.fonts.insert((key, scale), font.clone());
        font
    }
}

impl hayro_interpret::Device<'_> for Device {
    fn draw_glyph(
        &mut self,
        glyph: &hayro_interpret::font::Glyph<'_>,
        _transform: kurbo::Affine,
        glyph_transform: kurbo::Affine,
        _paint: &hayro_interpret::Paint<'_>,
        // TODO: Move this into outline glyph.
        _draw_mode: &hayro_interpret::GlyphDrawMode,
    ) {
        use hayro_interpret::hayro_cmap::BfString;
        let text = match glyph.as_unicode() {
            Some(s) => match s {
                BfString::Char(c) => format!("{c}"),
                BfString::String(s) => s,
            },
            None => format!("??"),
        };
        assert!(!text.is_empty());

        let glyph = match glyph {
            hayro_interpret::font::Glyph::Outline(outline) => outline,
            hayro_interpret::font::Glyph::Type3(_) => panic!(),
        };

        let font = self.font_id(&glyph_transform, glyph, &text);

        // _transform always identity
        let (x, y) = point_to_fixed(glyph_transform.translation().to_point());
        let advance = glyph.advance_width().unwrap_or(0.0) as u32 / 10;
        let x2 = x + advance;

        let line = match self.render.text_lines.binary_search_by_key(&y, |l| l.y) {
            Ok(i) => &mut self.render.text_lines[i],
            Err(i) => {
                self.render
                    .text_lines
                    .insert(i, TextLine { y, frags: vec![] });
                &mut self.render.text_lines[i]
            }
        };
        line.frags.push(Fragment { x, x2, font, text });
    }

    fn draw_path(
        &mut self,
        path: &kurbo::BezPath,
        transform: kurbo::Affine,
        paint: &hayro_interpret::Paint<'_>,
        _draw_mode: &hayro_interpret::PathDrawMode,
    ) {
        let hayro_interpret::Paint::Color(color) = paint else {
            panic!();
        };
        if color.to_rgba().to_rgba8() == hayro_interpret::color::AlphaColor::WHITE.to_rgba8() {
            // For some reason there are white lines in the PDF; ignore.
            return;
        }

        for seg in path.segments() {
            match seg {
                kurbo::PathSeg::Line(line) => {
                    let (x1, y1) = point_to_fixed(transform * line.p0);
                    let (x2, y2) = point_to_fixed(transform * line.p1);
                    let x_delta = x1.abs_diff(x2);
                    let y_delta = y1.abs_diff(y2);
                    if x_delta == 0 {
                        let (y1, y2) = (y1.min(y2), y1.max(y2));
                        self.render.vert_lines.push(Line { x1, y1, y2 });
                    } else if y_delta == 0 {
                        self.render.horiz_lines.push(y1);
                    } else {
                        // part of a figure
                    }
                }
                _ => {}
            }
        }
    }

    fn draw_image(&mut self, _image: hayro_interpret::Image<'_, '_>, _transform: kurbo::Affine) {
        todo!()
    }

    fn push_clip_path(&mut self, _clip_path: &hayro_interpret::ClipPath) {}
    fn pop_clip_path(&mut self) {}

    fn push_transparency_group(
        &mut self,
        _opacity: f32,
        _mask: Option<hayro_interpret::SoftMask<'_>>,
        _blend_mode: hayro_interpret::BlendMode,
    ) {
    }
    fn pop_transparency_group(&mut self) {}

    fn set_soft_mask(&mut self, _mask: Option<hayro_interpret::SoftMask<'_>>) {}
    fn set_blend_mode(&mut self, _blend_mode: hayro_interpret::BlendMode) {}
}
