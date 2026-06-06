use std::collections::HashMap;

use hayro_interpret::{Context, InterpreterCache, InterpreterSettings, hayro_syntax::Pdf};

// for computing when subsequent lines are part of the same paragraph
const MAX_LINE_HEIGHT: u32 = 150;

// for computing indentation in monospace blocks
const LEFT_MARGIN: u32 = 460;
const MONOSPACE_WIDTH: f32 = 50.0;

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

    let mut doc = Doc::default();
    hayro_interpret::interpret_page(page, &mut context, &mut doc);
    doc.postprocess();
    doc.render();
}

#[derive(Debug, Clone)]
struct Coord {
    x: u32,
    y: u32,
}

impl From<kurbo::Vec2> for Coord {
    fn from(v: kurbo::Vec2) -> Self {
        fn to_fixed(f: f64) -> u32 {
            (f * 10.0) as u32
        }
        Coord {
            x: to_fixed(v.x),
            y: to_fixed(v.y),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Font {
    Heading,
    SubHeading,
    Body,
    Code,
    Unknown(String, u32),
}

#[derive(Debug, Clone)]
struct Fragment {
    pos: Coord,
    font: Font,
    text: String,
}

#[derive(Debug)]
struct Line {
    y: u32,
    frags: Vec<Fragment>,
}

#[derive(Default)]
struct Doc {
    // It is pretty difficult to figure out the font of a glyph in hayro because it
    // resolves the font rather than giving you the font id from the raw PDF format.
    // This map is keyed off of the `(font_cache_key, scale)` of the glyphs.
    fonts: HashMap<(u128, u32), Font>,
    lines: Vec<Line>,
}

impl Doc {
    fn font_id(
        &mut self,
        glyph_transform: &kurbo::Affine,
        glyph: &hayro_interpret::font::Glyph<'_>,
    ) -> Font {
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
        match self.fonts.get(&(key, scale)) {
            Some(f) => return f.clone(),
            None => {}
        };

        let font_data = outline.font_data();
        let name = match &font_data {
            Some(f) => f.postscript_name.as_ref().unwrap(),
            None => "None",
        };

        let font = match (name, scale) {
            ("NeoSansIntelMedium", 12) => Font::Heading,
            ("NeoSansIntelMedium", 10) => Font::SubHeading,
            ("Verdana", 9) => Font::Body,
            ("NeoSansIntel", 9) => Font::Code,
            (name, scale) => Font::Unknown(name.to_string(), scale),
        };
        self.fonts.insert((key, scale), font.clone());
        font
    }

    fn postprocess(&mut self) {
        self.lines.reverse();
        self.join_fragments();
        self.join_paragraphs();
    }

    fn join_fragments(&mut self) {
        let mut new_lines = vec![];
        for line in self.lines.drain(..) {
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
            }
            joined.push(cur);
            new_lines.push(Line {
                y: joined[0].pos.y,
                frags: joined,
            });
        }
        self.lines = new_lines;
    }

    fn join_paragraphs(&mut self) {
        for i in (1..self.lines.len() - 1).rev() {
            let [cur, prev] = self.lines.get_disjoint_mut([i, i - 1]).unwrap();
            if cur.frags.len() != 1 || prev.frags.len() != 1 {
                continue;
            }
            if cur.frags[0].font != prev.frags[0].font {
                continue;
            }

            let delta = prev.y - cur.y;
            if delta < MAX_LINE_HEIGHT {
                if cur.frags[0].font == Font::Code {
                    // These constants found manually :(
                    let indent = (cur.frags[0].pos.x - LEFT_MARGIN) as f32 / MONOSPACE_WIDTH as f32;
                    prev.frags[0]
                        .text
                        .push_str(&format!("\n{}", " ".repeat(indent as usize)));
                }
                prev.frags[0].text.push_str(&cur.frags[0].text);
                self.lines.remove(i);
            }
        }
    }

    fn render(&self) {
        for line in &self.lines {
            if line.frags.len() == 1 {
                let frag = &line.frags[0];
                match frag.font {
                    Font::Heading => println!("# {}\n", frag.text),
                    Font::SubHeading => println!("## {}\n", frag.text),
                    Font::Body => println!("{}\n", frag.text),
                    Font::Code => println!("```\n{}\n```\n", frag.text),
                    Font::Unknown(_, _) => panic!("{:?}", frag),
                };
            } else {
                println!(
                    "| {} |",
                    line.frags
                        .iter()
                        .map(|f| f.text.as_str())
                        .collect::<Vec<_>>()
                        .join(" | ")
                );
            }
        }
    }
}

impl hayro_interpret::Device<'_> for Doc {
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
