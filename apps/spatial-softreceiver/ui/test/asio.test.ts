// The ASIO input on the screen (ADR 0025): its panel on the settings screen
// and its line on the status screen.

import { beforeEach, describe, expect, it } from 'vitest';
import type { Api } from '../src/api';
import { mountApp } from '../src/app';
import type { AsioStatus, Intent, Language, Snapshot } from '../src/bindings';
import { en as catalogEn, ko as catalogKo } from '../src/catalog';
import { fixture } from '../src/fixtures';
import { createMockApi } from '../src/mock';
import type { Tab } from '../src/screens/common';
import { en, ko } from '../src/text';

const HANGUL = /[ᄀ-ᇿ㄰-㆏ꥠ-꥿가-힯ힰ-퟿ﾠ-ￜ]/u;
const NBSP = ' ';

let root: HTMLElement;

beforeEach(() => {
  root = document.createElement('div');
  document.body.replaceChildren(root);
});

const tick = () => new Promise((resolve) => setTimeout(resolve, 0));

/** A tab over a bridge that records what it is sent. */
function mounted(start: Snapshot, tab: Tab = 'settings', dispatch?: Api['dispatch']) {
  const sent: Intent[] = [];
  const api: Api = {
    ...createMockApi(start),
    dispatch: dispatch ?? (async (intent) => void sent.push(intent)),
  };
  const app = mountApp(root, api, { tab });
  app.draw(start);
  return { app, sent };
}

/** A bridge whose dispatch waits until the test lets it go. */
function held() {
  const sent: Intent[] = [];
  let release: () => void = () => {};
  const dispatch: Api['dispatch'] = (intent) => {
    sent.push(intent);
    return new Promise<void>((resolve) => (release = resolve));
  };
  return { dispatch, sent, release: () => release() };
}

const $ = <T extends Element = HTMLElement>(selector: string) => root.querySelector<T>(selector);
const $$ = <T extends Element = HTMLElement>(selector: string) => [...root.querySelectorAll<T>(selector)];
// happy-dom's `select.value` misreports a selected last option; the attribute
// the screen sets is what a browser selects.
const chosen = (select: HTMLSelectElement | null) => select?.querySelector('option[selected]')?.getAttribute('value') ?? null;
const optionLabels = (select: HTMLSelectElement | null) =>
  [...(select?.querySelectorAll('option') ?? [])].map((option) => option.textContent);

function choose(select: HTMLSelectElement | null, value: string) {
  if (select === null) throw new Error('no select');
  select.value = value;
  select.dispatchEvent(new Event('change'));
}

/** Moves the slider (input) and, when `release`, lets it go (change). */
function slide(slider: HTMLInputElement | null, value: number, release = true) {
  if (slider === null) throw new Error('no slider');
  slider.value = String(value);
  slider.dispatchEvent(new Event('input'));
  if (release) slider.dispatchEvent(new Event('change'));
}

const catalogOf = (language: Language) => (language === 'ko' ? catalogKo : catalogEn);
const textOf = (language: Language) => (language === 'ko' ? ko : en);

