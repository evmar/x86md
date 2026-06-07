#!/bin/sh

set -e

cargo run --release -- --pdf ~/win/intel-instructions.pdf --out-dir out --from 119 --to 144
