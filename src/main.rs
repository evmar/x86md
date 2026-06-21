use crate::analyze::{Block, Doc};

mod analyze;
mod markdown;
mod render;

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

    let pdf = render::PDF::parse(data);
    let mut render = render::Renderer::new(&pdf);

    let mut pages_written = vec![];

    let mut docs = (args.from..=args.to)
        .map(|page| {
            println!("processing {}", page);
            let r = render.page(page);
            let blocks = analyze::analyze(r);
            let title = if let Block::Heading(1, title) = &blocks[0] {
                Some(title.clone())
            } else {
                None
            };
            Doc {
                page,
                title,
                blocks,
            }
        })
        .peekable();

    while let Some(mut doc) = docs.next() {
        while let Some(doc2) = docs.next_if(|doc| doc.title.is_none()) {
            doc.blocks.extend(doc2.blocks);
        }
        let name = markdown::write_file(&args.out_dir, doc)?;
        pages_written.push(name);
    }

    std::fs::write(
        format!("{out_dir}/index.md", out_dir = &args.out_dir),
        &std::fs::read("README.md")?,
    )?;

    Ok(())
}
