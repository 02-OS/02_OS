import Cairo from 'cairo';

// This is the approved scripts/boot-animation/02-logo.svg geometry. The first
// frame matches Plymouth's retained logo, including its short-display fit.
export const DURATION_SECONDS = 2.6;
const WHITE = [249 / 255, 249 / 255, 247 / 255];
// Generated from the SVG master by generate_assets.py.
// BEGIN GENERATED LOGO
const ZERO = [["M", 0.6484, -266.1914], ["C", -73.6602, -266.1914, -131.3516, -210.0391, -131.3516, -134.6328], ["L", -131.3516, 146.5156], ["C", -131.3516, 210.2656, -82.8047, 258.7734, -18.8984, 258.7734], ["L", 20.1914, 258.7734], ["C", 84.0977, 258.7734, 132.6484, 210.2656, 132.6484, 146.5156], ["L", 132.6484, -134.6328], ["C", 132.6484, -210.0391, 74.957, -266.1914, 0.6484, -266.1914], ["Z"]];
const TWO = [["M", -4.1094, -133.4727], ["C", -5.3008, -217.668, 66.3789, -283.5312, 156.9609, -282.4844], ["C", 250.2383, -285.3164, 320.3008, -217.1797, 318.5039, -129.3789], ["C", 319.4336, -81.7344, 303.0703, -45.2812, 270.8047, -12.5312], ["L", 41.5312, 227.3477], ["C", 36.0586, 233.0938, 36.3555, 238.1719, 42.418, 242.5781], ["L", 300.0469, 242.668], ["C", 309.043, 242.668, 316.332, 249.6055, 316.332, 258.1602], ["C", 316.332, 267.043, 309.0977, 274.2461, 300.1719, 274.2461], ["L", -150.0, 274.1406], ["L", -149.9492, 242.2344], ["L", -79.5664, 247.4297], ["C", -40.3242, 253.918, -7.6016, 235.0078, 10.793, 213.125], ["L", 248.375, -34.8867], ["C", 273.8555, -60.418, 287.4141, -90.7891, 286.7188, -129.6445], ["C", 288.8711, -198.5, 232.4844, -253.4727, 157.1836, -250.4609], ["C", 83.8594, -250.9688, 25.7344, -199.2812, 27.1172, -132.7148], ["C", 27.1172, -124.0898, 20.125, -117.1016, 11.5039, -117.1016], ["C", 2.8789, -117.1016, -4.1094, -124.0898, -4.1094, -133.4727], ["Z"]];
const TWO_FILLED = true;
const LOGO_INK = [[-280.0, 280.0, 300.0, -280.0], [[0.0, 0.0, 0.5490196078431373, 0.9725490196078431], [1.0, 0.0, 0.8431372549019608, 0.996078431372549]]];
const LOGO_STROKE_WIDTH = 32.1995;
// END GENERATED LOGO

function setLogoInk(cr, alpha, grow = 1, shiftX = 0) {
    // Logo-space gradient. shiftX/grow compensate when the pen is on the 0.
    const [x1, y1, x2, y2] = LOGO_INK[0];
    const ink = new Cairo.LinearGradient(
        (x1 - shiftX) / grow, y1 / grow,
        (x2 - shiftX) / grow, y2 / grow);
    for (const stop of LOGO_INK[1])
        ink.addColorStopRGBA(...stop, alpha);
    cr.setSource(ink);
}

// Smootherstep: zero first and second derivative at both ends, no overshoot.
function smooth(value) {
    const t = Math.max(0, Math.min(1, value));
    return t * t * t * (t * (t * 6 - 15) + 10);
}

function mix(from, to, amount) {
    return from + (to - from) * amount;
}

function drawPath(cr, commands) {
    for (const [command, ...values] of commands) {
        if (command === 'M')
            cr.moveTo(...values);
        else if (command === 'C')
            cr.curveTo(...values);
        else if (command === 'Z')
            cr.closePath();
        else
            cr.lineTo(...values);
    }
}

function zeroPath(cr) {
    drawPath(cr, ZERO);
}

