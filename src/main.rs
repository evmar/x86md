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

    let mut by_y = HashMap::new();
    for frag in device.frags {
        by_y.entry(frag.pos.y).or_insert(vec![]).push(frag);
    }
    for (y, mut frags) in by_y {
        println!("{y}");
        frags.sort_by_key(|f| f.pos.x);
        let mut joined = vec![];
        let mut x = frags[0].pos.x;
        let mut cur = Fragment {
            text: "".into(),
            pos: Coord {
                x: frags[0].pos.x,
                y: y,
            },
        };
        for frag in frags {
            if frag.pos.x - x < 100 {
                cur.text.push_str(&frag.text);
            } else {
                joined.push(cur);
                cur = Fragment {
                    text: frag.text,
                    pos: Coord {
                        x: frag.pos.x,
                        y: y,
                    },
                }
            }
            //            println!("{} {} {:?}", frag.pos.x, frag.pos.x - x, frag.text);
            x = frag.pos.x;
        }
        joined.push(cur);
        println!("{:?}", joined);
    }
}

#[derive(Debug)]
struct Coord {
    x: u32,
    y: u32,
}

impl From<kurbo::Vec2> for Coord {
    fn from(v: kurbo::Vec2) -> Self {
        Coord {
            x: (v.x * 10.0) as u32,
            y: (v.y * 10.0) as u32,
        }
    }
}

#[derive(Debug)]
struct Fragment {
    text: String,
    pos: Coord,
}

#[derive(Default)]
struct Device {
    frags: Vec<Fragment>,
}

impl hayro_interpret::Device<'_> for Device {
    fn draw_path(
        &mut self,
        _path: &kurbo::BezPath,
        _transform: kurbo::Affine,
        _paint: &hayro_interpret::Paint<'_>,
        _draw_mode: &hayro_interpret::PathDrawMode,
    ) {
        println!("TODO: path");
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
        // _transform always identity
        use hayro_interpret::hayro_cmap::BfString;
        let pos = glyph_transform.translation();
        let text = match glyph.as_unicode() {
            Some(s) => match s {
                BfString::Char(c) => format!("{c}"),
                BfString::String(s) => s,
            },
            None => format!("??"),
        };
        assert!(!text.is_empty());
        self.frags.push(Fragment {
            text,
            pos: pos.into(),
        });
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
