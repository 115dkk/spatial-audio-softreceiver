// The pedal skin (ADR 0046, 이펙터의 법칙): every fixture draws on every
// face its phase has, in both languages; the handles the gallery and the
// other skins share are where they should be; the box's parts and the
// faces' parts send what the standard screen sends; the face keys are tabs
// lit by their own ring; a key whose press leaves no state has no light;
// and the 30 Hz meters never add or remove an element.

import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { beforeEach, describe, expect, it } from 'vitest';
import type { Api } from '../src/api';
import type { EngineStatus, Intent, Language, Snapshot } from '../src/bindings';
import { fixture, fixtureNames } from '../src/fixtures';
import { type MockOptions, createMockApi } from '../src/mock';
import type { Tab } from '../src/screens/common';
import { mount } from '../src/skins/pedal';
import { segCells } from '../src/skins/pedal/parts';
import { en, ko } from '../src/text';

const HANGUL = /[ᄀ-ᇿ㄰-㆏가-힯]/;
const TABS: Tab[] = ['status', 'profile', 'upmix', 'settings'];

let root: HTMLElement;

beforeEach(() => {
  root = document.createElement('div');
  document.body.replaceChildren(root);
});

const tick = () => new Promise((resolve) => setTimeout(resolve, 0));
const $ = <T extends Element = HTMLElement>(selector: string) => root.querySelector<T>(selector);
const $$ = <T extends Element = HTMLElement>(selector: string) => [...root.querySelectorAll<T>(selector)];

/** The pedal over a bridge that records what it is sent (or the mock's own answers). */
function mounted(start: Snapshot, tab?: Tab, dispatch?: Api['dispatch'], options: MockOptions = {}) {
  const sent: Intent[] = [];
  const mock = createMockApi(start, options);
  const api: Api = { ...mock, dispatch: dispatch ?? (async (intent) => void sent.push(intent)) };
  const app = mount(root, api, tab === undefined ? {} : { tab });
  app.draw(start);
  if (start.phase.kind === 'running' && start.engine !== null) app.updateMeters(start.engine);
  return { app, sent };
}

function key(event: 'keydown' | 'keyup', target: Element, name: string) {
  target.dispatchEvent(new KeyboardEvent(event, { key: name, bubbles: true }));
}

/** Everything a person reads or hears on the page. */
function words(element: Element): string {
  const attributes = [...element.querySelectorAll('[aria-label], [title]')].flatMap((node) =>
    ['aria-label', 'title'].map((name) => node.getAttribute(name) ?? ''),
  );
  return [element.textContent ?? '', ...attributes].join('\n');
}

describe('every fixture', () => {
  for (const language of ['ko', 'en'] as Language[]) {
    for (const name of fixtureNames) {
      it(`${name} draws on each face of its phase (${language})`, () => {
        const start = fixture(name, language);
        const wizard = start.phase.kind === 'setup' || start.phase.kind === 'quit';
        for (const tab of wizard ? [undefined] : TABS) {
          root.replaceChildren();
          mounted(fixture(name, language), tab);
          expect($('.pd-box'), `${name} ${tab ?? 'wizard'}`).not.toBeNull();
          expect($('.pd-face')).not.toBeNull();
          const said = words(root);
          if (language === 'en') expect(said.match(HANGUL)?.[0] ?? null, `${name} ${tab ?? 'wizard'}`).toBeNull();
          else expect(HANGUL.test(said)).toBe(true);
          // Faces of the four tabs are the face keys' panel; setup and the closing face are not.
          expect($('#pd-face')?.getAttribute('role') ?? null).toBe(wizard ? null : 'tabpanel');
        }
      });
    }
  }
});