describe('the ASIO panel on the settings screen', () => {
  const panelOf = () => $$('.settings > .panel').find((panel) => panel.querySelector('#asio-enabled') !== null) ?? null;
  const checkbox = () => $<HTMLInputElement>('[data-key="asio-enabled"]');
  const layout = () => $<HTMLSelectElement>('[data-key="asio-layout"]');
  const slider = () => $<HTMLInputElement>('[data-key="asio-attenuation"]');
  const readout = () => $('.asio-attenuation .gain-readout')?.textContent ?? null;

  it.each<Language>(['ko', 'en'])('shows the saved switch, layout and attenuation (%s)', (language) => {
    mounted(fixture('running-asio', language));
    const t = textOf(language);
    expect(panelOf()?.querySelector('.panel-title')?.textContent).toBe(t.asio);
    expect(checkbox()?.checked).toBe(true);
    expect(checkbox()?.disabled).toBe(false);
    expect($('label[for="asio-enabled"]')?.textContent).toBe(t.settings.asioEnabled);
    expect(checkbox()?.getAttribute('aria-describedby')).toBe('asio-hint asio-limits');
    expect($('#asio-hint')?.textContent).toBe(t.settings.asioHint('Spatial SoftReceiver'));
    expect($('#asio-limits')?.textContent).toBe(t.settings.asioLimits);

    expect($('label[for="asio-layout"]')?.textContent).toBe(t.settings.asioLayout);
    expect(chosen(layout())).toBe('7.1.4');
    expect(optionLabels(layout())).toEqual(
      (
        [
          ['2.0', 2],
          ['5.1', 6],
          ['7.1', 8],
          ['7.1.4', 12],
          ['9.1.6', 16],
        ] as const
      ).map(([name, channels]) => `${name} (${t.format.channels(channels)})`),
    );
    expect(layout()?.disabled).toBe(false);
    expect($('#asio-layout-hint')?.textContent).toBe(t.settings.asioLayoutHint);

    expect($('label[for="asio-attenuation"]')?.textContent).toBe(t.settings.asioAttenuation);
    expect(slider()?.value).toBe('12');
    expect([slider()?.min, slider()?.max, slider()?.step]).toEqual(['0', '60', '1']);
    expect(slider()?.disabled).toBe(false);
    expect(readout()).toBe('12 dB');
    expect(slider()?.getAttribute('aria-valuetext')).toBe('12 dB');
    expect($('#asio-attenuation-hint')?.textContent).toBe(t.settings.asioAttenuationHint);
  });

  it('names the ASIO device as the DAW lists it, in English on the Korean screen too', () => {
    mounted(fixture('ready'));
    expect(catalogKo.names.product).not.toBe(catalogEn.names.product);
    expect($('#asio-hint')?.textContent).toContain(`ASIO 장치로 ${catalogEn.names.product}를 고르면`);
    expect($('#asio-hint')?.textContent).not.toContain(catalogKo.names.product + '를');
  });

  it('is on, at 7.1.4 and 0 dB, by default', () => {
    mounted(fixture('ready'));
    expect(checkbox()?.checked).toBe(true);
    expect(chosen(layout())).toBe('7.1.4');
    expect(slider()?.value).toBe('0');
    expect(readout()).toBe('0 dB');
  });

  it('switches the input off and on', async () => {
    const { app, sent } = mounted(fixture('running-asio'));
    checkbox()?.click();
    await tick();
    const off = fixture('running-asio');
    off.asio!.enabled = false;
    app.draw(off);
    await tick();
    expect(checkbox()?.checked).toBe(false);
    checkbox()?.click();
    expect(sent).toEqual([
      { kind: 'set_asio_enabled', value: false },
      { kind: 'set_asio_enabled', value: true },
    ]);
  });

  it('sends the layout chosen', () => {
    const { sent } = mounted(fixture('running-asio'));
    choose(layout(), '9.1.6');
    expect(sent).toEqual([{ kind: 'set_asio_layout', value: '9.1.6' }]);
  });

  it('moves the readout with the thumb and sends the attenuation once let go', () => {
    const { sent } = mounted(fixture('running-asio'));
    slide(slider(), 30, false);
    expect(readout()).toBe('30 dB');
    expect(slider()?.getAttribute('aria-valuetext')).toBe('30 dB');
    expect(sent).toEqual([]);
    slide(slider(), 60);
    expect(sent).toEqual([{ kind: 'set_asio_system_attenuation', value: 60 }]);
  });

  it.each<Language>(['ko', 'en'])('says muted at 60 dB (%s)', (language) => {
    const start = fixture('running-asio', language);
    start.asio!.system_attenuation_db = 60;
    mounted(start);
    const muted = textOf(language).status.muted;
    expect(readout()).toBe(muted);
    expect(slider()?.getAttribute('aria-valuetext')).toBe(muted);
    slide(slider(), 59, false);
    expect(readout()).toBe('59 dB');
  });

  it.each<Language>(['ko', 'en'])('locks the layout and the slider while off and keeps their values (%s)', (language) => {
    mounted(fixture('ready-asio-off', language));
    expect(checkbox()?.checked).toBe(false);
    expect(checkbox()?.disabled).toBe(false);
    expect(layout()?.disabled).toBe(true);
    expect(chosen(layout())).toBe('2.0');
    expect(slider()?.disabled).toBe(true);
    expect(slider()?.value).toBe('20');
    expect(readout()).toBe('20 dB');
  });

  it('locks every ASIO control while an intent is in flight, the switch standing where it goes', async () => {
    const bridge = held();
    const { app } = mounted(fixture('running-asio'), 'settings', bridge.dispatch);
    checkbox()?.click();
    expect(bridge.sent).toEqual([{ kind: 'set_asio_enabled', value: false }]);
    expect(checkbox()?.checked).toBe(false);
    expect(checkbox()?.getAttribute('aria-busy')).toBe('true');
    expect([checkbox()?.disabled, layout()?.disabled, slider()?.disabled]).toEqual([true, true, true]);
    bridge.release();
    await tick();
    const off = fixture('running-asio');
    off.asio!.enabled = false;
    app.draw(off);
    await tick();
    expect(checkbox()?.disabled).toBe(false);
    expect(checkbox()?.getAttribute('aria-busy')).toBeNull();
    expect([layout()?.disabled, slider()?.disabled]).toEqual([true, true]);
  });

  it('holds the layout and the attenuation being sent until the snapshot after them lands', async () => {
    const bridge = held();
    const { app } = mounted(fixture('running-asio'), 'settings', bridge.dispatch);
    choose(layout(), '5.1');
    expect(chosen(layout())).toBe('5.1');
    expect(layout()?.getAttribute('aria-busy')).toBe('true');
    expect(slider()?.disabled).toBe(true);
    bridge.release();
    await tick();
    const next = fixture('running-asio');
    next.asio!.layout = '5.1';
    app.draw(next);
    await tick();
    expect(layout()?.disabled).toBe(false);

    slide(slider(), 45);
    expect(bridge.sent.at(-1)).toEqual({ kind: 'set_asio_system_attenuation', value: 45 });
    expect(slider()?.value).toBe('45');
    expect(readout()).toBe('45 dB');
    expect(slider()?.disabled).toBe(true);
    bridge.release();
    await tick();
    next.asio!.system_attenuation_db = 45;
    app.draw(next);
    await tick();
    expect(slider()?.disabled).toBe(false);
    expect(slider()?.value).toBe('45');
  });

  it('follows the mock supervisor, which takes each change while running', async () => {
    const start = fixture('running');
    const api = createMockApi(start);
    const app = mountApp(root, api, { tab: 'settings' });
    app.draw(start);
    await api.dispatch({ kind: 'set_asio_layout', value: '9.1.6' });
    await api.dispatch({ kind: 'set_asio_system_attenuation', value: 60 });
    await api.dispatch({ kind: 'set_asio_enabled', value: false });
    app.draw(await api.snapshot());
    expect(checkbox()?.checked).toBe(false);
    expect(chosen(layout())).toBe('9.1.6');
    expect(readout()).toBe(ko.status.muted);
    expect([layout()?.disabled, slider()?.disabled]).toEqual([true, true]);
  });

  it('has no Hangul in English, in any state', () => {
    for (const name of ['ready', 'running-asio', 'ready-asio-off']) {
      const start = fixture(name, 'en');
      start.asio!.system_attenuation_db = 60;
      mounted(start);
      const panel = panelOf();
      expect(panel?.textContent, name).not.toMatch(HANGUL);
      for (const element of panel?.querySelectorAll('*') ?? []) {
        for (const attr of element.attributes) expect(attr.value, `${name} ${element.tagName}[${attr.name}]`).not.toMatch(HANGUL);
      }
    }
  });
});

