// The pedal skin (ADR 0046, 이펙터의 법칙): every fixture draws on every
// face its phase has, in both languages; the handles the gallery and the
// other skins share are where they should be; the box's parts and the
// faces' parts send what the standard screen sends; the face keys are tabs
// lit by their own ring; a key whose press leaves no state has no light; a
// key shows what it stands for by its own light, not a word beside it; a
// file goes in a card slot; the per-app entrance is a routing matrix;
// and the 30 Hz meters never add or remove an element.

import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { beforeEach, describe, expect, it } from 'vitest';
import type { Api } from '../src/api';
import type { EngineStatus, Intent, Language, Snapshot } from '../src/bindings';
import { fixture, fixtureNames } from '../src/fixtures';
import { describeFormat } from '../src/format';
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

  it('the plugin stands as a card in its slot, the rest of its facts on a spec plate', () => {
    const start = fixture('running-upmix');
    const { sent } = mounted(start, 'upmix');
    const view = start.upmix.endpoints.find((endpoint) => endpoint.plugin !== null)!;
    const input = $(`.pd-upmix-input[data-lane="${view.endpoint}"]`)!;
    // A .vst3 bundle is a folder: the card stands in the folder's slot, the file's is empty.
    const card = input.querySelector(`[data-key="plugin-folder-${view.endpoint}"] .pd-card`)!;
    expect(card.textContent).toBe(view.plugin!.name);
    expect(card.getAttribute('title')).toBe(view.plugin!.path);
    expect(input.querySelector(`[data-key="plugin-file-${view.endpoint}"] .pd-card`)).toBeNull();
    // The path is not printed; a screen reader hears it with the slot.
    expect(seen(input).includes(view.plugin!.path)).toBe(false);
    const slot = input.querySelector(`[data-key="plugin-folder-${view.endpoint}"]`)!;
    const said = (slot.getAttribute('aria-describedby') ?? '').split(' ').map((id) => document.getElementById(id)?.textContent);
    expect(said).toContain(view.plugin!.path);
    const plate = input.querySelector('.pd-spec')!;
    const rows = [...plate.querySelectorAll('.pd-spec-row')].map((row) => row.querySelector('dt')?.textContent);
    expect(rows).toEqual(expect.arrayContaining([ko.upmix.pluginVendor, ko.upmix.pluginFormat]));
    expect(plate.textContent).toContain(view.plugin!.vendor);
    expect(sent).toEqual([]);
  });

  it('an empty plugin slot says what it wants only to a screen reader', () => {
    const start = fixture('ready-upmix');
    start.upmix.endpoints[1]!.plugin = null;
    mounted(start, 'upmix');
    const input = $('.pd-upmix-input[data-lane="1"]')!;
    expect(input.querySelectorAll('.pd-slot:not(.is-loaded)')).toHaveLength(2);
    expect(seen(input).includes(ko.upmix.needsPlugin)).toBe(false);
    expect(seen(input).includes(ko.upmix.noPlugin)).toBe(false);
    const slot = input.querySelector('[data-key="plugin-file-1"]')!;
    expect(document.getElementById(slot.getAttribute('aria-describedby') ?? '')?.textContent).toBe(ko.upmix.needsPlugin);
  });
});

