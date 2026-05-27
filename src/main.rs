use pdf::content::{Matrix, Op, TextDrawAdjusted};

#[derive(Debug)]
struct Fragment {
    x: f32,
    y: f32,
    text: String,
}

fn text_adjusted_text(array: &[TextDrawAdjusted]) -> String {
    let mut text = String::new();
    for item in array {
        if let TextDrawAdjusted::Text(s) = item {
            text.push_str(&s.to_string_lossy());
        }
    }
    text
}

fn extract_fragments(ops: &[Op]) -> Vec<Fragment> {
    let mut fragments = Vec::new();

    let mut text_matrix = Matrix::default();
    let mut line_matrix = Matrix::default();
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
                text_matrix.e += translation.x;
                text_matrix.f += translation.y;
                line_matrix = text_matrix;
            }

            Op::TextNewline => {
                line_matrix.f -= leading;
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
                fragments.push(Fragment {
                    x: text_matrix.e,
                    y: text_matrix.f,
                    text: text_adjusted_text(array),
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
    for fragment in extract_fragments(&ops) {
        println!("{:7.2} {:7.2} {}", fragment.x, fragment.y, fragment.text);
    }
}
