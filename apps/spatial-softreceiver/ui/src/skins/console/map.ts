// The sound positions seen from above: the layout's directions around the
// listening place and, while running, the scene's objects (EngineStatus.scene)
// as their entrance's mark, so the lane reads by shape as well as colour
// (law 4). The drawing is built once a frame; the 30 Hz meters only move a
// fixed pool of marks, so a tick never builds nodes.

import type { DirectionView, EngineStatus, SceneObject } from '../../bindings';
import { h, liveText, setAttr, svg } from '../../dom';
import { text } from '../../text';
import { LANE_COUNT, entrance, laneMark, shapeOf } from './kit';

/** Every lane the scene can name times the objects one carries (`ssr_types::MAX_OBJECTS`). */
const OBJECTS_PER_LANE = 64;
const POOL = LANE_COUNT * OBJECTS_PER_LANE;

const SIZE = 300;
const CENTER = SIZE / 2;
const RADIUS = 112;
const LABEL_GAP = 18;
const LISTENER_CLEAR = 28;
const FLOOR_DB = -60;
const MARK_MIN = 3.5;
const MARK_MAX = 11;
const OPACITY_MIN = 0.4;
/** About 15° up counts as above the listening place. */
const UP_Z = 0.25;

const round1 = (value: number) => (Math.round(value * 10) / 10).toString();

function toPlane(x: number, y: number): [number, number] {
  const length = Math.hypot(x, y);
  const scale = length > 1 ? 1 / length : 1;
  return [CENTER + x * scale * RADIUS, CENTER - y * scale * RADIUS];
}

/** Azimuth 0° is ahead and grows to the left. */
function vector(direction: DirectionView): [number, number, number] {
  const azimuth = (direction.azimuth_deg * Math.PI) / 180;
  const elevation = (direction.elevation_deg * Math.PI) / 180;
  return [-Math.sin(azimuth) * Math.cos(elevation), Math.cos(azimuth) * Math.cos(elevation), Math.sin(elevation)];
}

function same(a: readonly DirectionView[], b: readonly DirectionView[]): boolean {
  if (a === b) return true;
  return (
    a.length === b.length &&
    a.every((d, i) => {
      const o = b[i];
      return o !== undefined && d.id === o.id && d.name === o.name && d.azimuth_deg === o.azimuth_deg && d.elevation_deg === o.elevation_deg;
    })
  );
}

function directionMark(direction: DirectionView): { point: SVGElement; name: SVGElement } {
  const [x, y, z] = vector(direction);
  const [cx, cy] = toPlane(x, y);
  const up = z >= UP_Z;
  const length = Math.hypot(x, y);
  const inward = up && length * RADIUS - LABEL_GAP > LISTENER_CLEAR;
  const sign = inward ? -1 : 1;
  const [ux, uy] = length < 0.05 ? [0, -1] : [(sign * x) / length, (sign * y) / length];
  return {
    point: svg(
      'g',
      { class: `cn-map-direction${up ? ' is-up' : ''}` },
      svg('title', null, `${direction.name} · ${text.profile.angles(direction.azimuth_deg, direction.elevation_deg)}`),
      svg('circle', { cx: round1(cx), cy: round1(cy), r: 3 }),
    ),
    name: svg(
      'text',
      {
        class: `cn-map-name${up ? ' is-up' : ''}`,
        x: round1(cx + ux * LABEL_GAP),
        y: round1(cy - uy * LABEL_GAP),
        'text-anchor': 'middle',
        'dominant-baseline': 'central',
        'aria-hidden': 'true',
      },
      direction.name,
    ),
  };
}

export interface SceneMap {
  element: HTMLElement;
  update(engine: EngineStatus): void;
}

/**
 * The map's drawing, legend and status line. `lanes` are the entrances the
 * legend names (the ASIO input only while it is on).
 */
