"""Cut a logo off a flat background with a smooth, anti-aliased silhouette.

1. Foreground = everything not flood-connected to background-colored pixels
   (threshold covers the navy drop shadow, not the black outlines).
2. Clean the mask: drop specks, fill pinholes, close small nicks.
3. Upscale the mask to the output size and smooth the contour there, so the
   edge is a clean curve rather than torn source pixels.
4. Colors are filled outward from the art before resizing, so no background
   bleeds into the anti-aliased rim.

Usage: cutout2.py in.png out.png [size] [threshold]
"""
import sys
import numpy as np
from PIL import Image
from scipy import ndimage as ndi

src, dst = sys.argv[1], sys.argv[2]
size = int(sys.argv[3]) if len(sys.argv) > 3 else 1024
THRESH = float(sys.argv[4]) if len(sys.argv) > 4 else 14.0

im = np.asarray(Image.open(src).convert("RGB")).astype(np.float64)
h, w, _ = im.shape
border = np.concatenate([im[0], im[-1], im[:, 0], im[:, -1]])
bg = np.median(border, 0)
d = np.sqrt(((im - bg) ** 2).sum(-1))

near = d < THRESH
labels, n = ndi.label(near)
touch = set(np.unique(np.concatenate([labels[0], labels[-1], labels[:, 0], labels[:, -1]]))) - {0}
idx = np.arange(1, n + 1)
areas = ndi.sum(np.ones_like(d), labels, idx)
means = ndi.mean(d, labels, idx)
is_bg = np.zeros(n + 1, bool)
for i in idx:
    # Border-connected, or an enclosed pocket (letter counter, gap) that is
    # clearly background colored rather than a dark detail of the art.
    is_bg[i] = i in touch or (areas[i - 1] >= 20 and means[i - 1] < THRESH * 0.45)
fg = ~is_bg[labels]

disk = lambda r: (np.add.outer(np.arange(-r, r + 1) ** 2, np.arange(-r, r + 1) ** 2) <= r * r)
fg = ndi.binary_closing(fg, structure=disk(2), iterations=1)
lab, m = ndi.label(fg)
if m:
    sizes = ndi.sum(np.ones_like(d), lab, np.arange(1, m + 1))
    fg = np.isin(lab, np.arange(1, m + 1)[sizes >= 60])
holes = ndi.binary_fill_holes(fg) & ~fg
hl, hn = ndi.label(holes)
if hn:
    hs = ndi.sum(np.ones_like(d), hl, np.arange(1, hn + 1))
    fg |= np.isin(hl, np.arange(1, hn + 1)[hs < 20])
fg = ndi.binary_opening(fg, structure=disk(1), iterations=1)

# Tight crop.
ys, xs = np.where(fg)
pad = 2
y0, y1 = max(ys.min() - pad, 0), min(ys.max() + 1 + pad, h)
x0, x1 = max(xs.min() - pad, 0), min(xs.max() + 1 + pad, w)
fg, im_c = fg[y0:y1, x0:x1], im[y0:y1, x0:x1]
ch, cw = fg.shape

# Interior ("core") pixels are pure art; the 1px rim around them is
# anti-aliased against the navy. Thin features that would vanish under the
# erosion stay core so they keep their own color.
core = ndi.binary_erosion(fg, iterations=1)
cl, cn = ndi.label(fg)
if cn:
    kept = ndi.maximum(core, cl, np.arange(1, cn + 1))
    core |= np.isin(cl, np.arange(1, cn + 1)[kept == 0])
_, (iy, ix) = ndi.distance_transform_edt(~core, return_indices=True)
F = im_c[iy, ix]
# Rim coverage: how far each pixel sits from the bg toward its art color.
fb = F - bg
den = (fb ** 2).sum(-1)
proj = ((im_c - bg) * fb).sum(-1) / np.maximum(den, 1e-6)
cover = np.where(den > 400, np.clip(proj, 0, 1), 1.0)
A = np.where(core, 1.0, np.where(fg, cover, 0.0))
# Colors come only from the interior, so no navy reaches the edge.
filled = F
margin = round(size * 0.03)
scale = min((size - 2 * margin) / cw, (size - 2 * margin) / ch)
nw, nh = round(cw * scale), round(ch * scale)

rgb = np.dstack([
    np.asarray(Image.fromarray(filled[..., i].astype(np.float32), "F").resize((nw, nh), Image.LANCZOS))
    for i in range(3)
])

# Smooth silhouette: supersample the mask, blur, threshold, then downsample
# for a crisp anti-aliased edge.
SS = 4
mask_hi = np.asarray(Image.fromarray(A.astype(np.float32), "F").resize((nw * SS, nh * SS), Image.BILINEAR))
mask_hi = ndi.gaussian_filter(mask_hi, sigma=scale * SS * float(sys.argv[5] if len(sys.argv) > 5 else 0.8))
mask_hi = (mask_hi >= 0.5).astype(np.float32)
alpha = np.asarray(Image.fromarray(mask_hi, "F").resize((nw, nh), Image.BOX))

out = np.zeros((size, size, 4), np.uint8)
ox, oy = (size - nw) // 2, (size - nh) // 2
out[oy:oy + nh, ox:ox + nw, :3] = np.clip(rgb, 0, 255).round().astype(np.uint8)
out[oy:oy + nh, ox:ox + nw, 3] = np.clip(alpha * 255, 0, 255).round().astype(np.uint8)
out[out[..., 3] == 0, :3] = 0
Image.fromarray(out, "RGBA").save(dst, optimize=True)
print(f"bg={bg} art={cw}x{ch} -> {nw}x{nh} on {size}x{size}")
