# Bundled default font

`default_font.ttf` is the engine's built-in default UI font, embedded into the
`gdrender2d` binary (via `TtfFont::builtin`) so text renders without an
explicit font resource — mirroring Godot's built-in default font.

It is **Open Sans SemiBold**, the face Godot itself embeds as its default theme
font (`upstream/godot/thirdparty/fonts/OpenSans_SemiBold.woff2`, see
`scene/theme/SCsub`), converted losslessly from WOFF2 to TTF with fontTools
because `ab_glyph` does not read WOFF2. Drawing Godot's own face is what makes
default-theme text measure and draw at Godot's sizes (pat-9ww3e); until then
this file was Cantarell Regular.

Open Sans is licensed under the **SIL Open Font License, Version 1.1**
(<https://openfontlicense.org>), which permits bundling and redistribution.
Copyright 2020 The Open Sans Project Authors
(<https://github.com/googlefonts/opensans>).
