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
import { MAP_POOL } from '../../screens/objectmap';
import { LANES } from './scope';
import { dot } from './ui';

/** viewBox units: a square, the horizon ring inside it. */
const SIZE = 300;
const CENTER = SIZE / 2;
const RADIUS = 104;
const LABEL_GAP = 20;
const LISTENER_CLEAR = 28;
/** Levels at or under the floor draw small and faint; 0 dBFS draws largest. */
const FLOOR_DB = -60;
const OBJECT_R_MIN = 3;
const OBJECT_R_MAX = 10;
const OPACITY_MIN = 0.4;
/** About 15° up counts as above the listening place. */
const UP_Z = 0.25;

const round1 = (value: number) => (Math.round(value * 10) / 10).toString();
const percent = (units: number) => `${((units / SIZE) * 100).toFixed(2)}%`;

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

  const drawDirections = (next: readonly DirectionView[]) => {
    if (same(next, drawn)) return;
    drawn = next;
    const marks: HTMLElement[] = [];
    for (const direction of next) {
      const [x, y, z] = vector(direction);
      const [cx, cy] = toPlane(x, y);
      const up = z >= UP_Z;
      const length = Math.hypot(x, y);
      // A name sits outward from its point; a raised point's name inward,
      // unless that would reach the listening place; under it when overhead.
      const inward = up && length * RADIUS - LABEL_GAP > LISTENER_CLEAR;
      const sign = inward ? -1 : 1;
      const [ux, uy] = length < 0.05 ? [0, -1] : [(sign * x) / length, (sign * y) / length];
      const where = `left:${percent(cx)};top:${percent(cy)}`;
      const sending = ctx.sending?.kind === 'test_direction' && ctx.sending.value === direction.id;
      const angles = p.angles(direction.azimuth_deg, direction.elevation_deg);
      const point = running
        ? h('button', {
            type: 'button',
            class: `pn-map-point${up ? ' is-up' : ''}`,
            style: where,
            'data-key': `${keyPrefix}-${direction.id}`,
            'data-intent': 'test_direction',
            'aria-label': `${direction.name}, ${angles}`,
            'aria-busy': sending ? 'true' : null,
            disabled: ctx.busy,
            title: angles,
            onClick: () => ctx.send({ kind: 'test_direction', value: direction.id }),
          })
        : h('span', { class: `pn-map-point${up ? ' is-up' : ''}`, style: where, title: `${direction.name} · ${angles}` });
      marks.push(
        point,
        h(
          'span',
          {
            class: `pn-map-label${up ? ' is-up' : ''}`,
            style: `left:${percent(cx + ux * LABEL_GAP)};top:${percent(cy - uy * LABEL_GAP)}`,
            'aria-hidden': 'true',
          },
          direction.name,
        ),
      );
    }
    points.replaceChildren(...marks);
  };
  drawDirections(directions);

  const circles = Array.from({ length: MAP_POOL }, () =>
    svg('circle', { class: 'pn-map-object', cx: CENTER, cy: CENTER, r: OBJECT_R_MIN, display: 'none' }),
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
  );
  const element = h(
    'div',
    { class: 'pn-map' },
    h('div', { class: 'pn-map-frame', role: 'group', 'aria-label': p.mapLabel }, drawing, points),
    h('p', { class: 'pn-map-status', 'aria-live': 'off' }, status.node),
    legend,
  );

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
      setAttr(circle, 'class', `pn-map-object ${lane} ${object.z >= UP_Z ? 'is-up' : 'is-flat'}`);
      if (slot >= shown) circle.removeAttribute('display');
    }
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
