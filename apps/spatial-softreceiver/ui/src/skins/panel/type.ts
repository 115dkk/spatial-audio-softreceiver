// The display's pixel type on the device's pixel grid. Galmuri draws one
// font pixel as 1/15 em (Galmuri14) or 1/12 em (Galmuri11); a glyph is sharp
// only when that is a whole number of device pixels. At 100 % the design's
// sizes already are; at Windows' 125, 150 and 175 % scaling they are not, so
// each text role takes the face and size nearest its design that lands on
// the grid, and line heights land on whole device pixels too. The values go
// to CSS as custom properties on the skin's root; panel.css falls back to the
// design's sizes without them.

interface Face {
  /** The CSS font stack (panel.css tokens). */
  family: string;
  /** Font pixels per em. */
  pixels: number;
}

const G14: Face = { family: 'var(--pn-px14)', pixels: 15 };
const G11: Face = { family: 'var(--pn-px11)', pixels: 12 };

export interface Role {
  /** Custom property stem: --pn-fs-<name>, --pn-ff-<name>, --pn-lh-<name>. */
  name: string;
  /** The design's size at 100 %. */
  size: number;
  /** Never smaller than this. */
  min: number;
  /** Faces it may take, the design's first. Bold roles keep Galmuri11, the only face with a bold. */
  faces: Face[];
  /** Line height as a multiple of the size. */
  leading: number;
}

export const ROLES: readonly Role[] = [
  { name: 'body', size: 15, min: 14, faces: [G14, G11], leading: 1.6 },
  { name: 'head', size: 24, min: 20, faces: [G11], leading: 1.5 },
  { name: 'num', size: 48, min: 40, faces: [G11], leading: 1 },
  { name: 'mid', size: 36, min: 30, faces: [G11], leading: 1 },
  { name: 'unit', size: 12, min: 12, faces: [G11, G14], leading: 1 },
];

/** How far a fit may stray from the design before the design's size is kept unsnapped. */
const STRAY = 0.34;

export interface Fit {
  family: string;
  size: number;
  line: number;
}

/** A size whose font pixel is `k` device pixels: the nearest to the design, the larger on a tie. */
export function fit(role: Role, ratio: number): Fit {
  let best: { face: Face; size: number; score: number } | null = null;
  role.faces.forEach((face, index) => {
    for (let k = 1; k <= 24; k += 1) {
      const size = (face.pixels * k) / ratio;
      if (size < role.min - 1e-6) continue;
      // Off by how much; another face than the design's costs a little; ties go to the larger.
      const score = Math.abs(size - role.size) + index * 0.5 - size * 1e-4;
      if (best === null || score < best.score) best = { face, size, score };
    }
  });
  const chosen = best as { face: Face; size: number; score: number } | null;
  const snapped = chosen !== null && Math.abs(chosen.size - role.size) <= role.size * STRAY;
  const face = snapped ? chosen.face : role.faces[0]!;
  const size = snapped ? chosen.size : role.size;
  // A line box of whole device pixels, so baselines do not sit on half pixels.
  const line = Math.max(1, Math.round(size * role.leading * ratio)) / ratio;
  return { family: face.family, size, line };
}

const css = (value: number) => `${Number(value.toFixed(4))}px`;

/** Writes the fits for `ratio` onto `root` and marks the ratio. */
export function applyRatio(root: HTMLElement, ratio: number): void {
  const usable = Number.isFinite(ratio) && ratio > 0 ? ratio : 1;
  root.dataset.dpr = String(Number(usable.toFixed(3)));
  for (const role of ROLES) {
    const { family, size, line } = fit(role, usable);
    root.style.setProperty(`--pn-ff-${role.name}`, family);
    root.style.setProperty(`--pn-fs-${role.name}`, css(size));
    root.style.setProperty(`--pn-lh-${role.name}`, css(line));
  }
}

/**
 * Keeps `root`'s type on the device's grid: now, and whenever the ratio
 * changes (another monitor, Windows scaling, the WebView's zoom).
 */
export function followRatio(root: HTMLElement): void {
  if (typeof window === 'undefined') return;
  const watch = () => {
    const ratio = window.devicePixelRatio || 1;
    applyRatio(root, ratio);
    if (typeof window.matchMedia !== 'function') return;
    const query = window.matchMedia(`(resolution: ${ratio}dppx)`);
    query.addEventListener?.('change', watch, { once: true });
  };
  watch();
}
