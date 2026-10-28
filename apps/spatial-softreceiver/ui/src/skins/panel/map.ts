// The sound position map on the display: the listening place from above,
// the output layout's directions around it and, while running, the sound
// objects of the last mixed block in their inputs' colours. A direction is a
// point to press (and to reach with Tab) that plays the test signal from it
// (`test_direction`). The drawing is built once a frame; the 30 Hz meters
// only move a fixed pool of circles.

import type { DirectionView, EngineStatus, SceneObject } from '../../bindings';
import { h, liveText, setAttr, svg } from '../../dom';
import { text } from '../../text';
import type { Context } from '../../screens/common';
import { type MapGeometry, heightClass, heightOf, placeDirections } from '../../screens/maplabels';
import { MAP_POOL } from '../../screens/objectmap';
import { LANES } from './scope';
import { dot } from './ui';

/**
 * viewBox units: a square, the horizon ring inside it. The frame is 260 to
 * 300 px wide (the status page's map column, the profile's side), and the
 * names are at most 16 px (the display's type at 150 %, type.ts), so they
 * are placed for 18.5 units.
 */
export const MAP_GEOMETRY: MapGeometry = { size: 300, radius: 104, gap: 20, listenerClear: 28, em: 18.5, pointRadius: 8 };
const SIZE = MAP_GEOMETRY.size;
const CENTER = SIZE / 2;
const RADIUS = MAP_GEOMETRY.radius;
/** Levels at or under the floor draw small and faint; 0 dBFS draws largest. */
const FLOOR_DB = -60;
const OBJECT_R_MIN = 3;
const OBJECT_R_MAX = 10;
const OPACITY_MIN = 0.4;
/** More directions than this take the display's smaller type for their names. */
export const DENSE_DIRECTIONS = 16;

const round1 = (value: number) => (Math.round(value * 10) / 10).toString();
const percent = (units: number) => `${((units / SIZE) * 100).toFixed(2)}%`;

function toPlane(x: number, y: number): [number, number] {
  const length = Math.hypot(x, y);
  const scale = length > 1 ? 1 / length : 1;
  return [CENTER + x * scale * RADIUS, CENTER - y * scale * RADIUS];
}

function same(a: readonly DirectionView[], b: readonly DirectionView[]): boolean {
  if (a === b) return true;
  return (
    a.length === b.length &&
    a.every((direction, index) => {
      const other = b[index];
      return (
        other !== undefined &&
        direction.id === other.id &&
        direction.name === other.name &&
        direction.azimuth_deg === other.azimuth_deg &&
        direction.elevation_deg === other.elevation_deg
      );
    })
  );
}

export interface SoundMap {
  element: HTMLElement;
  update(engine: EngineStatus): void;
}

/**
 * The map for `directions`. While `running` its points send the test signal
 * (keys `${keyPrefix}-<id>`) and `update` places the scene's objects.
 */
