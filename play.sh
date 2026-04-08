#!/bin/bash
export DISPLAY=:0
export XAUTHORITY=$HOME/.Xauthority
killall vdingoo
cargo run --release -- "${@:-nand/qiye.app}"