describe('a key shows its state by its own light (법칙 3)', () => {
  it('a capturing lane’s key is lit in the lane’s colour, and no state word is printed by it', () => {
    mounted(fixture('running-apps'), 'status');
    const key = $<HTMLButtonElement>('[data-key="capture-app-0"]')!;
    expect(key.classList.contains('is-lit')).toBe(true);
    expect(key.classList.contains('is-off')).toBe(false);
    expect(key.getAttribute('style')).toContain('--pd-lane-0');
    // It cannot be pressed while the engine runs, but it keeps its light.
    expect(key.disabled).toBe(true);
    const lane = $('.pd-ch[data-lane="0"]')!;
    expect(seen(lane).includes(ko.entrance.stateCapturing)).toBe(false);
    expect(seen(key)).toContain('game.exe');
    // A screen reader still hears the state with the key.
    expect(key.textContent).toContain(ko.entrance.stateCapturing);
  });

  it('a lane waiting for its app blinks, and one without an app or with the engine off is dark', () => {
    mounted(fixture('running-apps-waiting'), 'status');
    const waiting = $('[data-key="capture-app-0"]')!;
    expect(waiting.classList.contains('is-wait')).toBe(true);
    expect(waiting.classList.contains('is-lit')).toBe(false);
    expect(seen($('.pd-ch[data-lane="0"]')!).includes(ko.entrance.stateWaiting)).toBe(false);
    expect($('[data-key="capture-app-1"]')!.className).not.toMatch(/is-(lit|wait)/);
    root.replaceChildren();
    // Off, the app is open but nothing is taken: the ring is dark.
    mounted(fixture('ready-apps'), 'status');
    expect($('[data-key="capture-app-0"]')!.className).not.toMatch(/is-(lit|wait)/);
    expect(seen($('.pd-ch[data-lane="0"]')!).includes(ko.entrance.stateRunning)).toBe(false);
  });

  it('the profile’s kind keys print no "in use" by a dark key: the slide switch shows it', () => {
    mounted(fixture('ready-speakers'), 'profile');
    $('[data-key="profile-tab-headphones"]')!.click();
    const speakers = $('[data-key="profile-tab-speakers"]')!;
    expect(speakers.classList.contains('is-lit')).toBe(false);
    expect(seen(speakers).includes(ko.profile.inUse)).toBe(false);
    expect(speakers.textContent).toContain(ko.profile.inUse);
  });

  it('the discard output in use keeps its light while the engine runs, its keys locked', () => {
    mounted(fixture('running-apps'), 'settings');
    const start = fixture('running-apps');
    const list = $('[data-region="discard-list"]')!;
    const keys = [...list.querySelectorAll<HTMLButtonElement>('button')];
    expect(keys.every((key) => key.disabled)).toBe(true);
    const lit = keys.filter((key) => key.classList.contains('is-lit'));
    expect(lit).toHaveLength(1);
    expect(lit[0]!.textContent).toContain(start.app_capture!.discard!.name);
    expect(seen(list).includes(ko.status.currentOutput)).toBe(false);
  });
});