export function sceneMap(directions: readonly DirectionView[], running: boolean, lanes: readonly number[]): SceneMap {
  const p = text.profile;
  const points = svg('g', { class: 'cn-map-directions' });
  const names = svg('g', { class: 'cn-map-names' });
  let drawn: readonly DirectionView[] = [];
  const drawDirections = (next: readonly DirectionView[]) => {
    if (same(next, drawn)) return;
    drawn = next;
    const marks = next.map(directionMark);
    points.replaceChildren(...marks.map((mark) => mark.point));
    names.replaceChildren(...marks.map((mark) => mark.name));
  };
  drawDirections(directions);

  const pool = Array.from({ length: POOL }, () =>
    svg('path', { class: 'cn-map-object', d: shapeOf(0), transform: `translate(${CENTER} ${CENTER})`, display: 'none' }),
  );
  let shown = 0;

  const drawing = svg(
    'svg',
    { class: 'cn-map-drawing', viewBox: `0 0 ${SIZE} ${SIZE}`, role: 'img', 'aria-label': p.mapLabel },
    svg('circle', { class: 'cn-map-disc', cx: CENTER, cy: CENTER, r: RADIUS }),
    svg('circle', { class: 'cn-map-ring', cx: CENTER, cy: CENTER, r: round1(RADIUS * Math.SQRT1_2) }),
    svg('line', { class: 'cn-map-axis', x1: CENTER, y1: CENTER - RADIUS, x2: CENTER, y2: CENTER + RADIUS }),
    svg('line', { class: 'cn-map-axis', x1: CENTER - RADIUS, y1: CENTER, x2: CENTER + RADIUS, y2: CENTER }),
    points,
    names,
    svg(
      'g',
      { class: 'cn-map-listener', 'aria-hidden': 'true' },
      svg('path', { d: `M ${CENTER - 6} ${CENTER - 5} L ${CENTER} ${CENTER - 18} L ${CENTER + 6} ${CENTER - 5} Z` }),
      svg('circle', { cx: CENTER, cy: CENTER, r: 8 }),
    ),
    svg('g', { class: 'cn-map-objects' }, ...pool),
  );

  const status = liveText(running ? p.mapObjectsNone : p.mapOff);
  const legendItem = (mark: Element, label: string) => h('li', null, mark, h('span', null, label));
  const element = h(
    'div',
    { class: 'cn-map' },
    h('div', { class: 'cn-map-frame' }, drawing),
    h('p', { class: 'cn-map-status' }, status.node),
    h(
      'div',
      { class: 'cn-map-legend', role: 'group', 'aria-label': p.legendLabel },
      h('ul', null, ...lanes.map((lane) => legendItem(laneMark(lane), entrance(lane).name))),
      h(
        'ul',
        null,
        legendItem(h('span', { class: 'cn-map-key is-flat', 'aria-hidden': 'true' }), p.legendFlat),
        legendItem(h('span', { class: 'cn-map-key is-up', 'aria-hidden': 'true' }), p.legendUp),
      ),
    ),
    h('p', { class: 'cn-hint' }, p.mapBody),
  );

  function place(scene: readonly SceneObject[]) {
    const count = Math.min(scene.length, POOL);
    for (let slot = 0; slot < count; slot += 1) {
      const object = scene[slot] as SceneObject;
      const mark = pool[slot] as SVGPathElement;
      const [cx, cy] = toPlane(object.x, object.y);
      const strength = Math.min(1, Math.max(0, (object.level_dbfs - FLOOR_DB) / -FLOOR_DB));
      const size = MARK_MIN + strength * (MARK_MAX - MARK_MIN);
      const lane = object.lane < LANE_COUNT ? object.lane : 0;
      setAttr(mark, 'd', shapeOf(lane));
      setAttr(mark, 'transform', `translate(${round1(cx)} ${round1(cy)}) scale(${round1(size)})`);
      setAttr(mark, 'opacity', (OPACITY_MIN + strength * (1 - OPACITY_MIN)).toFixed(2));
      setAttr(mark, 'class', `cn-map-object cn-lane-${lane} ${object.z >= UP_Z ? 'is-up' : 'is-flat'}`);
      if (slot >= shown) mark.removeAttribute('display');
    }
    for (let slot = count; slot < shown; slot += 1) pool[slot]?.setAttribute('display', 'none');
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
