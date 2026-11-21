// Real mouse events for capture.sh, posted through CoreGraphics so the
// editor's canvas sees genuine pointer drags (System Events can only click).
//
// Usage (screen points, origin top left of the main display):
//   osascript -l JavaScript mouse.js click X Y
//   osascript -l JavaScript mouse.js drag X1 Y1 X2 Y2 [steps] [seconds]
//   osascript -l JavaScript mouse.js path SECONDS X1 Y1 X2 Y2 X3 Y3 ...
//   osascript -l JavaScript mouse.js move X Y
ObjC.import("CoreGraphics");

const DOWN = 1, UP = 2, MOVED = 5, DRAGGED = 6, LEFT = 0, HID_TAP = 0;

function post(type, x, y) {
  const event = $.CGEventCreateMouseEvent(null, type, $.CGPointMake(x, y), LEFT);
  $.CGEventPost(HID_TAP, event);
}

/* Glide through points with the button held, easing between each pair. */
function stroke(points, seconds) {
  const steps = Math.max(8, Math.round(seconds * 60));
  const pause = seconds / steps;
  post(MOVED, points[0][0], points[0][1]);
  delay(0.05);
  post(DOWN, points[0][0], points[0][1]);
  delay(0.05);
  const legs = points.length - 1;
  for (let i = 1; i <= steps; i++) {
    const t = (i / steps) * legs;
    const leg = Math.min(legs - 1, Math.floor(t));
    const f = t - leg;
    const e = f < 0.5 ? 2 * f * f : 1 - Math.pow(-2 * f + 2, 2) / 2;
    const [ax, ay] = points[leg], [bx, by] = points[leg + 1];
    post(DRAGGED, ax + (bx - ax) * e, ay + (by - ay) * e);
    delay(pause);
  }
  const [lx, ly] = points[points.length - 1];
  post(UP, lx, ly);
}

function run(argv) {
  const [cmd, ...rest] = argv;
  const n = rest.map(Number);
  if (cmd === "move") {
    post(MOVED, n[0], n[1]);
  } else if (cmd === "click") {
    post(MOVED, n[0], n[1]);
    delay(0.05);
    post(DOWN, n[0], n[1]);
    delay(0.05);
    post(UP, n[0], n[1]);
  } else if (cmd === "drag") {
    stroke([[n[0], n[1]], [n[2], n[3]]], n[5] || 0.5);
  } else if (cmd === "path") {
    const pts = [];
    for (let i = 1; i + 1 < n.length; i += 2) pts.push([n[i], n[i + 1]]);
    stroke(pts, n[0]);
  } else {
    throw new Error("unknown command: " + cmd);
  }
}