describe('a file goes in a card slot (법칙 2)', () => {
  it('the correction’s card carries the file’s name, and its eject key sends clear_correction', () => {
    const start = fixture('ready-speakers');
    const { sent } = mounted(start, 'profile');
    const correction = start.profile.speakers.correction!;
    const group = $('[data-group="pd-correction"]')!;
    const slot = group.querySelector<HTMLButtonElement>('[data-key="correction-import"]')!;
    expect(slot.classList.contains('is-loaded')).toBe(true);
    expect(slot.getAttribute('data-intent')).toBe('import_correction');
    expect(slot.querySelector('.pd-card-label')?.textContent).toBe('secs_mixed.yml');
    expect(slot.querySelector('.pd-card')?.getAttribute('title')).toBe(correction.path);
    // The title stays on the spec plate; the long path and the instructions are not printed.
    expect(group.querySelector('.pd-spec')?.textContent).toContain(correction.title);
    const printed = seen(group);
    expect(printed.includes(correction.path)).toBe(false);
    expect(printed.includes(ko.profile.correctionBody)).toBe(false);
    expect(slot.getAttribute('aria-describedby')?.split(' ')).toContain('correction-about');
    expect(document.getElementById('correction-about')?.textContent).toBe(ko.profile.correctionBody);
    const eject = group.querySelector<HTMLButtonElement>('[data-key="correction-clear"]')!;
    expect(eject.classList.contains('pd-mkey-key')).toBe(true);
    eject.click();
    expect(sent).toEqual([{ kind: 'clear_correction' }]);
  });

  it('an empty correction slot opens the same picker and sends import_correction; its LED blinks while it is sent', async () => {
    const start = fixture('ready');
    let asked: [string, { name: string; extensions: string[] }] | null = null;
    const sent: Intent[] = [];
    const mock = createMockApi(start);
    const api: Api = {
      ...mock,
      // The shell's file picker the old key called, with the same dialog and filter.
      shell: {
        ...mock.shell,
        pickFile: async (title, filter) => {
          asked = [title, filter];
          return 'C:\\x\\room.yml';
        },
      },
      dispatch: (intent) => {
        sent.push(intent);
        return new Promise<void>(() => {});
      },
    };
    const app = mount(root, api, { tab: 'profile' });
    app.draw(start);
    const slot = $<HTMLButtonElement>('[data-key="correction-import"]')!;
    expect(slot.classList.contains('is-loaded')).toBe(false);
    expect($('[data-key="correction-clear"]')).toBeNull();
    slot.click();
    await tick();
    await tick();
    expect(asked).toEqual([ko.profile.correctionDialog, { name: ko.profile.correctionFilter, extensions: ['yml', 'yaml'] }]);
    expect(sent).toEqual([{ kind: 'import_correction', value: 'C:\\x\\room.yml' }]);
    const busy = $('[data-key="correction-import"]')!;
    expect(busy.getAttribute('aria-busy')).toBe('true');
    expect(busy.closest('.pd-slotpart')?.querySelector('.pd-slot-led')?.classList.contains('is-busy')).toBe(true);
    expect(seen(busy.closest('[data-group="pd-correction"]')!).includes(ko.profile.importing)).toBe(false);
  });

  it('the BRIR set stands as a card in the slot of its kind: a folder’s, or a WAV file’s', () => {
    mounted(fixture('ready-headphones'), 'profile');
    expect($('[data-key="brir-folder"] .pd-card-label')?.textContent).toBe('living-room');
    expect($('[data-key="brir-file"] .pd-card')).toBeNull();
    root.replaceChildren();
    mounted(fixture('ready-headphones-24-1-10'), 'profile');
    expect($('[data-key="brir-file"] .pd-card-label')?.textContent).toBe('24.1.10.wav');
    expect($('[data-key="brir-folder"] .pd-card')).toBeNull();
    // profile.toml decides: the slots are shut.
    root.replaceChildren();
    mounted(fixture('ready-profile-file'), 'profile');
    $('[data-key="profile-tab-headphones"]')?.click();
    expect($$<HTMLButtonElement>('.pd-slot').every((slot) => slot.disabled)).toBe(true);
  });
});

