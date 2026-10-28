// The high-channel layouts on the screen (ADR 0042) in all three skins
// (ADR 0041): the 16 speaker layouts and 11 ASIO layouts to choose from, a
// test signal per direction with no cap, and the direction maps' third
// height, below the ears, with its legend line only while something below
// is drawn.

import { beforeEach, describe, expect, it } from 'vitest';
import type { Api } from '../src/api';
import { mountApp } from '../src/app';
import type { EngineStatus, Intent, Snapshot } from '../src/bindings';
import { fixture } from '../src/fixtures';
import { ASIO_LAYOUTS, SPEAKER_LAYOUTS } from '../src/guard';
import { createMockApi } from '../src/mock';
import type { Tab } from '../src/screens/common';
import { mount as mountConsole } from '../src/skins/console';
import { mount as mountPanel } from '../src/skins/panel';
import { layoutFamilies } from '../src/skins/panel/ui';
import type { App } from '../src/skins/types';
import { ko } from '../src/text';

let root: HTMLElement;

beforeEach(() => {
  root = document.createElement('div');
  document.body.replaceChildren(root);
});

const $ = <T extends Element = HTMLElement>(selector: string) => root.querySelector<T>(selector);
const $$ = <T extends Element = HTMLElement>(selector: string) => [...root.querySelectorAll<T>(selector)];

type Mount = (root: HTMLElement, api: Api, options: { tab?: Tab }) => App;

/** What each skin calls the parts of its map. */
interface Skin {
  name: string;
  mount: Mount;
  /** A direction's point (its class says its height). */
  point: string;
  /** The pool's object marks. */
  object: string;
  /** The legend's lines and the below-ear swatch in one. */
  legendItem: string;
  belowSwatch: string;
}

const SKINS: Skin[] = [
  { name: 'standard', mount: mountApp, point: '.map-direction', object: '.map-object', legendItem: '.map-legend li', belowSwatch: '.map-swatch.is-down' },
  { name: 'panel', mount: mountPanel, point: '.pn-map-point', object: '.pn-map-object', legendItem: '.pn-map-legend li', belowSwatch: '.pn-swatch.is-down' },
  { name: 'console', mount: mountConsole, point: '.cn-map-direction', object: '.cn-map-object', legendItem: '.cn-map-legend li', belowSwatch: '.cn-map-key.is-down' },
];

function mounted(skin: Skin, start: Snapshot, tab: Tab = 'profile') {
  const sent: Intent[] = [];
  const api: Api = { ...createMockApi(start), dispatch: async (intent) => void sent.push(intent) };
  const app = skin.mount(root, api, { tab });
  app.draw(start);
  if (start.phase.kind === 'running' && start.engine !== null) app.updateMeters(start.engine);
  return { app, sent };
}

/** The direction points, by the name their title or label gives, with their height. */
function heights(skin: Skin): Record<string, string> {
  return Object.fromEntries(
    $$(skin.point).map((point) => {
      const named = point.querySelector('title')?.textContent ?? point.getAttribute('aria-label') ?? point.getAttribute('title') ?? '';
      const name = named.split(/ · |, /)[0]!;
      const height = point.classList.contains('is-down') ? 'down' : point.classList.contains('is-up') ? 'up' : 'flat';
      return [name, height];
    }),
  );
}

const shownObjects = (skin: Skin) => $$(skin.object).filter((mark) => mark.getAttribute('display') !== 'none');
const belowLine = (skin: Skin) => root.querySelector(skin.belowSwatch)?.closest('li') ?? null;
const visibleLegend = (skin: Skin) => $$(skin.legendItem).filter((item) => !item.hidden).map((item) => item.textContent);