describe('the ASIO line on the status screen', () => {
  const line = () => $('.asio-line');
  const state = () => $('.asio-state');
  const client = () => $('.asio-client')?.textContent ?? null;
  /** The facts after the program file that are shown. */
  const details = () => $$('.asio-detail').filter((detail) => !detail.hidden).map((detail) => detail.textContent);
  const warning = () => $('.asio-line .card-warning');
  const fault = () => $('.asio-fault');

  function client48k(patch: Partial<AsioStatus> = {}): AsioStatus {
    return {
      client_pid: 4242,
      client_exe: 'cubase14.exe',
      sample_rate: 48_000,
      buffer_frames: 64,
      channels: 2,
      layout: '2.0',
      underruns: 0,
      generation: 1,
      clock_fault: false,
      fault_reason: null,
      ...patch,
    };
  }

  it('is not there while the engine is off or the input is off', () => {
    for (const name of ['ready', 'ready-asio-off', 'faulted']) {
      mounted(fixture(name), 'status');
      expect(line(), name).toBeNull();
    }
    const off = fixture('running-asio');
    off.asio!.enabled = false;
    mounted(off, 'status');
    expect(line()).toBeNull();
    // The other cards stay as they were.
    expect($$('.endpoint-card')).toHaveLength(3);
  });

  it.each<Language>(['ko', 'en'])('says no DAW is connected while on and none holds it (%s)', (language) => {
    mounted(fixture('running', language), 'status');
    const t = textOf(language);
    expect(line()?.closest('.entrances')).not.toBeNull();
    expect(line()?.querySelector('h3')?.textContent).toBe(t.asio);
    expect(line()?.getAttribute('aria-labelledby')).toBe('asio-line-title');
    expect(client()).toBe(t.status.asioWaiting);
    expect(state()?.className).toBe('asio-state tone-off');
    expect(details()).toEqual([]);
    expect(warning()?.hidden).toBe(true);
    expect(fault()?.hidden).toBe(true);
  });

  it('names the DAW, the rate, the buffer and its length, the layout and the underruns', () => {
    mounted(fixture('running-asio'), 'status');
    expect(client()).toBe('reaper.exe');
    expect(state()?.className).toBe('asio-state tone-on');
    expect(details()).toEqual([`48${NBSP}kHz`, `버퍼 256프레임 (5.3${NBSP}ms)`, '7.1.4 (12채널)', '끊김 0회']);
    expect(warning()?.hidden).toBe(true);
    expect(fault()?.hidden).toBe(true);
  });

  it('speaks English with the English snapshot', () => {
    mounted(fixture('running-asio', 'en'), 'status');
    expect(line()?.querySelector('h3')?.textContent).toBe('ASIO input');
    expect(details()).toEqual([
      `48${NBSP}kHz`,
      `Buffer 256${NBSP}frames (5.3${NBSP}ms)`,
      `7.1.4 (12${NBSP}channels)`,
      'Underruns 0',
    ]);
    expect(line()?.textContent).not.toMatch(HANGUL);
  });

  it.each<Language>(['ko', 'en'])('warns while a stand-in clock drives the DAW (%s)', (language) => {
    mounted(fixture('running-asio-clock-fault', language), 'status');
    expect(state()?.className).toBe('asio-state tone-warning');
    expect(client()).toBe('reaper.exe');
    expect(details()).toEqual(
      language === 'ko'
        ? [`48${NBSP}kHz`, `버퍼 128프레임 (2.7${NBSP}ms)`, '5.1 (6채널)', '끊김 37회']
        : [`48${NBSP}kHz`, `Buffer 128${NBSP}frames (2.7${NBSP}ms)`, `5.1 (6${NBSP}channels)`, 'Underruns 37'],
    );
    expect(warning()?.hidden).toBe(false);
    expect(warning()?.textContent).toBe(textOf(language).status.asioClockFault);
    expect(fault()?.hidden).toBe(true);
  });

  it.each<Language>(['ko', 'en'])('shows the fault reason as the engine wrote it, untranslated (%s)', (language) => {
    mounted(fixture('running-asio-fault', language), 'status');
    expect(state()?.className).toBe('asio-state tone-error');
    expect(client()).toBe(textOf(language).status.asioFailed);
    expect(details()).toEqual([]);
    // clock_fault comes with every fault; the reason says more.
    expect(warning()?.hidden).toBe(true);
    expect(fault()?.hidden).toBe(false);
    expect(fault()?.textContent).toBe('ASIO transport failed: owner_exists');
    // The supervisor's note says it once above, in its own sentence.
    expect($('.notes')?.textContent).toBe(
      catalogOf(language).supervisor.asio_unavailable('ASIO transport failed: owner_exists'),
    );
  });

  it('follows the meters in place: a DAW that comes, counts, loses the clock and goes', () => {
    const start = fixture('running');
    const { app } = mounted(start, 'status');
    const before = line();
    const engine = structuredClone(start.engine!);

    engine.asio = client48k();
    app.updateMeters(engine);
    expect(line()).toBe(before);
    expect(client()).toBe('cubase14.exe');
    expect(details()).toEqual([`48${NBSP}kHz`, `버퍼 64프레임 (1.3${NBSP}ms)`, '2.0 (2채널)', '끊김 0회']);

    engine.asio = client48k({ underruns: 1204, buffer_frames: 2048, clock_fault: true });
    app.updateMeters(engine);
    expect(details()).toEqual([`48${NBSP}kHz`, `버퍼 2,048프레임 (42.7${NBSP}ms)`, '2.0 (2채널)', '끊김 1,204회']);
    expect(state()?.className).toBe('asio-state tone-warning');
    expect(warning()?.hidden).toBe(false);

    engine.asio = client48k({ fault_reason: 'lost', clock_fault: true });
    app.updateMeters(engine);
    expect(client()).toBe(ko.status.asioFailed);
    expect(fault()?.textContent).toBe('lost');
    expect(warning()?.hidden).toBe(true);

    engine.asio = null;
    app.updateMeters(engine);
    expect(line()).toBe(before);
    expect(client()).toBe(ko.status.asioWaiting);
    expect(state()?.className).toBe('asio-state tone-off');
    expect(details()).toEqual([]);
    expect([warning()?.hidden, fault()?.hidden]).toEqual([true, true]);
  });

  it('shows a dash for a rate it has not been told', () => {
    const start = fixture('running');
    start.engine!.asio = client48k({ sample_rate: 0, client_exe: '' });
    mounted(start, 'status');
    expect(client()).toBe('-');
    expect(details()).toEqual(['-', '버퍼 64프레임 (- ms)', '2.0 (2채널)', '끊김 0회']);
  });
});
