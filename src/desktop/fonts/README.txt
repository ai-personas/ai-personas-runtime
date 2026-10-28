Embedded UI fonts

Inter and DejaVu Sans Mono are the default proportional and monospaced fonts
in Blender's BLF_api.hh. These copies were downloaded on 2026-10-27 from:
https://github.com/blender/blender/tree/main/release/datafiles/fonts

Inter.woff2 was converted to a static TrueType instance (weight 400, optical
size 14), and DejaVuSansMono.woff2 was decompressed to TrueType with fontTools.
This is a build-time conversion; using Personas needs no font downloads or
system font installation. The original copyright notices remain embedded.

Inter: SIL Open Font License 1.1, see Inter-OFL.txt.
DejaVu Sans Mono: Bitstream Vera/DejaVu license, see DejaVu-LICENSE.txt.

These fonts are third-party assets and are not covered by this repository's
Apache-2.0 license. Personas is not affiliated with or endorsed by Blender.
