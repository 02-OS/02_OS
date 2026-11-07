#!/usr/bin/env python3
"""Regenerate compact Plymouth assets from the approved 02 vector master.

Requires Python, pycairo, and Pillow only when editing the art. The ISO build
uses the committed PNGs and needs no render-time Python dependencies.
"""
from __future__ import annotations

import io
import json
import re
import xml.etree.ElementTree as ET
from pathlib import Path

import cairo
from PIL import Image

ROOT = Path(__file__).resolve().parents[2]
OUTPUT = ROOT / "profile/airootfs/usr/share/plymouth/themes/02-zero-portal"
WIDTH = 1280
CANVAS = (512, 448)


def logo_style(master):
    gradient = next(element for element in master.iter()
                    if element.tag.split("}")[-1] == "linearGradient")
    points = [float(gradient.get(key)) for key in ("x1", "y1", "x2", "y2")]
    stops = []
    for stop in gradient:
        color = stop.get("stop-color").lstrip("#")
        stops.append([float(stop.get("offset")),
                      *(int(color[index:index + 2], 16) / 255 for index in (0, 2, 4))])
    stroke = next(float(element.get("stroke-width")) for element in master.iter()
                  if element.get("stroke-width") is not None)
    return points, stops, stroke


def path_commands(path):
    tokens = re.findall(r"[MLCQVHZ]|[-+]?(?:\d*\.\d+|\d+)(?:[eE][-+]?\d+)?", path)
    position = 0
    current = (0, 0)
    start = current
    commands = []
    while position < len(tokens):
        operation = tokens[position]
        position += 1
        if operation == "Z":
            commands.append(["Z"])
            current = start
            continue
        count = {"M": 2, "L": 2, "C": 6, "Q": 4, "V": 1, "H": 1}[operation]
        arguments = [float(value) for value in tokens[position:position + count]]
        position += count
        if operation == "M":
            commands.append(["M", *arguments])
            current = start = tuple(arguments)
        elif operation == "L":
            commands.append(["L", *arguments])
            current = tuple(arguments)
        elif operation == "C":
            commands.append(["C", *arguments])
            current = tuple(arguments[-2:])
        elif operation == "Q":
            x0, y0 = current
            x1, y1, x2, y2 = arguments
            commands.append(["C", x0 + 2 * (x1 - x0) / 3, y0 + 2 * (y1 - y0) / 3,
                             x2 + 2 * (x1 - x2) / 3, y2 + 2 * (y1 - y2) / 3, x2, y2])
            current = (x2, y2)
        elif operation == "V":
            current = (current[0], arguments[0])
            commands.append(["L", *current])
        elif operation == "H":
            current = (arguments[0], current[1])
            commands.append(["L", *current])
    return commands


def path_to_cairo(ctx, path):
    for operation, *arguments in path_commands(path):
        if operation == "M":
            ctx.move_to(*arguments)
        elif operation == "L":
            ctx.line_to(*arguments)
        elif operation == "C":
            ctx.curve_to(*arguments)
        else:
            ctx.close_path()


def draw_tree(ctx, element):
    ctx.save()
    transform = element.get("transform", "")
    if transform:
        match = re.fullmatch(r"translate\(([-\d.]+)\s+([-\d.]+)\)", transform)
        if not match:
            raise ValueError(f"Unsupported vector transform: {transform}")
        ctx.translate(*map(float, match.groups()))
    if element.tag.split("}")[-1] == "path":
        ctx.new_path()
        path_to_cairo(ctx, element.get("d"))
        if element.get("stroke") == "none":
            ctx.fill()
        else:
            ctx.stroke()
    for child in element:
        draw_tree(ctx, child)
    ctx.restore()


def logo_image(master, width=WIDTH, canvas=CANVAS, supersample=3):
    surface = cairo.ImageSurface(cairo.FORMAT_ARGB32,
                                 canvas[0] * supersample, canvas[1] * supersample)
    ctx = cairo.Context(surface)
    ctx.scale(supersample, supersample)
    ctx.translate(canvas[0] / 2, canvas[1] / 2)
    ctx.scale(width * 0.2375 / 680, width * 0.2375 / 680)
    points, stops, stroke = logo_style(master)
    ink = cairo.LinearGradient(*points)
    for stop in stops:
        ink.add_color_stop_rgb(*stop)
    ctx.set_source(ink)
    ctx.set_line_width(stroke)
    ctx.set_line_cap(cairo.LINE_CAP_ROUND)
    ctx.set_line_join(cairo.LINE_JOIN_ROUND)
    draw_tree(ctx, master)
    buffer = io.BytesIO()
    surface.write_to_png(buffer)
    buffer.seek(0)
    return Image.open(buffer).convert("RGBA").resize(canvas, Image.Resampling.LANCZOS)


def main():
    master_path = Path(__file__).with_name("02-logo.svg")
    master = ET.parse(master_path).getroot()
    OUTPUT.mkdir(parents=True, exist_ok=True)
    # Plymouth holds the logo; GNOME's boot-only overlay performs the actual
    # portal reveal over the live login screen in the same compositor.
    logo_image(master, width=2560, canvas=(1024, 896)).save(OUTPUT / "logo.png", optimize=True)
    # Both native stages use the same paths, including the filled shared base.
    paths = {element.get("id"): element for element in master.iter()
             if element.tag.split("}")[-1] == "path"}
    generated = "\n".join(
        f"const {name.upper()} = {json.dumps(path_commands(paths[name].get('d')))};"
        for name in ("zero", "two"))
    generated += "\nconst TWO_FILLED = " + str(paths["two"].get("stroke") == "none").lower() + ";"
    points, stops, stroke = logo_style(master)
    generated += f"\nconst LOGO_INK = {json.dumps([points, stops])};"
    generated += f"\nconst LOGO_STROKE_WIDTH = {stroke};"
    motion = ROOT / "profile/airootfs/usr/share/gnome-shell/extensions/02-zero-portal@02os/motion.js"
    source = motion.read_text()
    begin, end = "// BEGIN GENERATED LOGO", "// END GENERATED LOGO"
    assert source.count(begin) == source.count(end) == 1
    before, rest = source.split(begin)
    _, after = rest.split(end)
    motion.write_text(before + begin + "\n" + generated + "\n" + end + after)
    branding_svg = master_path.read_text().replace('width="680" height="620"',
                                                 'width="96" height="88"', 1)
    for relative in ("pixmaps/02os-logo.svg", "icons/hicolor/scalable/apps/02os-logo.svg",
                     "icons/02-OS/scalable/apps/02os-logo.svg"):
        (ROOT / "profile/airootfs/usr/share" / relative).write_text(branding_svg)
    assets = list(OUTPUT.glob("*.png"))
    print(f"Generated {len(assets)} PNG assets, {sum(path.stat().st_size for path in assets):,} bytes")


if __name__ == "__main__":
    main()
