# Test font assets

`Cantarell-Regular.ttf` is used only by unit/render tests in this crate to
exercise real TTF parsing, rasterization, kerning, and shaping.

Cantarell is licensed under the **SIL Open Font License, Version 1.1**
(<https://openfontlicense.org>), which permits bundling and redistribution.
Copyright (c) The Cantarell Project Authors. "Cantarell" is a Reserved Font Name.

`Xolonium-Regular.ttf` is the OVERRIDE face for tests that need a font
DIFFERENT from the built-in default. ⚠️ `Cantarell-Regular.ttf` cannot serve
that purpose: it is byte-identical to `assets/default_font.ttf`, the face every
default-theme string is drawn with, so "override with Cantarell" changes no
pixels at all.

Xolonium is licensed under the **SIL Open Font License, Version 1.1** (full
text in `Xolonium-LICENSE.txt`). Copyright 2011-2016 Severin Meyer. "Xolonium"
is a Reserved Font Name. Taken from the official Godot demo "Dodge the Creeps".
