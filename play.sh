#!/bin/bash
export DISPLAY=:0
export XAUTHORITY=$HOME/.Xauthority
cargo run --release -- "${@:-nand/qiye.app}"
