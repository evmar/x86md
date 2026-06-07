# x86md

The Intel Software Developer's Manual Volume 2 documents each x86 machine instruction,
but is only available as PDF.  This project extracts the text/tables from the PDF and
generates Markdown and renders it to [HTML, browseable online](https://evmar.github.io/x86md/).

This is just like [x86doc](https://github.com/fay59/x86doc) but:

1. rewritten in Rust (tm);
2. with Markdown output;
3. prettier HTML.

## Developing

Run it like:

```
$ cargo run --release -- --pdf path/to/intel.pdf --out-dir out --from 119 --to 128
```