describe('the handles every skin shares (ADR 0041)', () => {
  it('power stands on the box in every phase, off in setup and after quitting', () => {
    for (const name of ['ready', 'running', 'faulted']) {
      root.replaceChildren();
      mounted(fixture(name), 'status');
      expect($<HTMLButtonElement>('[data-key="power"]')?.disabled, name).toBe(false);
    }
    for (const name of ['setup-welcome', 'quit']) {
      root.replaceChildren();
      mounted(fixture(name));
      expect($<HTMLButtonElement>('[data-key="power"]')?.disabled, name).toBe(true);
    }
  });

  it('the driver step has install, busy while it is sent', () => {
    mounted(fixture('setup-driver-not-installed'), undefined, () => new Promise<void>(() => {}));
    const install = $('[data-key="install"][data-intent="install_driver"]')!;
    expect(install).not.toBeNull();
    install.click();
    expect($('[data-key="install"]')!.getAttribute('aria-busy')).toBe('true');
  });

  it('outputs-toggle opens the output row, a lit key per output for the one in use', () => {
    const start = fixture('ready');
    mounted(start, 'status');
    expect($('[data-region="output-list"]')).toBeNull();
    expect($('[data-key="outputs-toggle"]')!.classList.contains('is-lit')).toBe(false);
    $('[data-key="outputs-toggle"]')!.click();
    const list = $('[data-region="output-list"]');
    expect(list).not.toBeNull();
    expect(list!.querySelectorAll('button')).toHaveLength(start.outputs.length);
    const toggle = $('[data-key="outputs-toggle"]')!;
    expect(toggle.getAttribute('aria-expanded')).toBe('true');
    expect(toggle.classList.contains('is-lit')).toBe(true);
    const lit = [...list!.querySelectorAll('button.is-lit')];
    expect(lit.map((button) => button.getAttribute('aria-checked'))).toEqual(['true']);
    expect(lit[0]?.textContent).toContain(start.chosen_output!.name);
  });

  it('the profile keys open their regions, their ring lit on the one shown', () => {
    mounted(fixture('ready'), 'profile');
    expect($('[data-region="profile-speakers"]')).not.toBeNull();
    $('[data-key="profile-tab-headphones"]')!.click();
    expect($('[data-region="profile-headphones"]')).not.toBeNull();
    expect($('[data-region="profile-speakers"]')).toBeNull();
    expect($('#profile-panel-headphones')?.getAttribute('aria-labelledby')).toBe('profile-tab-headphones');
    const tab = (kind: string) => $(`[data-key="profile-tab-${kind}"]`)!;
    expect(tab('headphones').getAttribute('aria-selected')).toBe('true');
    expect(tab('headphones').classList.contains('is-lit')).toBe(true);
    expect(tab('speakers').classList.contains('is-lit')).toBe(false);
    tab('speakers').click();
    expect($('[data-region="profile-speakers"]')).not.toBeNull();
  });

  it('a refused record shows an error band that can be closed', async () => {
    const start = fixture('ready');
    const api = createMockApi(start);
    const app = mount(root, api, { tab: 'settings' });
    app.draw(start);
    expect($('[data-region="error-band"]')).toBeNull();
    $('[data-key="record"]')!.click();
    await tick();
    await tick();
    const band = $('[data-region="error-band"]');
    expect(band?.getAttribute('role')).toBe('alert');
    // The refusal stands at the face's foot, with a red LED and a momentary key.
    expect(band?.closest('.pd-refusals')).not.toBeNull();
    expect(band?.querySelector('.pd-led')?.getAttribute('data-glow')).toBe('on');
    expect(band?.querySelector('button')?.textContent).toContain(ko.banner.dismiss);
    expect(band?.querySelector('button')?.classList.contains('pd-mkey-key')).toBe(true);
  });

  it("the snapshot's error stands near the top of the face with its action", () => {
    mounted(fixture('faulted'), 'status');
    const band = $('[data-region="error-band"]');
    expect(band?.getAttribute('role')).toBe('alert');
    expect(band?.closest('.pd-top')).not.toBeNull();
    expect(band?.querySelector('[data-intent="recheck"]')).not.toBeNull();
  });

  it('notes are printed, an amber LED by each', () => {
    const start = fixture('ready');
    start.notes = ['note one', 'note two'];
    mounted(start, 'status');
    const lines = $$('.pd-notes li');
    expect(lines.map((line) => line.textContent)).toEqual(['note one', 'note two']);
    expect(lines.every((line) => line.querySelector('.pd-led')?.getAttribute('style')?.includes('--pd-led-wait'))).toBe(true);
  });
});

