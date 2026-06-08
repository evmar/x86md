use std::collections::HashMap;

use hayro_interpret::{Context, InterpreterCache, InterpreterSettings, hayro_syntax::Pdf};

// for computing indentation in monospace blocks
const LEFT_MARGIN: u32 = 460;
const MONOSPACE_WIDTH: f32 = 50.0;

/// generate markdown documentation from Intel PDF manuals
#[derive(argh::FromArgs)]
struct Args {
    /// path to the PDF file
    #[argh(option)]
    pdf: String,

    /// path to the output directory
    #[argh(option)]
    out_dir: String,

    /// first page to process
    #[argh(option)]
    from: usize,

    /// last page to process
    #[argh(option)]
    to: usize,
}

fn main() -> std::io::Result<()> {
    let args: Args = argh::from_env();
    let data = std::fs::read(args.pdf).unwrap();
    std::fs::create_dir_all(&args.out_dir).unwrap();
    let pdf = Pdf::new(data).unwrap();

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

    let mut pages_written = vec![];
    let mut full_page = vec![];
    for page in args.from..=args.to {
        eprintln!("processing {}", page);
        let pdf_page = &pdf.pages()[page - 1];
        let mut device = Device::default();
        hayro_interpret::interpret_page(pdf_page, &mut context, &mut device);
        let doc = analyze(device.text_lines, device.horiz_lines);
        if let Block::Heading(1, title) = &doc[0] {
            if !full_page.is_empty() {
                let name = write_file(&args.out_dir, std::mem::take(&mut full_page))?;
                pages_written.push(name);
            }
            eprintln!("{page}: {title}");
        }
        full_page.extend(doc);
    }
    pages_written.push(write_file(&args.out_dir, full_page)?);

    std::fs::write(
        format!("{out_dir}/index.md", out_dir = &args.out_dir),
        &std::fs::read("README.md")?,
    )?;

    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Font {
    Heading,
    SubHeading,
    TableHeading,
    Body,
    Code,
    Footer,
    Unknown,
}

#[derive(Debug, Clone)]
struct Fragment {
    x: u32,
    x2: u32,
    font: Font,
    text: String,
}

#[derive(Debug)]
struct TextLine {
    y: u32,
    frags: Vec<Fragment>,
}

#[derive(Debug)]
enum Block {
    Heading(u32, String),
    Text(String),
    Code(String),
    Table(Vec<(Font, Vec<String>)>),
}

#[derive(Default)]
struct Device {
    // It is pretty difficult to figure out the font of a glyph in hayro because it
    // resolves the font rather than giving you the font id from the raw PDF format.
    // This map is keyed off of the `(font_cache_key, scale)` of the glyphs.
    fonts: HashMap<(u128, u32), Font>,
    text_lines: Vec<TextLine>,

    /// y coords of any horizontal lines, so that we never merge text across table borders.
    horiz_lines: Vec<u32>,
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
            assert_eq!(s, (c[3] * 1000.0).round() as u32);
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

fn analyze(mut text_lines: Vec<TextLine>, mut horiz_lines: Vec<u32>) -> Vec<Block> {
    horiz_lines.sort();
    text_lines.reverse();
    join_fragments(&mut text_lines);
    join_paragraphs(text_lines, horiz_lines)
}

/// For all the fragments that are within the same line, join them into a single fragment if they are close enough together.
fn join_fragments(text_lines: &mut Vec<TextLine>) {
    // for computing when subsequent glyphs are part of the same span
    const MAX_GLYPH_DELTA: u32 = 30;

    for line in text_lines.iter_mut() {
        let mut frags = std::mem::take(&mut line.frags);
        frags.sort_by_key(|f| f.x);
        let mut joined = vec![];
        let mut x = frags[0].x2;
        // Sometimes code blocks will have comments spaced way out to the side,
        // which looks like a table.  Detect it by noticing it when things are indented.
        let indented_code =
            frags[0].font == Font::Code && x > LEFT_MARGIN + (8 * MONOSPACE_WIDTH as u32);

        let mut prev = frags[0].clone();
        for cur in frags.into_iter().skip(1) {
            if cur.font != prev.font && cur.font != Font::Unknown {
                panic!("font mismatch {:?} vs {:?}", cur.font, prev.font);
            }
            let delta = cur.x.abs_diff(x);
            x = cur.x2;
            if delta < MAX_GLYPH_DELTA {
                prev.text.push_str(&cur.text);
            } else if indented_code {
                prev.text.push_str("  ");
                prev.text.push_str(&cur.text);
            } else {
                joined.push(prev);
                prev = cur;
            }
        }
        joined.push(prev);
        line.frags = joined;
    }

    text_lines.retain(|line| {
        !line
            .frags
            .iter()
            .all(|f| f.font == Font::Unknown && f.text == "*")
    });
}

/// For all the lines that are part of the same paragraph, join them into a single span of text if they are close enough together.
fn join_paragraphs(mut text_lines: Vec<TextLine>, horiz_lines: Vec<u32>) -> Vec<Block> {
    for i in (1..text_lines.len() - 1).rev() {
        let [cur, prev] = text_lines.get_disjoint_mut([i, i - 1]).unwrap();
        let indented = cur.frags[0].x > LEFT_MARGIN + (8 * MONOSPACE_WIDTH as u32);

        if cur.frags.len() == 1
            && prev.frags.len() == 1
            && cur.frags[0].font == Font::Code
            && prev.frags[0].font == Font::Code
        {
            let cur_frag = &mut cur.frags[0];
            let prev_frag = &mut prev.frags[0];
            let indent = (cur_frag.x - LEFT_MARGIN) as f32 / MONOSPACE_WIDTH as f32;
            prev_frag
                .text
                .push_str(&format!("\n{}", " ".repeat(indent as usize)));
            prev_frag.text.push_str(&cur_frag.text);
            text_lines.remove(i);
        } else {
            // match up cur/prev frags by x-position, handling plain text as well as tables

            // If there's a border between these two lines, never merge.
            let pos = horiz_lines.binary_search(&cur.y).unwrap_or_else(|i| i);
            let border = horiz_lines.get(pos).unwrap_or(&0);
            if (cur.y..prev.y).contains(border) {
                continue;
            }

            let mut merged = false;
            for cur_frag in cur.frags.iter_mut() {
                let Some(prev_frag) = prev
                    .frags
                    .iter_mut()
                    .find(|f| f.x.abs_diff(cur_frag.x) < 10)
                else {
                    continue;
                };
                if cur_frag.font != prev_frag.font {
                    continue;
                }

                // If there's no text on the left, it's more likely to be a continuation of a table cell
                // above, so relax the line height requirement a bit.
                let max_line_height = if !indented { 120 } else { 150 };
                let delta = prev.y.abs_diff(cur.y);
                if delta < max_line_height {
                    // If we merge two sentences across two lines, we need to insert a space,
                    // but if we merge lines that don't expect whitespace then we don't need a space.
                    // I guess we can just guess.
                    if prev_frag.text.ends_with(".") {
                        prev_frag.text.push_str(" ");
                    }
                    prev_frag.text.push_str(&cur_frag.text);
                    cur_frag.text.clear();
                    merged = true;
                }
            }
            if merged {
                if !cur.frags.iter().all(|f| f.text.is_empty()) {
                    panic!("merged but leftover {:?}, prev {:?}", cur.frags, prev.frags);
                }
                text_lines.remove(i);
            }
        }
    }

    let mut blocks = Vec::new();
    for TextLine { mut frags, .. } in text_lines {
        if frags.len() == 1 {
            let frag = frags.pop().unwrap();
            if matches!(frag.font, Font::Unknown) {
                panic!();
            }
            match frag.font {
                Font::Heading => blocks.push(Block::Heading(1, frag.text)),
                Font::SubHeading => blocks.push(Block::Heading(2, frag.text)),
                Font::TableHeading => blocks.push(Block::Text(frag.text)),
                Font::Body => blocks.push(Block::Text(frag.text)),
                Font::Code => blocks.push(Block::Code(frag.text)),
                Font::Footer => {}
                _ => panic!("{:?}", frag.font),
            }
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

/// Dump a document as Markdown.
fn render(w: &mut dyn std::io::Write, doc: Vec<Block>) -> std::io::Result<()> {
    for block in doc {
        match block {
            Block::Heading(1, text) => writeln!(w, "# {}\n", text)?,
            Block::Heading(2, text) => writeln!(w, "## {}\n", text)?,
            Block::Text(text) => writeln!(w, "{}\n", text)?,
            Block::Code(text) => writeln!(w, "```\n{}\n```\n", text)?,
            Block::Table(mut rows) => {
                if rows[0].0 == Font::Footer {
                    continue;
                }
                if rows[0].0 == Font::TableHeading {
                    let row = rows.remove(0);
                    writeln!(w, "| {} |", row.1.join(" | "))?;
                } else {
                    writeln!(w, "|{}", " |".repeat(rows[0].1.len()))?;
                };
                writeln!(w, "|{}", " --- |".repeat(rows[0].1.len()))?;

                for (font, row) in rows {
                    let row = match font {
                        // Intentionally ignore code here, because it makes the HTML output look bad.
                        Font::Body | Font::Code => row,
                        _ => panic!("table unexpected font {font:?} {:?}", row),
                    };
                    writeln!(w, "| {} |", row.join(" | "))?;
                }
                writeln!(w)?;
            }
            _ => panic!("{block:?}"),
        }
    }
    Ok(())
}

fn write_file(out_dir: &str, doc: Vec<Block>) -> std::io::Result<String> {
    let Block::Heading(1, title) = &doc[0] else {
        panic!();
    };
    let title = title
        .chars()
        .take_while(|&c| c <= 'z')
        .collect::<String>()
        .to_ascii_lowercase();

    let path = format!("{out_dir}/{title}.md");
    {
        let mut w = std::fs::File::create(&path)?;
        render(&mut w, doc)?;
    }
    eprintln!("wrote {path}");
    Ok(title)
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
        let pos = glyph_transform.translation();
        let x = (pos.x * 10.0).round() as u32;
        let advance = glyph.advance_width().unwrap_or(0.0) as u32 / 10;
        let x2 = x + advance;
        let y = (pos.y * 10.0).round() as u32;

        let line = match self.text_lines.binary_search_by_key(&y, |l| l.y) {
            Ok(i) => &mut self.text_lines[i],
            Err(i) => {
                self.text_lines.insert(i, TextLine { y, frags: vec![] });
                &mut self.text_lines[i]
            }
        };
        line.frags.push(Fragment { x, x2, font, text });
    }

    fn draw_path(
        &mut self,
        path: &kurbo::BezPath,
        _transform: kurbo::Affine,
        _paint: &hayro_interpret::Paint<'_>,
        _draw_mode: &hayro_interpret::PathDrawMode,
    ) {
        for seg in path.segments() {
            match seg {
                kurbo::PathSeg::Line(line) => {
                    let (x1, y1) = (
                        (line.p0.x * 10.0).round() as u32,
                        (line.p0.y * 10.0).round() as u32,
                    );
                    let (x2, y2) = (
                        (line.p1.x * 10.0).round() as u32,
                        (line.p1.y * 10.0).round() as u32,
                    );
                    let x_delta = x1.abs_diff(x2);
                    let y_delta = y1.abs_diff(y2);
                    if x_delta == 0 {
                    } else if y_delta == 0 {
                        self.horiz_lines.push(y1);
                    } else {
                        panic!();
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