describe.each(SKINS)('the $name map', (skin) => {
  it('draws the bottom ring of 30.2 below the ears and names it in the legend', () => {
    mounted(skin, fixture('ready-speakers-30-2'));
    const drawn = heights(skin);
    expect(Object.keys(drawn)).toHaveLength(30);
    expect(Object.entries(drawn).filter(([, height]) => height === 'down').map(([name]) => name)).toEqual(['Bfc', 'Bfl', 'Bfr']);
    expect(Object.values(drawn).filter((height) => height === 'up')).toHaveLength(13);
    expect(belowLine(skin)?.hidden).toBe(false);
    expect(belowLine(skin)?.textContent).toBe(ko.profile.legendDown);
  });

  it('keeps a 7.1.4 map and its legend as they were: no line for below the ears', () => {
    mounted(skin, fixture('running'));
    expect(Object.values(heights(skin))).not.toContain('down');
    expect(shownObjects(skin).some((mark) => mark.classList.contains('is-down'))).toBe(false);
    expect(belowLine(skin)?.hidden).toBe(true);
    expect(visibleLegend(skin)).not.toContain(ko.profile.legendDown);
    expect(visibleLegend(skin)).toContain(ko.profile.legendFlat);
    expect(visibleLegend(skin)).toContain(ko.profile.legendUp);
  });

  it('draws objects below the ears as such while a 22.2 source plays', () => {
    const start = fixture('running-headphones-22-2');
    mounted(skin, start);
    const scene = start.engine!.scene;
    const marks = shownObjects(skin);
    expect(marks).toHaveLength(scene.length);
    expect(marks.map((mark) => (mark.classList.contains('is-down') ? 'down' : mark.classList.contains('is-up') ? 'up' : 'flat'))).toEqual(
      scene.map((object) => (object.z <= -0.25 ? 'down' : object.z >= 0.25 ? 'up' : 'flat')),
    );
    expect(marks.filter((mark) => mark.classList.contains('is-down'))).toHaveLength(3);
    expect(belowLine(skin)?.hidden).toBe(false);
  });

  it('shows the legend line once an object goes below the ears, and keeps it after', () => {
    const start = fixture('running');
    const { app } = mounted(skin, start);
    expect(belowLine(skin)?.hidden).toBe(true);
    const engine: EngineStatus = structuredClone(start.engine!);
    engine.scene = [...engine.scene, { lane: 0, index: 9, x: 0, y: 0.866, z: -0.5, level_dbfs: -20 }];
    app.updateMeters(engine);
    expect(shownObjects(skin).at(-1)?.classList.contains('is-down')).toBe(true);
    expect(belowLine(skin)?.hidden).toBe(false);
    // The meters move on without it; the line does not blink with them.
    app.updateMeters(start.engine!);
    expect(belowLine(skin)?.hidden).toBe(false);
  });

  it('offers a test signal for every one of the 34 directions of 24.1.10', () => {
    const start = fixture('ready-headphones-24-1-10');
    mounted(skin, start);
    const tones = $$('[data-key^="direction-"]');
    expect(tones.map((tone) => tone.getAttribute('data-key'))).toEqual(start.profile.directions.map((direction) => `direction-${direction.id}`));
    expect(tones).toHaveLength(34);
    expect(tones.every((tone) => (tone as HTMLButtonElement).disabled)).toBe(true);
    expect(Object.keys(heights(skin))).toHaveLength(34);
  });

  it('sends a test signal from a below-ear direction while running', () => {
    const start = fixture('running-headphones-22-2');
    const { sent } = mounted(skin, start);
    const bfl = start.profile.directions.find((direction) => direction.name === 'Bfl')!;
    $<HTMLButtonElement>(`[data-key="direction-${bfl.id}"]`)!.click();
    expect(sent).toEqual([{ kind: 'test_direction', value: bfl.id }]);
  });
});

