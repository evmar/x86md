//! Analyze a rendered PDF page to extract Block like Heading and Table.

use crate::render::{Font, Line, Render, TextLine};

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

#[derive(Debug)]
pub struct Doc {
    pub page: usize,
    pub title: Option<String>,
    pub blocks: Vec<Block>,
}

pub fn analyze(render: Render) -> Vec<Block> {
    let Render {
        mut text_lines,
        mut horiz_lines,
        mut vert_lines,
    } = render;
    vert_lines = simplify_vert(vert_lines);
    text_lines.reverse();
    join_fragments(&mut text_lines);
    horiz_lines.sort();
    join_paragraphs(text_lines, horiz_lines, vert_lines)
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

        let mut prev = frags[0].clone();
        for cur in frags.into_iter().skip(1) {
            if cur.font != prev.font && cur.font != Font::Unknown {
                panic!("font mismatch {:?} vs {:?}", cur.font, prev.font);
            }
            let delta = cur.x.abs_diff(x);
            x = cur.x2;
            if delta < MAX_GLYPH_DELTA {
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
        // drop * footnote markers, their y pos confuses things; TODO
        !line
            .frags
            .iter()
            .all(|f| f.font == Font::Unknown && f.text == "*")
    });
}

/// For all the lines that are part of the same paragraph, join them into a single span of text if they are close enough together.
fn join_paragraphs(
    mut text_lines: Vec<TextLine>,
    horiz_lines: Vec<u32>,
    vert_lines: Vec<Line>,
) -> Vec<Block> {
    let trace = false;
    // work from bottom to top, merging upwards
    for i in (0..=text_lines.len() - 1).rev() {
        let cur = &mut text_lines[i];
        if trace {
            println!();
            println!("{i} {cur:?}");
        }

        let vert = vert_lines.iter().find(|l| (l.y1..l.y2).contains(&cur.y));
        if let Some(vert) = vert
            && vert.x1 > LEFT_MARGIN
        {
            if trace {
                println!("dropping figure");
            }
            text_lines.remove(i);
            continue;
        }
        let in_table = vert.is_some();

        if i == 0 {
            break;
        }
        for prev_i in (0..=i - 1).rev() {
            let [cur, prev] = text_lines.get_disjoint_mut([i, prev_i]).unwrap();
            if trace {
                println!("{i}/{prev_i}");
            }

            // If there's a border between these two lines, never merge.
            let horiz = horiz_lines.iter().find(|l| (cur.y..prev.y).contains(&l));
            if horiz.is_some() {
                if trace {
                    println!("border {horiz:?}, abort");
                }
                break;
            }

            let ydelta = prev.y.abs_diff(cur.y);
            if trace {
                println!("ydelt {ydelta} table={in_table}");
            }

            let max_line_height = if in_table { 150 } else { 130 };
            if ydelta >= max_line_height {
                break;
            }

            if !in_table && cur.frags[0].font == Font::Code && prev.frags[0].font == Font::Code {
                if trace {
                    println!("code, merge");
                }
                assert!(cur.frags.len() == 1);
                if prev.frags.len() != 1 {
                    let text = prev
                        .frags
                        .iter()
                        .map(|f| f.text.as_str())
                        .collect::<Vec<_>>()
                        .join("  ");
                    prev.frags[0].text = text;
                    prev.frags.truncate(1);
                }
                let cur_frag = &mut cur.frags[0];
                let prev_frag = &mut prev.frags[0];
                let indent = (cur_frag.x - LEFT_MARGIN) as f32 / MONOSPACE_WIDTH as f32;
                prev_frag
                    .text
                    .push_str(&format!("\n{}", " ".repeat(indent as usize)));
                prev_frag.text.push_str(&cur_frag.text);
                text_lines.remove(i);
                break;
            } else {
                // match up cur/prev frags by x-position, handling plain text as well as tables
                let mut merged = false;
                for cur_frag in cur.frags.iter_mut() {
                    let Some(prev_frag) = prev
                        .frags
                        .iter_mut()
                        .find(|f| f.x.abs_diff(cur_frag.x) < 10)
                    else {
                        if trace {
                            println!("no x match");
                        }
                        continue;
                    };
                    if cur_frag.font != prev_frag.font {
                        if trace {
                            println!("font change");
                        }
                        continue;
                    }

                    // If we merge two sentences across two lines, we need to insert a space,
                    // but if we merge lines that don't expect whitespace then we don't need a space.
                    let end = prev_frag.text.chars().last().unwrap();
                    if !['/', '-', ' '].contains(&end) {
                        prev_frag.text.push_str(" ");
                    }
                    prev_frag.text.push_str(&cur_frag.text);
                    cur_frag.text.clear();
                    merged = true;
                }
                if trace {
                    println!("merged={merged:?}");
                }
                if merged {
                    if !cur.frags.iter().all(|f| f.text.is_empty()) {
                        panic!(
                            "merged some but had leftovers:\n  {:?}\nprev\n  {:?}",
                            cur.frags, prev.frags
                        );
                    }
                    text_lines.remove(i);
                    break;
                }
            }
        }
    }

    let mut blocks = Vec::new();
    for TextLine { mut frags, .. } in text_lines {
        if frags.len() == 1 {
            let frag = frags.pop().unwrap();
            if matches!(frag.font, Font::Unknown) {
                panic!("unknown font in {frag:?}");
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

            // merge row into existing table if the font matches
            if let Some(Block::Table(prev)) = blocks.last_mut()
                && (prev.last().unwrap().0 == Font::TableHeading || prev.last().unwrap().0 == font)
            {
                prev.push((font, text));
            } else {
                if matches!(font, Font::Unknown) {
                    eprintln!("omitting unknown font {:?}", text);
                    continue;
                }
                blocks.push(Block::Table(vec![(font, text)]));
            }
        }
    }
    blocks
}

/// Given a collection of vertical lines, return a set of just the leftmost line spans.
fn simplify_vert(mut lines: Vec<Line>) -> Vec<Line> {
    lines.sort_by_key(|l| l.x1);

    let mut ys = vec![];
    for line in lines.iter() {
        ys.push(line.y1);
        ys.push(line.y2);
    }
    ys.sort();
    ys.dedup();

    let mut lefts: Vec<Line> = vec![];
    let mut cur: Option<&mut Line> = None;
    for y in ys {
        let x = lines
            .iter()
            .filter(|l| (l.y1..l.y2).contains(&y))
            .map(|l| l.x1)
            .min();

        match (&mut cur, x) {
            (None, None) => panic!(),
            (Some(c), None) => {
                c.y2 = y;
                cur = None;
            }
            (None, Some(x)) => {
                lefts.push(Line {
                    x1: x,
                    y1: y,
                    y2: 0,
                });
                cur = lefts.last_mut();
            }
            (Some(c), Some(x)) => {
                if x >= c.x1 {
                    continue;
                }
                c.y2 = y;
                lefts.push(Line {
                    x1: x,
                    y1: y,
                    y2: 0,
                });
                cur = lefts.last_mut();
            }
        }
    }
    assert!(cur.is_none());

    // drop tiny lines, seen at edges of tables
    lefts.retain(|l| l.y2.abs_diff(l.y1) > 1);
    lefts
}
