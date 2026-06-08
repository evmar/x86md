use crate::analyze::Block;

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
    let mut full_page = vec![];
    for page in args.from..=args.to {
        eprintln!("processing {}", page);
        let parse = render.page(page);
        let doc = analyze::analyze(parse);
        if let Block::Heading(1, title) = &doc[0] {
            if !full_page.is_empty() {
                let name = markdown::write_file(&args.out_dir, std::mem::take(&mut full_page))?;
                pages_written.push(name);
            }
            eprintln!("{page}: {title}");
        }
        full_page.extend(doc);
    }
    pages_written.push(markdown::write_file(&args.out_dir, full_page)?);

    std::fs::write(
        format!("{out_dir}/index.md", out_dir = &args.out_dir),
        &std::fs::read("README.md")?,
    )?;

    Ok(())
}
