// The front panel skin (ADR 0041, 전면 패널의 법칙): every fixture draws on
// every tab its phase has, in both languages; the hooks the gallery and the
// other skins share are where they should be; and the chassis parts and the
// display's rows send what the standard screen sends.

import { beforeEach, describe, expect, it } from 'vitest';
import type { Api } from '../src/api';
import type { Intent, Language, Snapshot } from '../src/bindings';
import { fixture, fixtureNames } from '../src/fixtures';
import { createMockApi } from '../src/mock';
import type { Tab } from '../src/screens/common';
import { mount } from '../src/skins/panel';
import { en, ko } from '../src/text';
import { ROLES, applyRatio, fit } from '../src/skins/panel/type';

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

/** The panel over a bridge that records what it is sent (or the mock's own answers). */
function mounted(start: Snapshot, tab?: Tab, dispatch?: Api['dispatch']) {
  const sent: Intent[] = [];
  const mock = createMockApi(start);
  const api: Api = { ...mock, dispatch: dispatch ?? (async (intent) => void sent.push(intent)) };
  const app = mount(root, api, tab === undefined ? {} : { tab });
  app.draw(start);
  if (start.phase.kind === 'running' && start.engine !== null) app.updateMeters(start.engine);
  return { app, sent };
}

function key(event: 'keydown' | 'keyup', target: Element, name: string) {
  target.dispatchEvent(new KeyboardEvent(event, { key: name, bubbles: true }));
}

describe('every fixture', () => {
  for (const language of ['ko', 'en'] as Language[]) {
    for (const name of fixtureNames) {
      it(`${name} draws on each tab of its phase (${language})`, () => {
        const start = fixture(name, language);
        const wizard = start.phase.kind === 'setup' || start.phase.kind === 'quit';
        for (const tab of wizard ? [undefined] : TABS) {
          root.replaceChildren();
          mounted(fixture(name, language), tab);
          expect($('.pn-chassis'), `${name} ${tab ?? 'wizard'}`).not.toBeNull();
          expect($('.pn-screen')).not.toBeNull();
          const words = root.textContent ?? '';
          if (language === 'en') expect(words.match(HANGUL)?.[0] ?? null, `${name} ${tab ?? 'wizard'}`).toBeNull();
          else expect(HANGUL.test(words)).toBe(true);
          // Pages carry the display's tabpanel; the wizard and the closing page do not.
          expect($('#pn-page')?.getAttribute('role') ?? null).toBe(wizard ? null : 'tabpanel');
        }
      });
    }
  }
});

describe('the hooks every skin shares (ADR 0041)', () => {
  it('power stands on the chassis in every phase, off in the wizard', () => {
    mounted(fixture('ready'), 'status');
    expect($<HTMLButtonElement>('[data-key="power"]')?.disabled).toBe(false);
    root.replaceChildren();
    mounted(fixture('setup-welcome'));
    expect($<HTMLButtonElement>('[data-key="power"]')?.disabled).toBe(true);
  });

  it('the driver step has install', () => {
    mounted(fixture('setup-driver-not-installed'));
    expect($('[data-key="install"][data-intent="install_driver"]')).not.toBeNull();
  });

  it('outputs-toggle opens the output list, a button per output', () => {
    const start = fixture('ready');
    mounted(start, 'status');
    expect($('[data-region="output-list"]')).toBeNull();
    $('[data-key="outputs-toggle"]')!.click();
    const list = $('[data-region="output-list"]');
    expect(list).not.toBeNull();
    expect(list!.querySelectorAll('button')).toHaveLength(start.outputs.length);
    expect($('[data-key="outputs-toggle"]')!.getAttribute('aria-expanded')).toBe('true');
  });

  it('the profile tabs open their regions', () => {
    mounted(fixture('ready'), 'profile');
    expect($('[data-region="profile-speakers"]')).not.toBeNull();
    $('[data-key="profile-tab-headphones"]')!.click();
    expect($('[data-region="profile-headphones"]')).not.toBeNull();
    expect($('[data-region="profile-speakers"]')).toBeNull();
    $('[data-key="profile-tab-speakers"]')!.click();
    expect($('[data-region="profile-speakers"]')).not.toBeNull();
  });

  it('a refused record shows an error band', async () => {
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
    // A refusal can be closed.
    expect(band?.querySelector('button')?.textContent).toContain(ko.banner.dismiss);
  });

  it("the snapshot's error stands as a band with its action", () => {
    mounted(fixture('faulted'), 'status');
    const band = $('[data-region="error-band"]');
    expect(band?.getAttribute('role')).toBe('alert');
    expect(band?.querySelector('[data-intent="recheck"]')).not.toBeNull();
  });
});