export function soundMap(ctx: Context, directions: readonly DirectionView[], running: boolean, keyPrefix: string): SoundMap {
  const p = text.profile;
  if (running) ctx.offer('test_direction');
  const points = h('div', { class: 'pn-map-points' });
  let drawn: readonly DirectionView[] = [];
  // Below-ear directions drawn, and whether an object has been below the
  // ears since the map was built (then its legend line stays).
  let directionsBelow = false;
  let objectsBelow = false;
  const below = h('li', null, h('span', { class: 'pn-swatch is-down', 'aria-hidden': 'true' }), p.legendDown);
  below.hidden = true;
  const showBelow = () => {
    const hidden = !(directionsBelow || objectsBelow);
    if (below.hidden !== hidden) below.hidden = hidden;
  };

  const drawDirections = (next: readonly DirectionView[]) => {
    if (same(next, drawn)) return;
    drawn = next;
    const marks: HTMLElement[] = [];
    const placed = placeDirections(next, MAP_GEOMETRY);
    for (const { direction, height, cx, cy, x, y } of placed) {
      const where = `left:${percent(cx)};top:${percent(cy)}`;
      const sending = ctx.sending?.kind === 'test_direction' && ctx.sending.value === direction.id;
      const angles = p.angles(direction.azimuth_deg, direction.elevation_deg);
      const kind = heightClass(height);
      const point = running
        ? h('button', {
            type: 'button',
            class: `pn-map-point${kind}`,
            style: where,
            'data-key': `${keyPrefix}-${direction.id}`,
            'data-intent': 'test_direction',
            'aria-label': `${direction.name}, ${angles}`,
            'aria-busy': sending ? 'true' : null,
            disabled: ctx.busy,
            title: angles,
            onClick: () => ctx.send({ kind: 'test_direction', value: direction.id }),
          })
        : h('span', { class: `pn-map-point${kind}`, style: where, title: `${direction.name} · ${angles}` });
      marks.push(
        point,
        h('span', { class: `pn-map-label${kind}`, style: `left:${percent(x)};top:${percent(y)}`, 'aria-hidden': 'true' }, direction.name),
      );
    }
    points.replaceChildren(...marks);
    element.classList.toggle('is-dense', next.length > DENSE_DIRECTIONS);
    directionsBelow = placed.some((mark) => mark.height === 'down');
    showBelow();
  };

  const circles = Array.from({ length: MAP_POOL }, () =>
    // pathLength: a below-ear object's dotted ring breaks into eight at any size.
    svg('circle', { class: 'pn-map-object', cx: CENTER, cy: CENTER, r: OBJECT_R_MIN, pathLength: 32, display: 'none' }),
  );
  let shown = 0;
  const drawing = svg(
    'svg',
    { class: 'pn-map-drawing', viewBox: `0 0 ${SIZE} ${SIZE}`, 'aria-hidden': 'true', focusable: 'false' },
    svg('circle', { class: 'pn-map-ring', cx: CENTER, cy: CENTER, r: RADIUS }),
    svg('circle', { class: 'pn-map-ring is-inner', cx: CENTER, cy: CENTER, r: round1(RADIUS * Math.SQRT1_2) }),
    svg('line', { class: 'pn-map-axis', x1: CENTER, y1: CENTER - RADIUS, x2: CENTER, y2: CENTER + RADIUS }),
    svg('line', { class: 'pn-map-axis', x1: CENTER - RADIUS, y1: CENTER, x2: CENTER + RADIUS, y2: CENTER }),
    svg(
      'g',
      { class: 'pn-map-listener' },
      svg('path', { d: `M ${CENTER - 5} ${CENTER - 5} L ${CENTER} ${CENTER - 17} L ${CENTER + 5} ${CENTER - 5} Z` }),
      svg('circle', { cx: CENTER, cy: CENTER, r: 8 }),
    ),
    svg('g', null, ...circles),
  );

  const status = liveText(running ? p.mapObjectsNone : p.mapOff);
  const legend = h(
    'ul',
    { class: 'pn-map-legend', 'aria-label': p.legendLabel },
    ...text.panel.lamps.slice(0, LANES).map((name, lane) => h('li', null, dot(`lane-${lane}`), name)),
    h('li', null, h('span', { class: 'pn-swatch is-flat', 'aria-hidden': 'true' }), p.legendFlat),
    h('li', null, h('span', { class: 'pn-swatch is-up', 'aria-hidden': 'true' }), p.legendUp),
    below,
  );
  const element = h(
    'div',
    { class: 'pn-map' },
    h('div', { class: 'pn-map-frame', role: 'group', 'aria-label': p.mapLabel }, drawing, points),
    h('p', { class: 'pn-map-status', 'aria-live': 'off' }, status.node),
    legend,
  );
  drawDirections(directions);

  function place(scene: readonly SceneObject[]) {
    const count = Math.min(scene.length, MAP_POOL);
    for (let slot = 0; slot < count; slot += 1) {
      const object = scene[slot] as SceneObject;
      const circle = circles[slot] as SVGCircleElement;
      const [cx, cy] = toPlane(object.x, object.y);
      const strength = Math.min(1, Math.max(0, (object.level_dbfs - FLOOR_DB) / -FLOOR_DB));
      setAttr(circle, 'cx', round1(cx));
      setAttr(circle, 'cy', round1(cy));
      setAttr(circle, 'r', round1(OBJECT_R_MIN + strength * (OBJECT_R_MAX - OBJECT_R_MIN)));
      setAttr(circle, 'opacity', (OPACITY_MIN + strength * (1 - OPACITY_MIN)).toFixed(2));
      const lane = object.lane < LANES ? `lane-${object.lane}` : 'lane-x';
      const height = heightOf(object.z);
      if (height === 'down') objectsBelow = true;
      setAttr(circle, 'class', `pn-map-object ${lane} is-${height}`);
      if (slot >= shown) circle.removeAttribute('display');
    }
    showBelow();
    for (let slot = count; slot < shown; slot += 1) circles[slot]?.setAttribute('display', 'none');
    shown = count;
    status.set(count === 0 ? p.mapObjectsNone : count === 1 ? p.mapObjectsOne : p.mapObjectsMany(count));
  }

  return {
    element,
    update(engine) {
      if (!running) return;
      drawDirections(engine.directions);
      place(engine.scene);
    },
  };
}
