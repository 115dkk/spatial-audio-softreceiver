// Where the direction maps put their names (src/screens/maplabels.ts): a
// sparse layout keeps every name at its usual place, and a dense one (22.2,
// 24.1.10, 30.2, ADR 0042) keeps its names apart, clear of the other points
// and inside the drawing, in every skin's geometry.

import { describe, expect, it } from 'vitest';
import type { DirectionView, SpeakerLayout } from '../src/bindings';
import { fixture } from '../src/fixtures';
import { speakerDirections } from '../src/mock';
import { type MapGeometry, directionHeight, heightOf, nameWidth, placeDirections } from '../src/screens/maplabels';
import { MAP_GEOMETRY as STANDARD } from '../src/screens/objectmap';
import { MAP_GEOMETRY as PANEL } from '../src/skins/panel/map';
import { MAP_GEOMETRY as CONSOLE } from '../src/skins/console/map';

const GEOMETRIES: [string, MapGeometry][] = [
  ['standard', STANDARD],
  ['panel', PANEL],
  ['console', CONSOLE],
];

/** Where the maps put a name before ADR 0042: beside its point, outward or (raised) inward. */
function usualPlace(direction: DirectionView, geometry: MapGeometry): [number, number] {
  const azimuth = (direction.azimuth_deg * Math.PI) / 180;
  const elevation = (direction.elevation_deg * Math.PI) / 180;
  const x = -Math.sin(azimuth) * Math.cos(elevation);
  const y = Math.cos(azimuth) * Math.cos(elevation);
  const z = Math.sin(elevation);
  const center = geometry.size / 2;
  const [cx, cy] = [center + x * geometry.radius, center - y * geometry.radius];
  const length = Math.hypot(x, y);
  const inward = z >= 0.25 && length * geometry.radius - geometry.gap > geometry.listenerClear;
  const sign = inward ? -1 : 1;
  const [ux, uy] = length < 0.05 ? [0, -1] : [(sign * x) / length, (sign * y) / length];
  return [cx + ux * geometry.gap, cy - uy * geometry.gap];
}

const boxOf = (name: string, x: number, y: number, em: number) => {
  const width = nameWidth(name) * em;
  return { x0: x - width / 2, y0: y - em / 2, x1: x + width / 2, y1: y + em / 2 };
};

describe('the height of a map mark', () => {
  it('is up from about 15° above the ears, down from about 15° below', () => {
    expect(heightOf(0)).toBe('flat');
    expect(heightOf(0.2)).toBe('flat');
    expect(heightOf(-0.2)).toBe('flat');
    expect(heightOf(0.25)).toBe('up');
    expect(heightOf(0.707)).toBe('up');
    expect(heightOf(-0.25)).toBe('down');
    expect(heightOf(-0.5)).toBe('down');
    const at = (elevation_deg: number) => directionHeight({ id: 0, name: 'X', azimuth_deg: 0, elevation_deg });
    expect([at(0), at(30), at(45), at(90), at(-30)]).toEqual(['flat', 'up', 'up', 'up', 'down']);
  });
});

describe.each(GEOMETRIES)('the names on the %s map', (_skin, geometry) => {
  const sparse: [string, DirectionView[]][] = [
    ['2.0', speakerDirections('2.0')],
    ['7.1.4 by layout', speakerDirections('7.1.4')],
    ['5.1.4 by layout', speakerDirections('5.1.4')],
    ['7.1.4 BRIR set', fixture('running').profile.directions],
    ['SECS 2R', fixture('ready-speakers').profile.directions],
  ];

  it.each(sparse)('keep their usual places for %s', (_name, directions) => {
    const placed = placeDirections(directions, geometry);
    for (const [index, direction] of directions.entries()) {
      const [x, y] = usualPlace(direction, geometry);
      expect(placed[index]?.x, direction.name).toBeCloseTo(x, 6);
      expect(placed[index]?.y, direction.name).toBeCloseTo(y, 6);
    }
  });

  const dense: SpeakerLayout[] = ['9.1.6', '13.1', '22.2', '24.1.10', '30.2'];
  it.each(dense)('stay apart, clear of the other points and inside the drawing for %s', (layout) => {
    const directions = speakerDirections(layout);
    const placed = placeDirections(directions, geometry);
    const boxes = placed.map((mark) => boxOf(mark.direction.name, mark.x, mark.y, geometry.em));
    for (const [index, box] of boxes.entries()) {
      const name = placed[index]!.direction.name;
      expect(box.x0, `${name} left`).toBeGreaterThanOrEqual(0);
      expect(box.y0, `${name} top`).toBeGreaterThanOrEqual(0);
      expect(box.x1, `${name} right`).toBeLessThanOrEqual(geometry.size);
      expect(box.y1, `${name} bottom`).toBeLessThanOrEqual(geometry.size);
      for (const [other, otherBox] of boxes.entries()) {
        if (other <= index) continue;
        const overlaps =
          Math.min(box.x1, otherBox.x1) > Math.max(box.x0, otherBox.x0) &&
          Math.min(box.y1, otherBox.y1) > Math.max(box.y0, otherBox.y0);
        expect(overlaps, `${name} and ${placed[other]!.direction.name}`).toBe(false);
      }
      for (const point of placed) {
        if (point === placed[index]) continue;
        const dx = Math.max(box.x0 - point.cx, 0, point.cx - box.x1);
        const dy = Math.max(box.y0 - point.cy, 0, point.cy - box.y1);
        expect(Math.hypot(dx, dy), `${name} over ${point.direction.name}`).toBeGreaterThan(geometry.pointRadius * 0.5);
      }
    }
  });
});
