use std::collections::HashMap;

use hayro_interpret::{Context, InterpreterCache, InterpreterSettings, hayro_syntax::Pdf};

fn main() {
    let args = std::env::args().collect::<Vec<_>>();
    let data = std::fs::read(&args[1]).unwrap();
    let pdf = Pdf::new(data).unwrap();

    let first_page = 118;
    let page = &pdf.pages()[first_page];

    // https://github.com/LaurenzV/hayro/blob/main/hayro-interpret/examples/extract_html.rs
    let settings = InterpreterSettings::default();
    let cache = InterpreterCache::new();
    let mut context = Context::new(
        kurbo::Affine::IDENTITY,
        kurbo::Rect::new(0.0, 0.0, 1.0, 1.0),
        &cache,
        pdf.xref(),
        settings,
    );

    let mut device = Device::default();
    hayro_interpret::interpret_page(page, &mut context, &mut device);

    let mut lines = device.lines;
    lines.reverse();
    for line in lines {
        let mut frags = line.frags;
        frags.sort_by_key(|f| f.pos.x);
        let mut joined = vec![];
        let mut x = frags[0].pos.x;
        let mut cur = frags[0].clone();
        for frag in frags.into_iter().skip(1) {
            let delta = frag.pos.x - x;
            x = frag.pos.x;

            if delta < 100 {
                cur.text.push_str(&frag.text);
            } else {
                joined.push(cur);
                cur = frag;
            }
            //            println!("{} {} {:?}", frag.pos.x, frag.pos.x - x, frag.text);
        }
        joined.push(cur);
        println!("{:?}", joined);
    }
}

fn to_fixed(f: f64) -> u32 {
    (f * 10.0) as u32
}

#[derive(Debug, Clone)]
struct Coord {
    x: u32,
    y: u32,
}

impl From<kurbo::Vec2> for Coord {
    fn from(v: kurbo::Vec2) -> Self {
        Coord {
            x: to_fixed(v.x),
            y: to_fixed(v.y),
        }
    }
}

#[derive(Debug, Clone)]
struct Fragment {
    pos: Coord,
    font: usize,
    text: String,
}

struct Line {
    y: u32,
    frags: Vec<Fragment>,
}

#[derive(Default)]
struct Device {
    fonts: Vec<String>,
    font_ids: HashMap<(u128, u32), usize>,
    lines: Vec<Line>,
}

impl Device {
    fn font_id(
        &mut self,
        glyph_transform: &kurbo::Affine,
        glyph: &hayro_interpret::font::Glyph<'_>,
    ) -> usize {
        let scale = {
            let c = glyph_transform.as_coeffs();
            let s = (c[0] * 1000.0).round() as u32;
            // sanity: x/y scale match
            assert_eq!(s, (c[3] * 1000.0).round() as u32);
            s
        };
        let outline = match glyph {
            hayro_interpret::font::Glyph::Outline(outline) => outline,
            hayro_interpret::font::Glyph::Type3(_) => panic!(),
        };

        let key = outline.font_cache_key();
        match self.font_ids.get(&(key, scale)) {
            Some(f) => return f.clone(),
            None => {}
        };

        let name = match outline.font_data() {
            Some(f) => f.postscript_name.unwrap().clone(),
            None => "None".into(),
        };
        let name = format!("{}{}", name, scale);
        let id = self.fonts.len();
        self.font_ids.insert((key, scale), id);
        self.fonts.push(name);
        id
    }
}

impl hayro_interpret::Device<'_> for Device {
    fn draw_path(
        &mut self,
        _path: &kurbo::BezPath,
        _transform: kurbo::Affine,
        _paint: &hayro_interpret::Paint<'_>,
        _draw_mode: &hayro_interpret::PathDrawMode,
    ) {
        // println!("TODO: path");
    }

    fn draw_glyph(
        &mut self,
        glyph: &hayro_interpret::font::Glyph<'_>,
        _transform: kurbo::Affine,
        glyph_transform: kurbo::Affine,
        _paint: &hayro_interpret::Paint<'_>,
        // TODO: Move this into outline glyph.
        _draw_mode: &hayro_interpret::GlyphDrawMode,
    ) {
        let font = self.font_id(&glyph_transform, glyph);

        use hayro_interpret::hayro_cmap::BfString;
        let text = match glyph.as_unicode() {
            Some(s) => match s {
                BfString::Char(c) => format!("{c}"),
                BfString::String(s) => s,
            },
            None => format!("??"),
        };
        assert!(!text.is_empty());

        // _transform always identity
        let pos: Coord = glyph_transform.translation().into();
        let line = match self.lines.binary_search_by_key(&pos.y, |l| l.y) {
            Ok(i) => &mut self.lines[i],
            Err(i) => {
                self.lines.insert(
                    i,
                    Line {
                        y: pos.y,
                        frags: vec![],
                    },
                );
                &mut self.lines[i]
            }
        };
        line.frags.push(Fragment { pos, font, text });
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