describe('the face keys (법칙 3)', () => {
  it('are tabs in a tablist, aria-selected exactly where the ring is lit, no LED of their own', () => {
    for (const tab of TABS) {
      root.replaceChildren();
      mounted(fixture('ready'), tab);
      const list = $('[role="tablist"].pd-facekeys')!;
      const keys = [...list.querySelectorAll<HTMLButtonElement>('[role="tab"]')];
      expect(keys.map((face) => face.textContent)).toEqual([ko.nav.status, ko.nav.profile, ko.nav.upmix, ko.nav.settings]);
      for (const face of keys) {
        expect(face.getAttribute('aria-selected') === 'true', `${tab} ${face.dataset.key}`).toBe(face.classList.contains('is-lit'));
        expect(face.getAttribute('aria-controls')).toBe('pd-face');
        expect(face.querySelector('.pd-ikey-act')).not.toBeNull();
      }
      expect(keys.filter((face) => face.classList.contains('is-lit')).map((face) => face.dataset.key)).toEqual([`nav-${tab}`]);
      // No LED stands above or beside a face key.
      expect(list.querySelector('.pd-led')).toBeNull();
      expect(list.parentElement?.querySelector(':scope > .pd-led')).toBeNull();
      expect($('#pd-face')!.getAttribute('aria-labelledby')).toBe(`pd-tab-${tab}`);
    }
  });

  it('open a face on press and move with the arrow keys', () => {
    mounted(fixture('ready'), 'status');
    $('[data-key="nav-upmix"]')!.click();
    expect($('#pd-face')!.getAttribute('aria-labelledby')).toBe('pd-tab-upmix');
    expect($('#pd-face')!.dataset.face).toBe('upmix');
    const upmix = $('[data-key="nav-upmix"]')!;
    upmix.focus();
    key('keydown', upmix, 'ArrowRight');
    expect($('#pd-face')!.dataset.face).toBe('settings');
  });

  it('are dark and off in setup and after quitting, and every lamp is dark', () => {
    for (const name of ['setup-welcome', 'setup-driver-not-installed', 'quit']) {
      root.replaceChildren();
      mounted(fixture(name));
      const keys = $$<HTMLButtonElement>('[role="tab"][data-key^="nav-"]');
      expect(keys, name).toHaveLength(4);
      expect(keys.every((face) => face.disabled && !face.classList.contains('is-lit')), name).toBe(true);
      expect($$('.pd-lamps .pd-led').map((lamp) => lamp.getAttribute('data-glow')), name).toEqual(['off', 'off', 'off']);
    }
  });
});