describe('the layouts to choose from', () => {
  const speakerLayouts = Object.keys(SPEAKER_LAYOUTS);
  const asioLayouts = Object.keys(ASIO_LAYOUTS);

  it.each([SKINS[0]!, SKINS[2]!])('the $name skin lists all 16 speaker layouts and all 11 ASIO layouts in order', (skin) => {
    const options = (select: string) => [...($(select)?.querySelectorAll('option') ?? [])].map((option) => option.getAttribute('value'));
    mounted(skin, fixture('ready-speakers-30-2'));
    expect(options('[data-key="speaker-layout"]')).toEqual(speakerLayouts);
    expect($('[data-key="speaker-layout"] option[selected]')?.getAttribute('value')).toBe('30.2');
    root.replaceChildren();
    mounted(skin, fixture('ready-asio-24-1-10'), 'settings');
    expect(options('[data-key="asio-layout"]')).toEqual(asioLayouts);
    expect($('[data-key="asio-layout"] option[selected]')?.textContent).toBe('24.1.10 (35채널)');
  });

  it('groups layouts one family a line, a lone family joining the line before', () => {
    expect(layoutFamilies(speakerLayouts)).toEqual([5, 4, 3, 4]);
    expect(layoutFamilies(asioLayouts)).toEqual([4, 3, 4]);
    expect(layoutFamilies([])).toEqual([]);
    expect(layoutFamilies(['7.1.4'])).toEqual([1]);
  });

  it('the panel puts the 16 speaker layouts in four family lines of one radio group', async () => {
    const { sent } = mounted(SKINS[1]!, fixture('ready-speakers-30-2'));
    const group = $('[role="radiogroup"][aria-labelledby="pn-layout-title"]')!;
    const lines = [...group.querySelectorAll('.pn-choice-line')].map((line) => [...line.querySelectorAll('[role="radio"]')].map((radio) => radio.textContent));
    expect(lines).toEqual([
      ['2.0', '2.1', '4.0', '5.1', '7.1'],
      ['5.1.2', '5.1.4', '7.1.2', '7.1.4'],
      ['9.1', '9.1.4', '9.1.6'],
      ['13.1', '22.2', '24.1.10', '30.2'],
    ]);
    const radios = [...group.querySelectorAll<HTMLButtonElement>('[role="radio"]')];
    expect(radios.map((radio) => radio.getAttribute('data-key'))).toEqual(speakerLayouts.map((layout) => `speaker-layout-${layout}`));
    // One stop for Tab, on the chosen layout; the arrows cross the lines.
    expect(radios.filter((radio) => radio.tabIndex === 0).map((radio) => radio.textContent)).toEqual(['30.2']);
    expect(radios.find((radio) => radio.getAttribute('aria-checked') === 'true')?.textContent).toBe('30.2');
    const sevenOne = radios[4]!;
    sevenOne.focus();
    sevenOne.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowRight', bubbles: true }));
    expect(document.activeElement?.textContent).toBe('5.1.2');
    radios[13]!.click();
    expect(sent).toEqual([{ kind: 'set_speaker_layout', value: '22.2' }]);
  });

  it('the panel puts the 11 ASIO layouts in three family lines with their channel counts', () => {
    mounted(SKINS[1]!, fixture('ready-asio-24-1-10'), 'settings');
    const group = $('[role="radiogroup"][aria-labelledby="asio-layout-label"]')!;
    const lines = [...group.querySelectorAll('.pn-choice-line')].map((line) =>
      [...line.querySelectorAll('[role="radio"] .pn-row-label')].map((label) => label.textContent),
    );
    expect(lines).toEqual([
      ['2.0', '5.1', '7.1', '7.1.4'],
      ['9.1', '9.1.4', '9.1.6'],
      ['13.1', '22.2', '24.1.10', '30.2'],
    ]);
    const chosen = group.querySelector('[aria-checked="true"]')!;
    expect(chosen.getAttribute('data-key')).toBe('asio-layout-24.1.10');
    expect(chosen.querySelector('.pn-row-detail')?.textContent).toBe('35채널');
  });

  it('the panel lists many test directions compactly, their angles in words for a screen reader', () => {
    const start = fixture('ready-headphones-24-1-10');
    mounted(SKINS[1]!, start);
    const list = $('.pn-tests[role="group"]')!;
    expect(list.classList.contains('is-dense')).toBe(true);
    const rsc = start.profile.directions.find((direction) => direction.name === 'Rsc')!;
    const row = $(`[data-key="direction-${rsc.id}"]`)!;
    expect(row.querySelector('.pn-row-label')?.textContent).toBe('Rsc');
    expect(row.querySelector('.pn-row-detail')?.textContent).toBe('-10° / 0°');
    expect(row.getAttribute('aria-label')).toBe(`Rsc, ${ko.profile.angles(-10, 0)}`);
    // A layout of a few directions keeps its rows as they were.
    root.replaceChildren();
    mounted(SKINS[1]!, fixture('running'));
    expect($('.pn-tests[role="group"]')!.classList.contains('is-dense')).toBe(false);
    expect($('[data-key="direction-0"]')!.hasAttribute('aria-label')).toBe(false);
  });

  it('the panel draws the names of a dense map in its small type', () => {
    mounted(SKINS[1]!, fixture('ready-headphones-24-1-10'));
    expect($('.pn-map')!.classList.contains('is-dense')).toBe(true);
    root.replaceChildren();
    mounted(SKINS[1]!, fixture('ready-headphones'));
    expect($('.pn-map')!.classList.contains('is-dense')).toBe(false);
  });
});
