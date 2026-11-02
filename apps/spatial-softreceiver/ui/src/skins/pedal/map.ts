// The sound map (ADR 0046, 이펙터의 법칙 9): the listening place seen from
// above, printed on the plate with the front at the top; a push button per
// direction of the output layout that plays the test signal from it
// (`test_direction`), its short name printed beside it, a printed ring
// round the buttons above the ears and a broken one round those below; and
// a polar grid of LEDs inside the plan, none under a button or a printed
// name. While running, each sound object of the last mixed block lights the
// LED nearest where it sits, in its input's colour. The plan, the buttons
// and the LEDs are built once a frame; the 30 Hz meters only switch LEDs on
// and off.

import type { DirectionView, EngineStatus } from '../../bindings';
import { h, liveText, setAttr, svg } from '../../dom';
import { text } from '../../text';
import type { Context } from '../../screens/common';
import { type Height, type MapGeometry, heightClass, heightOf, nameWidth, placeDirections } from '../../screens/maplabels';
import { LANES } from './scope';

/**
 * viewBox units: a 300-unit square. The buttons sit on the horizon ring and,
 * above or below the ears, inside it where the plan puts them; their names
 * are printed at 12 units in the drawing's own units, so they keep their
 * places at any size. A name stands 20 units from its button's centre, so a
 * name set inward on the diagonal of a raised button clears its washer.
 */
export const MAP_GEOMETRY: MapGeometry = { size: 300, radius: 114, gap: 20, listenerClear: 28, em: 12, pointRadius: 9 };
const SIZE = MAP_GEOMETRY.size;
const CENTER = SIZE / 2;
const RADIUS = MAP_GEOMETRY.radius;
/** More directions than this take smaller buttons and names. */
export const DENSE_DIRECTIONS = 16;
/** A direction button's width in the drawing's units, and a dense layout's. */
const POINT = 17;
const DENSE_POINT = 12;
/** Two buttons this close keep at least this much metal between them, down to the smallest button. */
const POINT_GAP = 1.5;
const SMALLEST_POINT = 8;
/** The LED field: an object on the horizon lights the outer ring, just inside the buttons. */
const FIELD = 96;
/** Rings of LEDs: where each sits as a share of the field, and how many. */
const RINGS: readonly [number, number][] = [
  [0.2, 8],
  [0.36, 16],
  [0.52, 24],
  [0.68, 32],
  [0.84, 48],
  [1, 64],
];
/** A button covers the LEDs this close to its centre (its radius and an LED's). */
const COVER = 11;
/** The names' size in the drawing's units (pedal.css `.pd-map-name`), the dense size after. */
const NAME_EM = 12;
const DENSE_NAME_EM = 11;
/** How far an LED stays from a printed name's letters: a lit one's radius and its glow. */
const NAME_CLEAR = 6;

/** Each plan's mask needs its own id on the page. */
let plans = 0;

const round1 = (value: number) => (Math.round(value * 10) / 10).toString();
const percent = (units: number) => `${((units / SIZE) * 100).toFixed(2)}%`;

interface Lens {
  ring: number;
  index: number;
  x: number;
  y: number;
  element: SVGCircleElement;
}

export interface SoundMap {
  element: HTMLElement;
  update(engine: EngineStatus): void;
}

/**
 * The map for `directions`. While `running` its buttons send the test
 * signal (keys `${keyPrefix}-<id>`) and `update` lights the LEDs.
 */