describe('the box and the faces send', () => {
  it('power sends enable while off, and shows it in flight', () => {
    const { sent } = mounted(fixture('ready'), 'status', () => new Promise<void>(() => {}));
    $('[data-key="power"]')!.click();
    expect(sent).toEqual([]);
    expect($('[data-key="power"]')!.getAttribute('aria-busy')).toBe('true');
    root.replaceChildren();
    const recorded = mounted(fixture('ready'), 'status');
    $('[data-key="power"]')!.click();
    expect(recorded.sent).toEqual([{ kind: 'enable' }]);
  });

  it('power sends disable while running', () => {
    const { sent } = mounted(fixture('running'), 'status');
    $('[data-key="power"]')!.click();
    expect(sent).toEqual([{ kind: 'disable' }]);
  });

  it('a key of the output row sends choose_output', () => {
    const start = fixture('ready');
    const { sent } = mounted(start, 'status');
    $('[data-key="outputs-toggle"]')!.click();
    const other = start.outputs.findIndex((output) => output.id !== start.chosen_output?.id);
    $(`[data-region="output-list"] [data-key="status-output-${other}"]`)!.click();
    expect(sent).toEqual([{ kind: 'choose_output', value: start.outputs[other]!.id }]);
  });

  it('the output kind is a slide switch that sends set_profile_kind', () => {
    const { sent } = mounted(fixture('ready-headphones'), 'status');
    const group = $('[data-key="kind"]')!;
    expect(group.getAttribute('role')).toBe('radiogroup');
    expect($('[data-key="kind-headphones"]')!.getAttribute('aria-checked')).toBe('true');
    expect(group.getAttribute('style')).toContain('--at:0');
    $('[data-key="kind-speakers"]')!.click();
    expect(sent).toEqual([{ kind: 'set_profile_kind', value: 'speakers' }]);
  });

  it('headphones without a BRIR set say why in print instead of sending', () => {
    const { sent } = mounted(fixture('ready'), 'status');
    const headphones = $('[data-key="kind-headphones"]')!;
    expect(headphones.getAttribute('aria-disabled')).toBe('true');
    const why = document.getElementById(headphones.getAttribute('aria-describedby') ?? '');
    expect(why?.textContent).toBe(ko.profile.needsBrir);
    headphones.click();
    expect(sent).toEqual([]);
  });

  it('the LEVEL encoder turns by keys, its readout following, and sends set_gain once let go', () => {
    const start = fixture('ready-headphones');
    const { sent } = mounted(start, 'status');
    const knob = $('[data-key="gain"]')!;
    expect(knob.getAttribute('role')).toBe('slider');
    key('keydown', knob, 'ArrowUp');
    key('keydown', knob, 'ArrowUp');
    expect(knob.getAttribute('aria-valuetext')).toBe(`${(start.profile.gain_db + 1).toFixed(1)} dB`);
    // The segment readout follows the knob before anything is sent.
    expect($('.pd-gain-readout')!.textContent).toContain((start.profile.gain_db + 1).toFixed(1).replace('-', '−'));
    expect($$('.pd-encoder .pd-ring-led.is-on').length).toBeGreaterThan(0);
    expect(sent).toEqual([]);
    key('keyup', knob, 'ArrowUp');
    expect(sent).toEqual([{ kind: 'set_gain', value: start.profile.gain_db + 1 }]);
  });

  it('the skin is a rotary selector that sends set_skin', () => {
    const { sent } = mounted({ ...fixture('ready'), skin: 'pedal' }, 'settings');
    expect($('[data-key="skin-pedal"]')!.getAttribute('aria-checked')).toBe('true');
    expect($('[data-key="skin-pedal"]')!.closest('.pd-rotary')?.getAttribute('role')).toBe('radiogroup');
    $('[data-key="skin-console"]')!.click();
    expect(sent).toEqual([{ kind: 'set_skin', value: 'console' }]);
  });

  it('the on/off options are DIP switches that toggle', async () => {
    const { sent } = mounted(fixture('ready'), 'settings');
    const asio = $('[data-key="asio-enabled"]')!;
    expect(asio.closest('.pd-dip')).not.toBeNull();
    expect(asio.getAttribute('aria-pressed')).toBe('true');
    asio.click();
    await tick();
    expect(sent).toEqual([{ kind: 'set_asio_enabled', value: false }]);
  });

  it('a button on the map plays the test signal while running', () => {
    const start = fixture('running');
    const { sent } = mounted(start, 'status');
    const first = start.engine!.directions[0]!;
    $(`[data-key="map-direction-${first.id}"]`)!.click();
    expect(sent).toEqual([{ kind: 'test_direction', value: first.id }]);
  });

  it('the ASIO attenuation is a knob with its readout', () => {
    const start = fixture('ready');
    const { sent } = mounted(start, 'settings');
    const knob = $('[data-key="asio-attenuation"]')!;
    expect(knob.getAttribute('role')).toBe('slider');
    key('keydown', knob, 'ArrowUp');
    key('keyup', knob, 'ArrowUp');
    expect(sent).toEqual([{ kind: 'set_asio_system_attenuation', value: (start.asio?.system_attenuation_db ?? 0) + 1 }]);
  });

  it('English prints the box in English', () => {
    mounted(fixture('ready', 'en'), 'status');
    expect($('#pd-power-print')!.textContent).toBe(en.pedal.power);
    expect($$('.pd-jack-list .pd-name').map((name) => name.textContent)).toEqual(en.pedal.lamps);
    expect($$('.pd-lamps .pd-label').map((name) => name.textContent)).toEqual([en.pedal.upmixLamp, en.pedal.brirLamp, en.status.on]);
  });
});