describe('the per-app entrance is a routing matrix', () => {
  it('lights exactly the crossing of each rule, in its input’s colour', () => {
    const start = fixture('ready');
    mounted(start, 'settings');
    const matrix = $('.pd-matrix')!;
    const lit = [...matrix.querySelectorAll('.pd-key.is-lit')].map((key) => key.getAttribute('data-key')).sort();
    const rows = [
      ...start.routing.apps.map((app) => [app.exe, app.rule] as const),
      ...start.routing.rules.filter((rule) => !start.routing.apps.some((app) => app.exe === rule.exe)).map((rule) => [rule.exe, rule.entrance] as const),
    ];
    expect(lit).toEqual(rows.filter(([, rule]) => rule !== null).map(([exe, rule]) => `routing-rule-${exe}-${rule}`).sort());
    const lanes = ['main', 'music', 'pass'];
    for (const key of matrix.querySelectorAll('.pd-key.is-lit')) {
      const entrance = key.getAttribute('data-key')!.split('-').at(-1)!;
      expect(key.getAttribute('style')).toContain(`--pd-lane-${lanes.indexOf(entrance)}`);
    }
    // Every row has a key per input, the input names printed once at the head.
    expect(matrix.querySelectorAll('thead th')).toHaveLength(3);
    expect(matrix.querySelectorAll('[data-key^="routing-rule-"] .pd-key-label')).toHaveLength(0);
    expect(matrix.querySelectorAll('[data-key^="routing-rule-"]')).toHaveLength(rows.length * 3);
  });

  it('pressing the lit key clears the rule, another moves it, and a row-end key restores or removes', () => {
    const start = fixture('ready');
    const { sent } = mounted(start, 'settings');
    $('[data-key="routing-rule-applemusic.exe-music"]')!.click();
    expect(sent).toEqual([{ kind: 'set_app_rule', value: { exe: 'applemusic.exe', entrance: null } }]);
    root.replaceChildren();
    const again = mounted(start, 'settings');
    $('[data-key="routing-rule-foobar2000.exe-main"]')!.click();
    expect(again.sent).toEqual([{ kind: 'set_app_rule', value: { exe: 'foobar2000.exe', entrance: 'main' } }]);
    // Changed from the factory's: restore. The user's own: remove. As shipped: no key.
    expect($('[data-key="routing-remove-kodi.exe"]')?.textContent).toContain(ko.settings.routingRestore);
    expect($('[data-key="routing-remove-player.exe"]')?.textContent).toContain(ko.settings.routingRemove);
    expect($('[data-key="routing-remove-foobar2000.exe"]')).toBeNull();
  });

  it('prints the factory’s rules, tapes the names this PC gives, and dims the input an app plays into now', () => {
    mounted(fixture('ready'), 'settings');
    const name = (exe: string) => $(`[data-key="routing-rule-${exe}-main"]`)!.closest('tr')!.querySelector('th')!;
    expect(name('foobar2000.exe').querySelector('.pd-silk')).not.toBeNull();
    expect(name('player.exe').querySelector('.pd-tape')).not.toBeNull();
    expect(name('browser.exe').querySelector('.pd-tape')?.textContent).toBe('Browser');
    // "Default" is seen, not read: only a screen reader hears it.
    expect(seen(name('foobar2000.exe')).includes(ko.settings.routingBuiltIn)).toBe(false);
    expect(name('foobar2000.exe').textContent).toContain(ko.settings.routingBuiltIn);
    // Apple Music is sent to music but plays in the game input now.
    expect($('[data-key="routing-rule-applemusic.exe-main"]')!.classList.contains('is-dim')).toBe(true);
    expect($('[data-key="routing-rule-applemusic.exe-music"]')!.classList.contains('is-lit')).toBe(true);
    // The explanation is the group's description, not a printed paragraph.
    const group = $('[data-group="pd-routing"]')!;
    expect(group.getAttribute('aria-describedby')).toBe('routing-hint');
    expect(seen(group).includes(document.getElementById('routing-hint')!.textContent!)).toBe(false);
  });

  it('where this PC cannot route apps, the DIP switch carries a numbered mark and the sentence is on the legend plate', () => {
    mounted(fixture('ready-routing-unavailable'), 'settings');
    const sentence = ko.settings.routingUnavailable;
    const toggle = $<HTMLButtonElement>('[data-key="routing-enabled"]')!;
    expect(toggle.disabled).toBe(true);
    const mark = toggle.parentElement!.querySelector('.pd-mark:not([hidden])')!;
    expect(mark.querySelector('.pd-mark-n')?.textContent).toBe('1');
    expect(toggle.getAttribute('aria-describedby')?.split(' ')).toContain('routing-unavailable');
    expect(document.getElementById('routing-unavailable')?.textContent).toBe(sentence);
    const plate = $('#pd-settings-notes, [data-group="pd-settings-notes"]')!;
    expect(plate.hidden).toBe(false);
    expect(plate.textContent).toContain(sentence);
    expect(seen($('[data-group="pd-routing"]')!).includes(sentence)).toBe(false);
    root.replaceChildren();
    mounted(fixture('ready'), 'settings');
    expect($('[data-group="pd-settings-notes"]')!.hidden).toBe(true);
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

/** How many segments are lit in each cell of a readout's digits. */
function litPerCell(readout: Element): number[] {
  const polygons = [...readout.querySelectorAll('.pd-seg polygon')];
  const cells: number[] = [];
  for (let at = 0; at < polygons.length; at += 7) {
    cells.push(polygons.slice(at, at + 7).filter((polygon) => polygon.classList.contains('is-on')).length);
  }
  return cells;
}

describe('a switch the running engine holds shows a lit lock lamp, not a sentence (법칙 3)', () => {
  it('lights the lamps by the entrance, the output path and the discard output while running', () => {
    mounted(fixture('running-apps'), 'settings');
    const lamps = $$('.pd-lock');
    expect(lamps).toHaveLength(3);
    expect(lamps.every((lamp) => lamp.classList.contains('is-lit'))).toBe(true);
    const entrance = $('[data-group="pd-entrance"]')!;
    expect(entrance.querySelector('.pd-slide')?.classList.contains('is-off')).toBe(true);
    // The sentence saying what to turn off is on the legend plate, not printed by the switch; a screen reader hears it at the switch.
    expect(seen(entrance).includes(ko.entrance.locked)).toBe(false);
    expect(seen($('[data-group="pd-settings-notes"]')!)).toContain(ko.entrance.locked);
    expect(entrance.querySelector('.pd-slide')?.getAttribute('aria-describedby')?.split(' ')).toContain('entrance-locked');
    expect(document.getElementById('entrance-locked')?.textContent).toBe(ko.entrance.locked);
    expect(seen($('[data-group="pd-settings-notes"]')!)).toContain(ko.settings.outputPathLocked);
  });

  it('keeps the lamps dark while the engine is off, and the plate without their sentences', () => {
    mounted(fixture('ready-apps'), 'settings');
    expect($$('.pd-lock').length).toBeGreaterThan(0);
    expect($$('.pd-lock.is-lit')).toHaveLength(0);
    expect($('[data-group="pd-entrance"] .pd-slide')?.classList.contains('is-off')).toBe(false);
    expect(root.textContent).not.toContain(ko.entrance.locked);
    expect(root.textContent).not.toContain(ko.settings.outputPathLocked);
  });

  it('locks the app keys on the status face the same way, and only with the app-sound entrance', () => {
    mounted(fixture('running-apps'), 'status');
    const box = $('[data-group="pd-lane-hints"]')!;
    expect(box.querySelector('.pd-lock.is-lit')).not.toBeNull();
    expect(seen(box).includes(ko.entrance.appsLocked)).toBe(false);
    expect(seen($('[data-group="pd-status-notes"]')!)).toContain(ko.entrance.appsLocked);
    root.replaceChildren();
    mounted(fixture('running'), 'status');
    expect($('.pd-lock')).toBeNull();
    expect(root.textContent).not.toContain(ko.entrance.appsLocked);
  });
});

describe('the DIP legend prints what each position does now (법칙 5)', () => {
  it('says the on sentence under an on lever and the off sentence under an off lever', () => {
    const start = fixture('ready');
    const { app } = mounted(start, 'settings');
    const lines = () => [...$('[data-group="pd-options"] .pd-dip-legend')!.querySelectorAll('li')].map((line) => line.textContent ?? '');
    const levers = () => $$('[data-group="pd-options"] .pd-dip-row').map((lever) => lever.getAttribute('aria-pressed'));
    expect(levers()).toEqual(['false', 'true', 'true']);
    const on = lines();
    expect(on).toHaveLength(3);
    // The plain browser has no autostart: the legend says so.
    expect(on[0]).toContain(ko.settings.autostartUnavailable);
    expect(on[1]).toContain(ko.settings.routingHint(ko.endpoints[2]!.name, ko.endpoints[0]!.name));
    expect(on[2]).toContain(ko.settings.asioHint(ko.settings.asioDevice));
    expect(on[1]).not.toContain(ko.settings.routingOff);
    app.draw({ ...start, routing: { ...start.routing, enabled: false }, asio: { ...start.asio!, enabled: false } });
    expect(levers()).toEqual(['false', 'false', 'false']);
    const off = lines();
    expect(off[1]).toContain(ko.settings.routingOff);
    expect(off[2]).toContain(ko.settings.asioOff);
    expect(off[2]).not.toContain(ko.settings.asioHint(ko.settings.asioDevice));
    // The lever is described by its own line.
    expect($('[data-key="asio-enabled"]')?.getAttribute('aria-describedby')?.split(' ')).toContain('asio-hint');
    expect(document.getElementById('asio-hint')?.textContent).toBe(ko.settings.asioOff);
  });

  it('the exclusive switch of the app-sound entrance prints its position alone', () => {
    const start = fixture('ready-apps');
    const { app } = mounted(start, 'settings');
    const line = () => document.getElementById('app-capture-exclusive-hint')?.textContent;
    expect(start.app_capture!.exclusive).toBe(false);
    expect(line()).toBe(ko.entrance.exclusiveOff);
    expect(root.textContent).not.toContain(ko.entrance.exclusiveOn);
    app.draw({ ...start, app_capture: { ...start.app_capture!, exclusive: true } });
    expect(line()).toBe(ko.entrance.exclusiveOn);
    expect(root.textContent).not.toContain(ko.entrance.exclusiveOff);
  });
});

describe('an input\u2019s format is parts (법칙 1)', () => {
  it('puts the channels and rate on readouts and the kind on a tag, and says the sentence to a screen reader', () => {
    const start = fixture('running');
    mounted(start, 'status');
    const first = $('.pd-ch[data-lane="0"]')!;
    const status = start.engine!.endpoints[0]!;
    const readouts = [...first.querySelectorAll('.pd-fmtparts .pd-readout')];
    expect(readouts).toHaveLength(2);
    // 8 channels: the last cell of a two-cell readout shows an 8, every segment lit.
    expect(litPerCell(readouts[0]!)).toEqual([0, 7]);
    // 48 kHz: a blank, a 4 (four segments) and an 8.
    expect(litPerCell(readouts[1]!)).toEqual([0, 4, 7]);
    expect(readouts[0]!.querySelector('.pd-unit')?.textContent).toBe(ko.pedal.channelsUnit);
    expect(readouts[1]!.querySelector('.pd-unit')?.textContent).toBe('kHz');
    expect(first.querySelector('.pd-fmt-kind')?.textContent).toBe('PCM \u00b7 float32');
    expect(first.querySelector('.pd-fmtparts > .pd-sr')?.textContent).toBe(describeFormat(status.format));
    // The sentence is not printed as a line.
    expect(seen(first).includes(describeFormat(status.format))).toBe(false);
  });

  it('leaves the readouts dark with no input', () => {
    const start = fixture('running');
    const { app } = mounted(start, 'status');
    const engine = start.engine!;
    app.updateMeters({ ...engine, endpoints: engine.endpoints.map((status) => ({ ...status, format: null })) as EngineStatus['endpoints'] });
    // The three driver inputs; the ASIO input has no format readouts.
    const dark = $$('.pd-ch[data-lane] .pd-fmtparts');
    expect(dark).toHaveLength(3);
    for (const parts of dark) {
      expect(parts.querySelector('.pd-fmt-kind')?.textContent).toBe(ko.status.noInput);
      // A dash lights one segment per readout, nothing else.
      for (const readout of parts.querySelectorAll('.pd-readout')) expect(litPerCell(readout).reduce((sum, lit) => sum + lit, 0)).toBe(1);
    }
  });

  it('prints an app-sound lane\u2019s name on the heading and its app on the key alone', () => {
    const start = fixture('running-apps');
    mounted(start, 'status');
    const first = $('.pd-ch[data-lane="0"]')!;
    expect(seen(first.querySelector('.pd-ep')!)).toBe(ko.entrance.lanes[0]);
    const name = start.app_capture!.main!.display_name;
    expect(seen(first).split(name).length - 1).toBe(1);
    expect(first.querySelector('[data-key]')?.textContent).toContain(name);
  });
});

describe('the manual key prints what the parts are for (법칙 5)', () => {
  it('keeps every sentence of the manual off the faces until the key is lit, and still describes the parts', () => {
    mounted(fixture('running'), 'settings');
    const room = $('.pd-room')!;
    const manualKey = $<HTMLButtonElement>('[data-key="manual"]')!;
    expect(room.dataset.manual).toBe('off');
    expect(manualKey.getAttribute('aria-pressed')).toBe('false');
    expect(manualKey.classList.contains('is-lit')).toBe(false);
    // The sentences are in the document for a screen reader, described from their parts.
    const hint = $('#language-hint')!;
    expect(hint.classList.contains('pd-manual')).toBe(true);
    expect(hint.textContent).toBe(ko.settings.languageHint);
    expect($('[aria-describedby~="language-hint"]')).not.toBeNull();

    manualKey.click();
    expect($('.pd-room')!.dataset.manual).toBe('on');
    const lit = $<HTMLButtonElement>('[data-key="manual"]')!;
    expect(lit.getAttribute('aria-pressed')).toBe('true');
    expect(lit.classList.contains('is-lit')).toBe(true);
    lit.click();
    expect($('.pd-room')!.dataset.manual).toBe('off');
  });

  it('never puts a lock, a requirement or an error in the manual', () => {
    for (const [name, tab] of [
      ['running', 'settings'],
      ['ready', 'profile'],
      ['running-upmix', 'upmix'],
      ['faulted', 'status'],
    ] as const) {
      mounted(fixture(name), tab);
      // The legend plate and the bands are status: printed whatever the key says.
      for (const part of $$('.pd-noteplate, [data-region="error-band"], #entrance-blocked')) {
        expect(part.closest('.pd-manual'), `${name}: ${part.className}`).toBeNull();
      }
      root.replaceChildren();
    }
  });

  it('stands on every face that has parts, and is off after quitting', () => {
    for (const tab of TABS) {
      mounted(fixture('ready'), tab);
      expect($<HTMLButtonElement>('[data-key="manual"]')!.disabled, tab).toBe(false);
      root.replaceChildren();
    }
    mounted(fixture('quit'));
    expect($<HTMLButtonElement>('[data-key="manual"]')!.disabled).toBe(true);
    // Setup's faces print all they have: nothing there is the manual's.
    root.replaceChildren();
    mounted(fixture('setup-welcome'));
    expect($<HTMLButtonElement>('[data-key="manual"]')!.disabled).toBe(true);
    expect($('.pd-manual')).toBeNull();
  });

  it('gives two parts with the same note one number and one row of the legend', () => {
    // With the app-sound entrance running, the output path and the discard output are locked by the same sentence.
    mounted(fixture('running-apps'), 'settings');
    const rows = $$('.pd-noteplate .pd-noterow').filter((row) => !row.hidden);
    const sentences = rows.map((row) => row.querySelector('.pd-noterow-text')!.textContent);
    expect(new Set(sentences).size).toBe(sentences.length);
    expect(rows.map((row) => row.querySelector('.pd-mark-n')!.textContent)).toEqual(rows.map((_, index) => String(index + 1)));
    const numberAt = (group: string) => $(`[data-group="${group}"] .pd-lockline .pd-mark-n`)!.textContent;
    expect(numberAt('pd-discard')).toBe(numberAt('pd-output-path'));
  });
});