function drawTwo(cr, slide) {
    cr.save();
    // Only the area to the right of the 0 is visible. The 2 moves behind that
    // silhouette rather than fading or reappearing from its left side.
    cr.rectangle(-170, -2000, 10170, 4000);
    cr.clip();
    cr.rectangle(-10000, -10000, 20000, 20000);
    cr.save();
    cr.translate(-170, 0);
    zeroPath(cr);
    cr.restore();
    cr.setFillRule(Cairo.FillRule.EVEN_ODD);
    cr.clip();
    cr.setFillRule(Cairo.FillRule.WINDING);
    cr.translate(-500 * slide, 0);
    setLogoInk(cr, 1);
    cr.newPath();
    drawPath(cr, TWO);
    if (TWO_FILLED)
        cr.fill();
    else
        cr.stroke();
    cr.restore();
}

export function drawPortalFrame(cr, width, height, elapsed) {
    // St.DrawingArea reuses its backing texture: clear it on every repaint so
    // an expanding hole does not retain white pixels from the previous frame.
    cr.setOperator(Cairo.Operator.CLEAR);
    cr.paint();
    cr.setOperator(Cairo.Operator.OVER);
    if (elapsed >= DURATION_SECONDS)
        return;

    const canvasWidth = Math.max(1, Math.floor(Math.min(width * 0.4,
        height * 0.66 * 512 / 448)));
    const scale = canvasWidth * 0.59375 / 680;
    // The 0 stays at its place in the logo and keeps its tall oval.
    const originX = -170;
    const originY = -4;
    const x = originX * scale + width / 2;
    const y = height / 2 + originY * scale;
    const halfWidth = 132;
    const expand = elapsed < 0.95 ? 0 : smooth((elapsed - 0.95) / 1.65);
    let cover = 1;
    for (const px of [0, width]) {
        for (const py of [0, height]) {
            const dx = (px - x) / scale;
            const dy = (py - y) / scale;
            const clamped = Math.max(-130, Math.min(130, dy));
            cover = Math.max(cover, Math.hypot(dx, dy - clamped) / halfWidth);
        }
    }
    const grow = mix(1, cover + LOGO_STROKE_WIDTH / halfWidth, expand);
    // The hole eases open across the growth instead of punching through in one step.
    const open = elapsed < 1.1 ? 0 : smooth((elapsed - 1.1) / 0.9);
    const innerGrow = Math.max(0, (grow - LOGO_STROKE_WIDTH / 2 / halfWidth) * open);
    // The last of the white rim dissolves, so the overlay does not snap off.
    const veil = expand < 0.72 ? 1 : 1 - smooth((expand - 0.72) / 0.28);

    cr.setSourceRGBA(...WHITE, veil);
    cr.rectangle(0, 0, width, height);
    if (innerGrow > 0) {
        cr.newSubPath();
        cr.save();
        cr.translate(x, y);
        cr.scale(scale * innerGrow, scale * innerGrow);
        cr.translate(0, -originY);
        zeroPath(cr);
        cr.restore();
        cr.setFillRule(Cairo.FillRule.EVEN_ODD);
    }
    cr.fill();
    cr.setFillRule(Cairo.FillRule.WINDING);

    cr.save();
    cr.translate(width / 2, height / 2);
    cr.scale(scale, scale);
    cr.setLineWidth(LOGO_STROKE_WIDTH);
    cr.setLineCap(Cairo.LineCap.ROUND);
    cr.setLineJoin(Cairo.LineJoin.ROUND);
    if (elapsed < 0.95)
        drawTwo(cr, smooth((elapsed - 0.15) / 0.8));

    cr.translate(originX, 0);
    cr.translate(0, originY);
    cr.scale(grow, grow);
    cr.translate(0, -originY);
    cr.setLineWidth(LOGO_STROKE_WIDTH / grow);
    const inkAlpha = expand > 0.55 ? 1 - smooth((expand - 0.55) / 0.45) : 1;
    setLogoInk(cr, inkAlpha, grow, originX);
    cr.newPath();
    zeroPath(cr);
    cr.stroke();
    cr.restore();
}
