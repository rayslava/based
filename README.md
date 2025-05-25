# BASic EDitor

An attempt to write a tiny embeddable editor for Linux without using any
external dependencies even libc.

Basically this is an experinment inspired by
[kilo](https://github.com/antirez/kilo) and
[tiny-rust-demo](https://github.com/kmcallister/tiny-rust-demo) so don't expect
it to be a full-featured tool.

## Keybindings

The editor supports some of Emacs default keybinds

- C-x C-f and C-x C-s to find and save file
- Default movement with C-f, C-b, C-p, C-n, C-a, C-e
- Kill-buffer (not ring) with C-k, C-w, M-w, M-y and marking with C-SPC

## Features

- Open/Save file
- Create new file (just find the new name with C-x C-f)
- Highlight for some keywords

# Build Status

[![Build](https://github.com/rayslava/based/actions/workflows/rust.yaml/badge.svg)](https://github.com/rayslava/based/actions/workflows/rust.yaml)[![codecov](https://codecov.io/gh/rayslava/based/graph/badge.svg?token=JU0J4SAWGF)](https://codecov.io/gh/rayslava/based)
