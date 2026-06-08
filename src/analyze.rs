//! Analyze a rendered PDF page to extract Block like Heading and Table.

use crate::render::{Font, Render, TextLine};

// for computing indentation in monospace blocks
const LEFT_MARGIN: u32 = 460;
const MONOSPACE_WIDTH: f32 = 50.0;

#[derive(Debug)]
pub enum Block {
    Heading(u32, String),
    Text(String),
    Code(String),
    Table(Vec<(Font, Vec<String>)>),
}

pub fn analyze(render: Render) -> Vec<Block> {
    let Render {
        mut text_lines,
        mut horiz_lines,
    } = render;
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