describe('the lights say what the snapshot says', () => {
  const glow = (lamp: string) => $(`.pd-lamp[data-lamp="${lamp}"] .pd-led`)!.getAttribute('data-glow');

  it('켜짐 follows the engine, BRIR the headphones with a set, 업믹스 an applied upmixer', () => {
    mounted(fixture('ready'), 'status');
    expect([glow('upmix'), glow('brir'), glow('power')]).toEqual(['off', 'off', 'off']);
    root.replaceChildren();
    mounted(fixture('running'), 'status');
    expect([glow('upmix'), glow('brir'), glow('power')]).toEqual(['off', 'on', 'on']);
    root.replaceChildren();
    mounted(fixture('running-upmix'), 'status');
    expect([glow('upmix'), glow('brir'), glow('power')]).toEqual(['on', 'on', 'on']);
    root.replaceChildren();
    // The speakers' upmix switch is off: nothing is applied to the output in use.
    mounted(fixture('ready-speakers-upmix-off'), 'status');
    expect(glow('upmix')).toBe('off');
    root.replaceChildren();
    mounted(fixture('faulted'), 'status');
    expect($('.pd-lamp[data-lamp="power"] .pd-led')!.getAttribute('style')).toContain('--pd-led-fault');
  });

  it('the update apply key is a momentary key with no light', async () => {
    const start = fixture('ready');
    mounted(start, 'settings', undefined, { shell: { autostart: false, update: { current: '0.30.2', ready: '0.31.0' } } });
    await tick();
    await tick();
    const apply = $('[data-key="update-apply"]')!;
    expect(apply).not.toBeNull();
    expect(apply.classList.contains('pd-mkey-key')).toBe(true);
    expect(apply.querySelector('.pd-mkey-cap')).not.toBeNull();
    expect(apply.querySelector('.pd-ikey, .pd-led')).toBeNull();
    expect(apply.classList.contains('is-lit')).toBe(false);
    // The diagnostic recording's key has a light: red, dark until it is pressed.
    const record = $('[data-key="record"]')!;
    expect(record.querySelector('.pd-ikey-act')).not.toBeNull();
    expect(record.getAttribute('style')).toContain('--pd-led-fault');
    expect(record.classList.contains('is-lit')).toBe(false);
  });

  it('the arrow keys at the face stand dark and off until the face runs on', () => {
    mounted(fixture('ready'), 'settings');
    const up = $<HTMLButtonElement>('[data-key="face-up"]')!;
    const down = $<HTMLButtonElement>('[data-key="face-down"]')!;
    expect(up.textContent).toBe(ko.pedal.scrollUp);
    expect(down.textContent).toBe(ko.pedal.scrollDown);
    expect(up.disabled && down.disabled).toBe(true);
    expect(up.classList.contains('is-lit') || down.classList.contains('is-lit')).toBe(false);
  });
});

describe('the 30 Hz meters', () => {
  function changed(engine: EngineStatus): EngineStatus {
    const next = structuredClone(engine);
    next.scene = [...next.scene.slice(1), { lane: 1, index: 3, x: 0.7, y: -0.7, z: 0.4, level_dbfs: -6 }];
    next.underruns += 3;
    next.lost_bytes = 123_456;
    const first = next.endpoints[0]!;
    first.chain_errors = { ...first.chain_errors, corrupt_payload: 4, future_kind: 2 } as typeof first.chain_errors;
    first.unsupported = 'format';
    first.muted = true;
    next.endpoints[1]!.format = null;
    return next;
  }

  it.each(['status', 'profile'] as Tab[])('change no element count on the %s face', (tab) => {
    for (const name of ['running', 'running-asio', 'running-apps', 'running-upmix']) {
      root.replaceChildren();
      const start = fixture(name);
      const { app } = mounted(start, tab);
      const before = root.querySelectorAll('*').length;
      app.updateMeters(changed(start.engine!));
      expect(root.querySelectorAll('*').length, `${name} ${tab}`).toBe(before);
      app.updateMeters(start.engine!);
      expect(root.querySelectorAll('*').length, `${name} ${tab}`).toBe(before);
    }
  });

  it('light the bar graph, the readouts and the chain errors by kind in place', () => {
    const start = fixture('running');
    const { app } = mounted(start, 'status');
    const lane = $('.pd-ch[data-lane="0"]')!;
    const lit = () => lane.querySelectorAll('.pd-ladder i.is-on').length;
    expect(lit()).toBeGreaterThan(0);
    app.updateMeters(changed(start.engine!));
    expect(lane.querySelector('.pd-ch-notes')!.textContent).toContain(ko.unsupported.format);
    const kinds = [...$$('.pd-kinds li')].filter((line) => !line.hidden).map((line) => line.textContent);
    expect(kinds.some((line) => line?.includes(ko.chainErrorKinds.corrupt_payload ?? ''))).toBe(true);
    expect(kinds.some((line) => line?.includes('future_kind'))).toBe(true);
    // Silence on a lane darkens its bar graph.
    const silent = structuredClone(start.engine!);
    silent.scene = [];
    app.updateMeters(silent);
    expect(lit()).toBe(0);
  });
});

/** What the eye reads in an element: its text without the screen-reader-only words and hidden parts. */
function seen(element: Element): string {
  const copy = element.cloneNode(true) as Element;
  for (const node of copy.querySelectorAll('.pd-sr, [hidden]')) node.remove();
  return copy.textContent ?? '';
}

