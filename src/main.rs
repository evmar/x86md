use pdf::content::{Matrix, Op, TextDrawAdjusted};

#[derive(Debug)]
struct Fragment {
    x: f32,
    y: f32,
    text: String,
}

fn extract_fragments(ops: &[Op]) -> Vec<Fragment> {
    let mut fragments = Vec::new();

    // matrix:
    //   (a, d): font w/h
    //   (b, c): shear (unused)
    //   (e, f): translation x/y
    let mut line_matrix = Matrix::default(); // start of line
    let mut text_matrix = Matrix::default(); // current text position
    let mut leading = 0.0;

    for op in ops {
        match op {
            Op::Leading { leading: value } => {
                leading = *value;
            }

            Op::SetTextMatrix { matrix } => {
                text_matrix = *matrix;
                line_matrix = *matrix;
            }

            Op::MoveTextPosition { translation } => {
                // translation is in text units, needs to be scaled by font size
                text_matrix.e += translation.x * line_matrix.a;
                text_matrix.f += translation.y * line_matrix.d;
                line_matrix = text_matrix;
            }

            Op::TextNewline => {
                line_matrix.f -= leading * line_matrix.d;
                text_matrix = line_matrix;
            }

            Op::TextDraw { text } => {
                fragments.push(Fragment {
                    x: text_matrix.e,
                    y: text_matrix.f,
                    text: text.to_string_lossy(),
                });
            }

            Op::TextDrawAdjusted { array } => {
                let mut text = String::new();
                for item in array {
                    match item {
                        TextDrawAdjusted::Text(s) => {
                            text.push_str(&format!("({})", &s.to_string_lossy()));
                        }
                        TextDrawAdjusted::Spacing(_) => {}
                    }
                }
                fragments.push(Fragment {
                    x: text_matrix.e,
                    y: text_matrix.f,
                    text,
                });
            }

            _ => {}
        }
    }

    fragments
}

fn main() {
    let args = std::env::args().collect::<Vec<_>>();
    let file = pdf::file::FileOptions::cached().open(&args[1]).unwrap();
    let resolver = file.resolver();
    println!("{} pages", file.pages().count());

    let first_page = 118;
    let page = file.get_page(first_page).unwrap();
    let content = page.contents.as_ref().unwrap();
    let ops = content.operations(&resolver).unwrap();
    let mut fragments = extract_fragments(&ops);
    fragments.sort_by(|a, b| b.y.total_cmp(&a.y).then(a.x.total_cmp(&b.x)));
    for fragment in fragments {
        println!("{:7.2} {:7.2} {}", fragment.x, fragment.y, fragment.text);
    }
}
