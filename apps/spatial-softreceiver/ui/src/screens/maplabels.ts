// The direction maps' shared geometry (the standard object map, the panel's
// sound position map and the console's): which height a direction or an
// object is drawn at, and where each direction's name goes.
//
// A name sits beside its point on the side the map has always used: outward
// past the horizon ring, inward for a raised point unless that reaches the
// listening place, under an overhead point. A layout of 22 to 34 directions
// (ADR 0042) puts points 5° apart, so that place is often taken by another
// name or point, or would leave the drawing. Then the name tries further out
// on that side, then all round its point (nearest that side first, close by
// and further off), and when every place is taken keeps the one it overlaps
// least. A place nearer another point than its own counts against it, since
// the name would read as that point's. The maps are built before they are
// laid out, so a name's width is reckoned from its letters at the largest
// size the skin draws it; a sparse layout (7.1.4) finds every first place
// free and looks as it always has.

import type { DirectionView } from '../bindings';

/** A vector this far up (about 15°) is above the listening place. */
export const UP_Z = 0.25;
/** A vector this far down (about 15°) is below the ears (the 22.2 and 30.2 bottom ring at −30°). */
export const DOWN_Z = -0.25;

export type Height = 'flat' | 'up' | 'down';

/** The height class a rendering vector's z falls in. */
export function heightOf(z: number): Height {
  return z >= UP_Z ? 'up' : z <= DOWN_Z ? 'down' : 'flat';
}

/** A direction's height class from its elevation. */
export function directionHeight(direction: DirectionView): Height {
  return heightOf(Math.sin((direction.elevation_deg * Math.PI) / 180));
}

/** The class a mark wears for its height: none on the ear plane. */
export function heightClass(height: Height): string {
  return height === 'flat' ? '' : ` is-${height}`;
}

/** A map's drawing in its own units (the viewBox), and how big its names are there. */
export interface MapGeometry {
  /** The drawing is `size` × `size`; the listening place is its centre. */
  size: number;
  /** The horizon ring's radius. */
  radius: number;
  /** From a point to its name's centre. */
  gap: number;
  /** How far from the centre a name stays, clear of the listening place. */
  listenerClear: number;
  /** The names' font size at the largest the skin draws them, in drawing units. */
  em: number;
  /** How far from a point's centre another name keeps. */
  pointRadius: number;
}

export interface PlacedDirection {
  direction: DirectionView;
  height: Height;
  /** The point. */
  cx: number;
  cy: number;
  /** The name's centre. */
  x: number;
  y: number;
}

/** A direction's rendering vector: x right, y front, z up; azimuth 0° is ahead and grows to the left. */
function directionVector(direction: DirectionView): [x: number, y: number, z: number] {
  const azimuth = (direction.azimuth_deg * Math.PI) / 180;
  const elevation = (direction.elevation_deg * Math.PI) / 180;
  return [-Math.sin(azimuth) * Math.cos(elevation), Math.cos(azimuth) * Math.cos(elevation), Math.sin(elevation)];
}

/** A vector on the drawing: the front at the top. Longer than the horizon it sits on the ring. */
function toPlane(geometry: MapGeometry, x: number, y: number): [cx: number, cy: number] {
  const center = geometry.size / 2;
  const length = Math.hypot(x, y);
  const scale = length > 1 ? 1 / length : 1;
  return [center + x * scale * geometry.radius, center - y * scale * geometry.radius];
}

/** Width of a letter in em, near enough for the skins' sans and pixel faces. */
function advance(character: string): number {
  if ('iljftrI1.,:;!|()[]\' '.includes(character)) return 0.34;
  if ('mwMW'.includes(character)) return 0.86;
  if (/[A-Z]/.test(character)) return 0.66;
  if (/[0-9]/.test(character)) return 0.6;
  if (/[a-z]/.test(character)) return 0.56;
  // Hangul and the rest: a full em.
  return 1;
}

/** A name's width in em. */
export function nameWidth(name: string): number {
  let width = 0;
  for (const character of name) width += advance(character);
  return width;
}

interface Box {
  x0: number;
  y0: number;
  x1: number;
  y1: number;
}

/** A place costing less than this is free. */
const FREE = 1e-6;

/** Ways round a point in 15° steps, the usual side first. */
const TURNS = [0, ...Array.from({ length: 11 }, (_, index) => [index + 1, -(index + 1)]).flat(), 12];

