use std::collections::HashMap;

use hayro_interpret::{Context, InterpreterCache, InterpreterSettings, hayro_syntax::Pdf};

fn main() {
    let args = std::env::args().collect::<Vec<_>>();
    let data = std::fs::read(&args[1]).unwrap();
    let pdf = Pdf::new(data).unwrap();

    const FIRST_PAGE: usize = 118;
    const LAST_PAGE: usize = 119;

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

    let mut full_page = vec![];
    for page in FIRST_PAGE..=LAST_PAGE {
        let page = &pdf.pages()[page];
        let mut device = Device::default();
        hayro_interpret::interpret_page(page, &mut context, &mut device);
        let doc = analyze(device.lines);
        full_page.extend(doc);
    }
    render(full_page);
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Font {
    Heading,
    SubHeading,
    TableHeading,
    Body,
    Code,
    Footer,
    Unknown(String, u32),
}

#[derive(Debug, Clone)]
struct Fragment {
    x: u32,
    font: Font,
    text: String,
}

#[derive(Debug)]
struct Line {
    y: u32,
    frags: Vec<Fragment>,
}

enum Block {
    Text(Font, String),
    Table(Vec<(Font, Vec<String>)>),
}

#[derive(Default)]
struct Device {
    // It is pretty difficult to figure out the font of a glyph in hayro because it
    // resolves the font rather than giving you the font id from the raw PDF format.
    // This map is keyed off of the `(font_cache_key, scale)` of the glyphs.
    fonts: HashMap<(u128, u32), Font>,
    lines: Vec<Line>,
}

impl Device {
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
            ("NeoSansIntelMedium", 9) => Font::TableHeading,
            ("NeoSansIntel", 8) => Font::Footer,
            ("Verdana", 9) => Font::Body,
            ("NeoSansIntel", 9) => Font::Code,
            _ => Font::Unknown(name.to_string(), scale),
        };
        self.fonts.insert((key, scale), font.clone());
        font
    }
}

fn analyze(mut lines: Vec<Line>) -> Vec<Block> {
    lines.reverse();
    join_fragments(&mut lines);
    join_paragraphs(lines)
}

/// For all the fragments that are within the same line, join them into a single fragment if they are close enough together.
fn join_fragments(lines: &mut [Line]) {
    // for computing when subsequent glyphs are part of the same span
    const MAX_GLYPH_WIDTH: u32 = 100;

    for line in lines {
        let mut frags = std::mem::take(&mut line.frags);
        frags.sort_by_key(|f| f.x);
        let mut joined = vec![];
        let mut x = frags[0].x;
        let mut cur = frags[0].clone();
        for frag in frags.into_iter().skip(1) {
            let delta = frag.x - x;
            x = frag.x;
            if delta < MAX_GLYPH_WIDTH {
                cur.text.push_str(&frag.text);
            } else {
                joined.push(cur);
                cur = frag;
            }
        }
        joined.push(cur);
        line.frags = joined;
    }
}

/// For all the lines that are part of the same paragraph, join them into a single span of text if they are close enough together.
fn join_paragraphs(mut lines: Vec<Line>) -> Vec<Block> {
    // These constants found manually :(
    // for computing when subsequent lines are part of the same paragraph
    const MAX_LINE_HEIGHT: u32 = 150;
    // for computing indentation in monospace blocks
    const LEFT_MARGIN: u32 = 460;
    const MONOSPACE_WIDTH: f32 = 50.0;

    for i in (1..lines.len() - 1).rev() {
        let [cur, prev] = lines.get_disjoint_mut([i, i - 1]).unwrap();
        if cur.frags.len() == 1 && prev.frags.len() == 1 {
            let cur_frag = &mut cur.frags[0];
            let prev_frag = &mut prev.frags[0];
            if cur_frag.font != prev_frag.font {
                continue;
            }
            let delta = prev.y - cur.y;
            if delta < MAX_LINE_HEIGHT {
                if cur_frag.font == Font::Code {
                    let indent = (cur_frag.x - LEFT_MARGIN) as f32 / MONOSPACE_WIDTH as f32;
                    prev_frag
                        .text
                        .push_str(&format!("\n{}", " ".repeat(indent as usize)));
                }
                prev_frag.text.push_str(&cur_frag.text);
                lines.remove(i);
            }
        } else {
            // table
            let mut merged = false;
            for cur_frag in cur.frags.iter_mut() {
                let Some(prev_frag) = prev.frags.iter_mut().find(|f| {
                    // /10 here because it appears off by 1 sometimes
                    f.x / 10 == cur_frag.x / 10
                }) else {
                    continue;
                };
                if cur_frag.font != prev_frag.font {
                    continue;
                }

                let delta = prev.y - cur.y;
                assert!(delta < MAX_LINE_HEIGHT);

                prev_frag.text.push_str(&cur_frag.text);
                cur_frag.text.clear();
                merged = true;
            }
            if merged {
                assert!(cur.frags.iter().all(|f| f.text.is_empty()));
                lines.remove(i);
            }
        }
    }

    let mut blocks = Vec::new();
    for Line { mut frags, .. } in lines {
        if frags.len() == 1 {
            let frag = frags.pop().unwrap();
            blocks.push(Block::Text(frag.font, frag.text));
        } else {
            let font = frags[0].font.clone();

            assert!(frags.iter().all(|f| f.font == font));
            let text = frags.into_iter().map(|f| f.text).collect();

            // merge row into previous table if the font matches
            if let Some(Block::Table(prev)) = blocks.last_mut()
                && (prev.last().unwrap().0 == Font::TableHeading || prev.last().unwrap().0 == font)
            {
                prev.push((font, text));
            } else {
                blocks.push(Block::Table(vec![(font, text)]));
            }
        }
    }
    blocks
}

/// Dump self as Markdown.
fn render(doc: Vec<Block>) {
    for block in doc {
        match block {
            Block::Text(font, text) => {
                match font {
                    Font::Heading => println!("# {}\n", text),
                    Font::SubHeading => println!("## {}\n", text),
                    Font::Body => println!("{}\n", text),
                    Font::Code => println!("```\n{}\n```\n", text),
                    _ => panic!("{font:?} {:?}", text),
                };
            }
            Block::Table(mut rows) => {
                if rows[0].0 == Font::Footer {
                    continue;
                }
                if rows[0].0 == Font::TableHeading {
                    let row = rows.remove(0);
                    println!("| {} |", row.1.join(" | "));
                } else {
                    println!("|{}", " |".repeat(rows[0].1.len()));
                };
                println!("|{}", " --- |".repeat(rows[0].1.len()));

                for (font, row) in rows {
                    let row = match font {
                        Font::Code => row
                            .into_iter()
                            .map(|s| format!("`{s}`"))
                            .collect::<Vec<_>>(),
                        Font::Body => row,
                        _ => panic!("{font:?} {:?}", row),
                    };
                    println!("| {} |", row.join(" | "));
                }
                println!();
            }
        }
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
        let pos = glyph_transform.translation();
        let x = (pos.x * 10.0) as u32;
        let y = (pos.y * 10.0) as u32;

        let line = match self.lines.binary_search_by_key(&y, |l| l.y) {
            Ok(i) => &mut self.lines[i],
            Err(i) => {
                self.lines.insert(i, Line { y, frags: vec![] });
                &mut self.lines[i]
            }
        };
        line.frags.push(Fragment { x, font, text });
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