describe('the chassis and the display send', () => {
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

  it('a row of the output list sends choose_output', () => {
    const start = fixture('ready');
    const { sent } = mounted(start, 'status');
    $('[data-key="outputs-toggle"]')!.click();
    const other = start.outputs.findIndex((output) => output.id !== start.chosen_output?.id);
    $(`[data-region="output-list"] [data-key="status-output-${other}"]`)!.click();
    expect(sent).toEqual([{ kind: 'choose_output', value: start.outputs[other]!.id }]);
  });

  it('the output kind keys send set_profile_kind', () => {
    const { sent } = mounted(fixture('ready-headphones'), 'status');
    expect($('[data-key="kind-headphones"]')!.getAttribute('aria-checked')).toBe('true');
    $('[data-key="kind-speakers"]')!.click();
    expect(sent).toEqual([{ kind: 'set_profile_kind', value: 'speakers' }]);
  });

  it('headphones without a BRIR set say why instead of sending', () => {
    const { sent } = mounted(fixture('ready'), 'status');
    const headphones = $('[data-key="kind-headphones"]')!;
    expect(headphones.getAttribute('aria-disabled')).toBe('true');
    const why = document.getElementById(headphones.getAttribute('aria-describedby') ?? '');
    expect(why?.textContent).toBe(ko.profile.needsBrir);
    headphones.click();
    expect(sent).toEqual([]);
    expect($('.pn-osd')!.hidden).toBe(false);
    expect($('.pn-osd')!.textContent).toBe(ko.profile.needsBrir);
  });

  it('the encoder turns by keys and sends set_gain once let go', () => {
    const start = fixture('ready-headphones');
    const { sent } = mounted(start, 'status');
    const knob = $('[data-key="gain"]')!;
    expect(knob.getAttribute('role')).toBe('slider');
    key('keydown', knob, 'ArrowUp');
    key('keydown', knob, 'ArrowUp');
    expect(knob.getAttribute('aria-valuetext')).toBe(`${(start.profile.gain_db + 1).toFixed(1)} dB`);
    // The big numeral follows the encoder before anything is sent.
    expect($('.pn-gain-standby')!.textContent).toContain((start.profile.gain_db + 1).toFixed(1).replace('-', '−'));
    expect(sent).toEqual([]);
    key('keyup', knob, 'ArrowUp');
    expect(sent).toEqual([{ kind: 'set_gain', value: start.profile.gain_db + 1 }]);
  });

  it('a skin row sends set_skin', () => {
    const { sent } = mounted({ ...fixture('ready'), skin: 'panel' }, 'settings');
    expect($('[data-key="skin-panel"]')!.getAttribute('aria-checked')).toBe('true');
    $('[data-key="skin-console"]')!.click();
    expect(sent).toEqual([{ kind: 'set_skin', value: 'console' }]);
  });

  it('a point on the map plays the test signal while running', () => {
    const start = fixture('running');
    const { sent } = mounted(start, 'status');
    const first = start.engine!.directions[0]!;
    $(`[data-key="map-direction-${first.id}"]`)!.click();
    expect(sent).toEqual([{ kind: 'test_direction', value: first.id }]);
  });

  it('the page keys are a tablist whose panel is the display', () => {
    mounted(fixture('ready'), 'status');
    const keys = $$('[role="tablist"] [role="tab"][data-key^="nav-"]');
    expect(keys.map((tab) => tab.textContent)).toEqual([ko.nav.status, ko.nav.profile, ko.nav.upmix, ko.nav.settings]);
    keys[2]!.click();
    expect($('#pn-page')!.getAttribute('aria-labelledby')).toBe('pn-tab-upmix');
    expect($('[data-key="nav-upmix"]')!.getAttribute('aria-selected')).toBe('true');
  });

  it('English prints the chassis in English', () => {
    mounted(fixture('ready', 'en'), 'status');
    expect($('#pn-power-print')!.textContent).toBe(en.panel.power);
    expect($$('.pn-lamp .pn-print').map((print) => print.textContent)).toEqual(en.panel.lamps);
  });
});

describe("the display's pixel type on the device grid", () => {
  it.each([1, 1.25, 1.5, 1.75, 2])('every role lands on whole device pixels at %s', (ratio) => {
    for (const role of ROLES) {
      const { family, size, line } = fit(role, ratio);
      const pixels = family.includes('px14') ? 15 : 12;
      // One font pixel is a whole number of device pixels, and so is the line box.
      expect(Math.abs(((size / pixels) * ratio) % 1) < 1e-6 || Math.abs(((size / pixels) * ratio) % 1) > 1 - 1e-6, `${role.name} ${size}`).toBe(true);
      expect(Math.abs(Math.round(line * ratio) - line * ratio)).toBeLessThan(1e-6);
      expect(size).toBeGreaterThanOrEqual(role.min);
    }
  });

  it('keeps the design at 100 % and writes the fits onto the root', () => {
    const body = fit(ROLES.find((role) => role.name === 'body')!, 1);
    expect(body).toEqual({ family: 'var(--pn-px14)', size: 15, line: 24 });
    const host = document.createElement('div');
    applyRatio(host, 1.5);
    expect(host.dataset.dpr).toBe('1.5');
    expect(host.style.getPropertyValue('--pn-fs-body')).toBe('16px');
    expect(host.style.getPropertyValue('--pn-ff-body')).toBe('var(--pn-px11)');
  });
});