const area = (a: Box, b: Box) =>
  Math.max(0, Math.min(a.x1, b.x1) - Math.max(a.x0, b.x0)) * Math.max(0, Math.min(a.y1, b.y1) - Math.max(a.y0, b.y0));

/** From a box to a point; 0 inside. */
function distance(box: Box, x: number, y: number): number {
  const dx = Math.max(box.x0 - x, 0, x - box.x1);
  const dy = Math.max(box.y0 - y, 0, y - box.y1);
  return Math.hypot(dx, dy);
}

/**
 * Every direction's point and name. The names are placed in the layout's
 * order, so the first channels (the front) keep their usual places.
 */
export function placeDirections(directions: readonly DirectionView[], geometry: MapGeometry): PlacedDirection[] {
  const { size, radius, gap, listenerClear, em, pointRadius } = geometry;
  const center = size / 2;
  const points = directions.map((direction) => {
    const [x, y, z] = directionVector(direction);
    const [cx, cy] = toPlane(geometry, x, y);
    const height = heightOf(z);
    const length = Math.hypot(x, y);
    // Outward past the ring; a raised point inward unless that reaches the
    // listening place; under the point when overhead.
    const inward = height === 'up' && length * radius - gap > listenerClear;
    const sign = inward ? -1 : 1;
    // On the drawing (y down).
    const [ux, uy] = length < 0.05 ? [0, 1] : [(sign * x) / length, (-sign * y) / length];
    return { direction, height, cx, cy, ux, uy };
  });

  const placed: Box[] = [];
  const pad = Math.max(1, em * 0.08);
  const step = em * 1.1;
  const listener: [number, number, number] = [center, center, 14];

  return points.map((point) => {
    const width = nameWidth(point.direction.name) * em + 2 * pad;
    const height = em + pad;
    const along = (d: number): [number, number] => [point.cx + point.ux * d, point.cy + point.uy * d];
    // The usual place and further out on the same side; then all round the
    // point, nearest the usual side first, close by and a step further.
    const candidates: [number, number][] = [along(gap), along(gap + step)];
    const preferred = Math.atan2(point.uy, point.ux);
    for (const far of [0, step / 2, step]) {
      for (const turn of TURNS) {
        const angle = preferred + (turn * Math.PI) / 12;
        const [cos, sin] = [Math.cos(angle), Math.sin(angle)];
        // Just clear of its own point, whichever way the box lies.
        const reach =
          Math.min(Math.abs(cos) < 1e-9 ? Infinity : width / 2 / Math.abs(cos), Math.abs(sin) < 1e-9 ? Infinity : height / 2 / Math.abs(sin)) +
          pointRadius +
          far;
        candidates.push([point.cx + cos * reach, point.cy + sin * reach]);
      }
    }
    let best: { at: [number, number]; box: Box; cost: number } | null = null;
    for (const at of candidates) {
      const box = { x0: at[0] - width / 2, y0: at[1] - height / 2, x1: at[0] + width / 2, y1: at[1] + height / 2 };
      let cost = 0;
      // Outside the drawing.
      const inside = area(box, { x0: 1, y0: 1, x1: size - 1, y1: size - 1 });
      cost += (width * height - inside) * 4;
      for (const other of placed) cost += area(box, other) * 2;
      // Other points; a name may touch its own, as the side names always have.
      let nearestOther = Infinity;
      for (const other of points) {
        if (other === point) continue;
        nearestOther = Math.min(nearestOther, Math.hypot(at[0] - other.cx, at[1] - other.cy));
        const d = distance(box, other.cx, other.cy);
        if (d < pointRadius) cost += (pointRadius - d) * em;
      }
      const d = distance(box, listener[0], listener[1]);
      if (d < listener[2]) cost += (listener[2] - d) * em;
      // A name nearer another point than its own reads as that point's.
      if (nearestOther < Math.hypot(at[0] - point.cx, at[1] - point.cy)) cost += em * 2;
      // Rounding leaves crumbs of area; a place this close to free is free,
      // and an earlier place wins a tie.
      if (best === null || cost < best.cost - FREE) best = { at, box, cost };
      if (cost < FREE) break;
    }
    const chosen = best as { at: [number, number]; box: Box; cost: number };
    placed.push(chosen.box);
    return { direction: point.direction, height: point.height, cx: point.cx, cy: point.cy, x: chosen.at[0], y: chosen.at[1] };
  });
}