describe('the upmix face prints only what is chosen (법칙 5)', () => {
  const bodies = (built: boolean) => ({
    first_party: built ? ko.upmix.firstPartyBody : `${ko.upmix.firstPartyBody} ${ko.upmix.firstPartyHint}`,
    plugin: ko.upmix.pluginBody,
    off: ko.upmix.offBody,
  });

  it('one description per input, the chosen upmixer’s, under its lit key; the others only for a screen reader', () => {
    for (const name of ['running-upmix', 'ready-upmix', 'ready']) {
      root.replaceChildren();
      const start = fixture(name);
      mounted(start, 'upmix');
      const all = bodies(start.upmix.first_party_available);
      for (const view of start.upmix.endpoints) {
        const input = $(`.pd-upmix-input[data-lane="${view.endpoint}"]`)!;
        const captions = [...input.querySelectorAll('.pd-callout')];
        expect(captions, `${name} ${view.endpoint}`).toHaveLength(1);
        expect(captions[0]!.textContent).toBe(all[view.effective]);
        // The lit key is the chosen one, and the row is described by the caption.
        const lit = [...input.querySelectorAll('.pd-upkeys .pd-key.is-chosen')];
        expect(lit.map((key) => key.getAttribute('data-key'))).toEqual([`upmixer-${view.endpoint}-${view.effective}`]);
        expect(input.querySelector('.pd-upkeys')!.getAttribute('aria-describedby')).toContain(captions[0]!.id);
        // The others are not printed, but each unchosen key still says its own.
        const printed = seen(input);
        for (const [choice, body] of Object.entries(all)) {
          if (choice === view.effective) continue;
          expect(printed.includes(body), `${name} ${view.endpoint} ${choice} printed`).toBe(false);
          const key = input.querySelector(`[data-key="upmixer-${view.endpoint}-${choice}"]`)!;
          expect(document.getElementById(key.getAttribute('aria-describedby') ?? '')?.textContent).toBe(body);
        }
      }
    }
  });

  it('the running upmixer’s outputs light their positions on the channel indicators', () => {
    const start = fixture('running-upmix');
    mounted(start, 'upmix');
    const view = start.upmix.endpoints.find((endpoint) => endpoint.state.kind === 'running')!;
    const strip = $(`.pd-upmix-input[data-lane="${view.endpoint}"] .pd-upout`)!;
    const lit = [...strip.querySelectorAll('.pd-chan.is-lit')].map((spot) => spot.getAttribute('data-chan'));
    expect([...lit].sort()).toEqual([...view.outputs].sort());
    expect(strip.querySelectorAll('.pd-chan:not(.is-lit) .pd-led[data-glow="on"]')).toHaveLength(0);
    // The names are for the eye; a screen reader hears the sentence the text said before.
    expect(strip.querySelector('.pd-sr')?.textContent).toBe(ko.upmix.outputs(view.outputs.join(' ')));
  });

  it('the plugin facts stand on a spec plate', () => {
    const start = fixture('running-upmix');
    mounted(start, 'upmix');
    const view = start.upmix.endpoints.find((endpoint) => endpoint.plugin !== null)!;
    const plate = $(`.pd-upmix-input[data-lane="${view.endpoint}"] .pd-spec`)!;
    const rows = [...plate.querySelectorAll('.pd-spec-row')].map((row) => row.querySelector('dt')?.textContent);
    expect(rows).toContain(ko.upmix.pluginName);
    expect(plate.textContent).toContain(view.plugin!.name);
    expect(plate.textContent).toContain(view.plugin!.path);
  });
});