export function soundMap(ctx: Context, directions: readonly DirectionView[], running: boolean, keyPrefix: string): SoundMap {
  const p = text.profile;
  if (running) ctx.offer('test_direction');
  const dense = directions.length > DENSE_DIRECTIONS;
  const placed = placeDirections(directions, MAP_GEOMETRY);
  // Directions 5° apart on the horizon (ADR 0042) stand closer than two
  // dense buttons are wide; the buttons get smaller rather than overlap.
  let closest = Infinity;
  placed.forEach((a, index) => {
    for (const b of placed.slice(index + 1)) closest = Math.min(closest, Math.hypot(a.cx - b.cx, a.cy - b.cy));
  });
  const point = Math.min(dense ? DENSE_POINT : POINT, Math.max(SMALLEST_POINT, closest - POINT_GAP));
  const reach = Math.max(6, point / 2);

  // The buttons, their printed names and the rings round the raised and lowered ones.
  const buttons: HTMLButtonElement[] = [];
  const names: SVGElement[] = [];
  const marks: SVGElement[] = [];
  for (const { direction, height, cx, cy, x, y } of placed) {
    const sending = ctx.sending?.kind === 'test_direction' && ctx.sending.value === direction.id;
    const angles = p.angles(direction.azimuth_deg, direction.elevation_deg);
    buttons.push(
      h(
        'button',
        {
          type: 'button',
          class: `pd-map-point${heightClass(height)}`,
          style: `left:${percent(cx)};top:${percent(cy)}`,
          'data-key': `${keyPrefix}-${direction.id}`,
          'data-intent': running ? 'test_direction' : null,
          'aria-label': `${direction.name}, ${angles}`,
          'aria-busy': sending ? 'true' : null,
          disabled: !running || ctx.busy,
          title: angles,
          onClick: () => ctx.send({ kind: 'test_direction', value: direction.id }),
        },
        h('span', { class: 'pd-map-cap', 'aria-hidden': 'true' }),
      ),
    );
    names.push(svg('text', { class: 'pd-map-name', x: round1(x), y: round1(y), 'text-anchor': 'middle', 'dominant-baseline': 'central' }, direction.name));
    if (height !== 'flat') marks.push(svg('circle', { class: `pd-map-mark is-${height}`, cx: round1(cx), cy: round1(cy), r: reach + 3.5 }));
  }

  // The LED grid, leaving out the LEDs a button covers and those a name is printed over.
  const em = dense ? DENSE_NAME_EM : NAME_EM;
  const nameBox = (direction: DirectionView, x: number, y: number, clear: number) => {
    const half = (nameWidth(direction.name) * em) / 2 + clear;
    const tall = em * 0.45 + clear;
    return { x0: x - half, x1: x + half, y0: y - tall, y1: y + tall };
  };
  const printed = placed.map(({ direction, x, y }) => nameBox(direction, x, y, NAME_CLEAR));
  const lenses: Lens[][] = RINGS.map(([share, count], ring) => {
    const list: Lens[] = [];
    for (let index = 0; index < count; index += 1) {
      const turn = (2 * Math.PI * index) / count;
      const x = CENTER + Math.sin(turn) * share * FIELD;
      const y = CENTER - Math.cos(turn) * share * FIELD;
      if (placed.some((point) => Math.hypot(point.cx - x, point.cy - y) < COVER)) continue;
      if (printed.some((box) => x > box.x0 && x < box.x1 && y > box.y0 && y < box.y1)) continue;
      list.push({ ring, index, x, y, element: svg('circle', { class: 'pd-mled', cx: round1(x), cy: round1(y), r: 2.3 }) });
    }
    return list;
  });
  const every = lenses.flat();

  const directionsBelow = placed.some((mark) => mark.height === 'down');
  let objectsBelow = false;
  const below = h('li', null, h('span', { class: 'pd-swatch is-down', 'aria-hidden': 'true' }), p.legendDown);
  below.hidden = !directionsBelow;

  // The plan's lines break where a name is printed across them, as a screen printer would set them.
  plans += 1;
  const cut = `pd-map-cut-${plans}`;
  const gaps = placed.map(({ direction, x, y }) => {
    const box = nameBox(direction, x, y, 1.5);
    return svg('rect', { x: round1(box.x0), y: round1(box.y0), width: round1(box.x1 - box.x0), height: round1(box.y1 - box.y0), fill: '#000' });
  });
  const plan = svg(
    'svg',
    { class: 'pd-map-plan', viewBox: `0 0 ${SIZE} ${SIZE}`, 'aria-hidden': 'true', focusable: 'false' },
    svg(
      'defs',
      null,
      svg(
        'mask',
        { id: cut, maskUnits: 'userSpaceOnUse', x: 0, y: 0, width: SIZE, height: SIZE },
        svg('rect', { x: 0, y: 0, width: SIZE, height: SIZE, fill: '#fff' }),
        ...gaps,
      ),
    ),
    svg(
      'g',
      { mask: `url(#${cut})` },
      svg('circle', { class: 'pd-print-line', cx: CENTER, cy: CENTER, r: RADIUS }),
      svg('circle', { class: 'pd-print-line is-quiet', cx: CENTER, cy: CENTER, r: round1(FIELD * 0.6) }),
      svg('line', { class: 'pd-print-line is-quiet', x1: CENTER, y1: CENTER - RADIUS, x2: CENTER, y2: CENTER + RADIUS }),
      svg('line', { class: 'pd-print-line is-quiet', x1: CENTER - RADIUS, y1: CENTER, x2: CENTER + RADIUS, y2: CENTER }),
      svg('circle', { class: 'pd-print-line pd-map-head', cx: CENTER, cy: CENTER, r: 10 }),
      svg('path', { class: 'pd-print-line', d: `M${CENTER - 4} ${CENTER - 9.4} L${CENTER} ${CENTER - 15} L${CENTER + 4} ${CENTER - 9.4}` }),
      ...marks,
    ),
    svg('g', { class: 'pd-mleds' }, ...every.map((lens) => lens.element)),
    svg('g', null, ...names),
  );

  const status = liveText(running ? p.mapObjectsNone : p.mapOff);
  const element = h(
    'div',
    { class: `pd-map${dense ? ' is-dense' : ''}`, style: `--pd-point:${round1(point)}` },
    h('div', { class: 'pd-map-frame', role: 'group', 'aria-label': p.mapLabel }, plan, h('div', { class: 'pd-map-points' }, ...buttons)),
    h('p', { class: 'pd-fine pd-map-status', 'aria-live': 'off' }, status.node),
    h(
      'ul',
      { class: 'pd-map-legend', 'aria-label': p.legendLabel },
      h('li', null, h('span', { class: 'pd-swatch is-flat', 'aria-hidden': 'true' }), p.legendFlat),
      h('li', null, h('span', { class: 'pd-swatch is-up', 'aria-hidden': 'true' }), p.legendUp),
      below,
    ),
  );

  /**
   * The free LED nearest a point of the plan: its ring by distance, then by
   * turn; when a button covers that one or a louder sound has it, the
   * nearest free one, so every sound lights an LED of its own.
   */
  const nearest = (x: number, y: number, taken: { has(lens: Lens): boolean }): Lens | null => {
    const share = Math.hypot(x - CENTER, y - CENTER) / FIELD;
    let ring = 0;
    RINGS.forEach(([at], index) => {
      if (Math.abs(at - share) < Math.abs((RINGS[ring]?.[0] ?? 0) - share)) ring = index;
    });
    const count = RINGS[ring]?.[1] ?? 1;
    const turn = Math.atan2(x - CENTER, CENTER - y);
    const index = (Math.round((turn / (2 * Math.PI)) * count) + count) % count;
    const found = lenses[ring]?.find((lens) => lens.index === index);
    if (found !== undefined && !taken.has(found)) return found;
    let best: Lens | null = null;
    let distance = Infinity;
    for (const lens of every) {
      if (taken.has(lens)) continue;
      const d = Math.hypot(lens.x - x, lens.y - y);
      if (d < distance) {
        distance = d;
        best = lens;
      }
    }
    return best;
  };

  const lit = new Map<Lens, { lane: number; level: number; height: Height }>();
  const shown = new Map<Lens, string>();
  return {
    element,
    update(engine) {
      if (!running) return;
      lit.clear();
      // The loudest sounds take their places first.
      const scene = [...engine.scene].sort((a, b) => b.level_dbfs - a.level_dbfs);
      for (const object of scene) {
        const length = Math.hypot(object.x, object.y);
        const scale = length > 1 ? 1 / length : 1;
        const lens = nearest(CENTER + object.x * scale * FIELD, CENTER - object.y * scale * FIELD, lit);
        if (lens === null) continue;
        const height = heightOf(object.z);
        if (height === 'down') objectsBelow = true;
        lit.set(lens, { lane: object.lane, level: object.level_dbfs, height });
      }
      for (const lens of every) {
        const on = lit.get(lens);
        const name = on === undefined ? 'pd-mled' : `pd-mled is-lit ${on.lane < LANES ? `lane-${on.lane}` : 'lane-x'} is-${on.height}`;
        if (shown.get(lens) !== name) {
          shown.set(lens, name);
          setAttr(lens.element, 'class', name);
        }
      }
      const hidden = !(directionsBelow || objectsBelow);
      if (below.hidden !== hidden) below.hidden = hidden;
      const count = engine.scene.length;
      status.set(count === 0 ? p.mapObjectsNone : count === 1 ? p.mapObjectsOne : p.mapObjectsMany(count));
    },
  };
}
