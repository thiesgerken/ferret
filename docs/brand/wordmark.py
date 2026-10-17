"""Outlines the "ferret" wordmark and the mark-and-wordmark lockups, see README.md.

    uv run --with fonttools --with uharfbuzz docs/brand/wordmark.py <Alegreya[wght].ttf>

Writes wordmark.svg, logo-light.svg and logo-dark.svg next to this script.
"""

import re
import sys
from pathlib import Path

import uharfbuzz as hb
from fontTools.pens.boundsPen import BoundsPen
from fontTools.pens.svgPathPen import SVGPathPen
from fontTools.pens.transformPen import TransformPen
from fontTools.ttLib import TTFont
from fontTools.varLib.instancer import instantiateVariableFont

BRAND = Path(__file__).parent
TEXT = "ferret"
# SemiBold, as the UI's `font-wordmark`.
WEIGHT = 600
# The mark's crop in its 32×32 box: from the top of the ear to the flat base, as in the UI.
MARK_TOP, MARK_BASE = 3.4, 28.15
# Next to the wordmark the mark is 0.8 em high and 0.25 em away (the UI's `gap-1.5` at 24 px).
MARK_HEIGHT_EM, GAP_EM = 0.8, 0.25


def outline(font_path: str) -> tuple[str, float, tuple[float, float, float, float]]:
    """The text as one SVG path with its baseline at y = 0, its em size and its bounds."""
    font = instantiateVariableFont(TTFont(font_path), {"wght": WEIGHT})
    upm = font["head"].unitsPerEm
    blob = hb.Blob.from_file_path(font_path)
    hb_font = hb.Font(hb.Face(blob))
    hb_font.set_variations({"wght": WEIGHT})
    buffer = hb.Buffer()
    buffer.add_str(TEXT)
    buffer.guess_segment_properties()
    # Shaping applies the font's kerning and ligatures, which drawing glyph by glyph would miss.
    hb.shape(hb_font, buffer, {"kern": True, "liga": True})

    glyphs = font.getGlyphSet()
    order = font.getGlyphOrder()
    # A tenth of a font unit is far below a pixel at any size the wordmark is shown at.
    pen = SVGPathPen(glyphs, ntos=lambda n: f"{n:.1f}".removesuffix(".0"))
    bounds = BoundsPen(glyphs)
    x = 0
    for info, position in zip(buffer.glyph_infos, buffer.glyph_positions):
        glyph = glyphs[order[info.codepoint]]
        # Font units grow upwards, SVG downwards.
        transform = (1, 0, 0, -1, x + position.x_offset, -position.y_offset)
        glyph.draw(TransformPen(pen, transform))
        glyph.draw(TransformPen(bounds, transform))
        x += position.x_advance
    return pen.getCommands(), upm, bounds.bounds


def mark_paths(variant: str) -> str:
    svg = (BRAND / f"mark-{variant}.svg").read_text()
    return "\n".join(re.findall(r"<path [^>]*/>", svg))


def main(font_path: str) -> None:
    path, upm, (left, top, right, bottom) = outline(font_path)
    (BRAND / "wordmark.svg").write_text(
        f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="{left:.0f} {top:.0f} {right - left:.0f} {bottom - top:.0f}">\n'
        f'  <path fill="currentColor" d="{path}"/>\n</svg>\n'
    )

    mark_height = MARK_HEIGHT_EM * upm
    scale = mark_height / (MARK_BASE - MARK_TOP)
    mark_width = 32 * scale
    text_x = mark_width + GAP_EM * upm
    box_top, box_bottom = min(top, -mark_height), max(bottom, 0)
    for variant, colour in [("light", "#885629"), ("dark", "#d0975f")]:
        (BRAND / f"logo-{variant}.svg").write_text(
            f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 {box_top:.0f} {text_x + right:.0f} {box_bottom - box_top:.0f}">\n'
            # The mark sits on the wordmark's baseline with its flat base.
            f'  <g transform="translate(0 {-mark_height:.1f}) scale({scale:.4f}) translate(0 {-MARK_TOP})">\n'
            f"    {mark_paths(variant)}\n"
            f"  </g>\n"
            f'  <path fill="{colour}" transform="translate({text_x:.1f} 0)" d="{path}"/>\n'
            f"</svg>\n"
        )


if __name__ == "__main__":
    main(sys.argv[1])