describe('a part’s caution stands on the legend plate by its number (법칙 5)', () => {
  it('an unsupported format puts a mark in the channel and its sentence on the plate, not in the column', () => {
    const start = fixture('running');
    const engine = structuredClone(start.engine!);
    for (const status of engine.endpoints) {
      status.unsupported = null;
      status.chain_errors = {};
    }
    engine.endpoints[0]!.unsupported = 'format';
    engine.asio = null;
    const { app } = mounted({ ...start, engine }, 'status');
    app.updateMeters(engine);
    const sentence = ko.unsupported.format;
    const lane = $('.pd-ch[data-lane="0"]')!;
    const marks = [...lane.querySelectorAll('.pd-mark:not([hidden])')];
    expect(marks).toHaveLength(1);
    expect(marks[0]!.querySelector('.pd-led')?.getAttribute('style')).toContain('--pd-led-wait');
    expect(marks[0]!.querySelector('.pd-mark-n')?.textContent).toBe('1');
    // Not printed in the narrow column; a screen reader still hears it there.
    expect(seen(lane).includes(sentence)).toBe(false);
    expect(marks[0]!.querySelector('.pd-sr')?.textContent).toBe(sentence);
    // The plate prints it by the same number, with the channel's name.
    const plate = $('.pd-noteplate')!;
    expect(plate.hidden).toBe(false);
    const rows = [...plate.querySelectorAll('.pd-noterow:not([hidden])')];
    expect(rows).toHaveLength(1);
    expect(rows[0]!.querySelector('.pd-mark-n')?.textContent).toBe('1');
    expect(rows[0]!.textContent).toContain(sentence);
    expect(rows[0]!.textContent).toContain(ko.pedal.lamps[0]);
  });

  it('numbers the notes in reading order and hides the plate when none is left', () => {
    const start = fixture('running');
    const { app } = mounted(start, 'status');
    const numbers = () => $$('.pd-ch .pd-mark:not([hidden]) .pd-mark-n').map((mark) => mark.textContent);
    const shown = numbers();
    expect(shown.length).toBeGreaterThan(0);
    expect(shown).toEqual(shown.map((_, index) => String(index + 1)));
    const quiet = structuredClone(start.engine!);
    for (const status of quiet.endpoints) {
      status.unsupported = null;
      status.chain_errors = {};
    }
    quiet.asio = null;
    app.updateMeters(quiet);
    expect(numbers()).toEqual([]);
    expect($('.pd-noteplate')!.hidden).toBe(true);
  });
});

describe('the output channels are a channel indicator strip', () => {
  it('on the status face, lit for the channels the output carries', () => {
    const start = fixture('running-speakers');
    mounted(start, 'status');
    const strip = $('.pd-outchans')!;
    const spots = [...strip.querySelectorAll('.pd-chan')];
    expect(spots.length).toBeGreaterThanOrEqual(start.engine!.output_channels);
    expect(strip.querySelectorAll('.pd-chan.is-lit')).toHaveLength(start.engine!.output_channels);
    expect(spots.map((spot) => spot.getAttribute('data-chan')).slice(0, start.profile.speakers.channels.length)).toEqual(
      [...start.profile.speakers.channels].sort((a, b) => a.channel - b.channel).map((channel) => channel.name),
    );
  });

  it('on the profile face, an LED per device channel lit while the speakers play through it', () => {
    const start = fixture('running-speakers');
    mounted(start, 'profile');
    const leds = $$('.pd-patch tbody .pd-led');
    expect(leds).toHaveLength(start.profile.speakers.channels.length);
    expect(leds.every((light) => light.getAttribute('data-glow') === 'on')).toBe(true);
    root.replaceChildren();
    mounted(fixture('ready-speakers'), 'profile');
    expect($$('.pd-patch tbody .pd-led').every((light) => light.getAttribute('data-glow') === 'off')).toBe(true);
  });
});

describe('the print (법칙 6)', () => {
  it('is never Regular: no rule of the skin sets 400', () => {
    // Vite rewrites `new URL(path, import.meta.url)` into an asset URL here, so the path is joined by hand.
    const css = readFileSync(join(dirname(fileURLToPath(import.meta.url)), '../src/skins/pedal/pedal.css'), 'utf8');
    expect(css.match(/font-weight:\s*(400|normal)\b/g)).toBeNull();
    expect(css).toMatch(/\.pd-room \{[^}]*font-weight: 500;/);
    expect(css).toMatch(/\.pd-room \{[^}]*text-shadow: var\(--pd-lip\);/);
  });
});

describe('the segment readouts', () => {
  it('light a cell per digit or sign, the point riding on the cell before it', () => {
    expect(segCells('−6.0', 4).map((cell) => `${cell.glyph}${cell.point ? '.' : ''}`)).toEqual([' ', '−', '6.', '0']);
    expect(segCells('1,204', 4).map((cell) => cell.glyph)).toEqual(['1', '2', '0', '4']);
    // Wider than the readout: dashes, as a real one overflows.
    expect(segCells('123456', 4).map((cell) => cell.glyph)).toEqual(['-', '-', '-', '-']);
    expect(segCells('+2.0', 4).map((cell) => cell.glyph)).toEqual([' ', ' ', '2', '0']);
  });
});
