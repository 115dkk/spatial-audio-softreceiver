// The console skin (ADR 0041): every fixture draws in both languages on every
// page its phase has, the hooks every skin shares are where the gallery looks
// for them, the controls send what the standard screen's send, and the
// command palette opens, runs and closes from the keyboard.

import { beforeEach, describe, expect, it } from 'vitest';
import type { Api } from '../src/api';
import type { Intent, Language, Snapshot } from '../src/bindings';
import { fixture, fixtureNames } from '../src/fixtures';
import { createMockApi } from '../src/mock';
import type { Tab } from '../src/screens/common';
import { mount } from '../src/skins/console';
import { en, ko } from '../src/text';

const HANGUL = /[ᄀ-ᇿ㄰-㆏가-힯]/;
const TABS: readonly Tab[] = ['status', 'profile', 'upmix', 'settings'];

let root: HTMLElement;

beforeEach(() => {
  root = document.createElement('div');
  document.body.replaceChildren(root);
});

const tick = () => new Promise((resolve) => setTimeout(resolve, 0));

function mounted(start: Snapshot, tab?: Tab, dispatch?: Api['dispatch']) {
  const sent: Intent[] = [];
  const api: Api = {
    ...createMockApi(start),
    dispatch: dispatch ?? (async (intent) => void sent.push(intent)),
  };
  const app = mount(root, api, tab === undefined ? {} : { tab });
  app.draw(start);
  return { app, sent };
}

const $ = <T extends Element = HTMLElement>(selector: string) => root.querySelector<T>(selector);
const $$ = <T extends Element = HTMLElement>(selector: string) => [...root.querySelectorAll<T>(selector)];

/** Everything a person reads or hears: the text and the spoken or shown attributes. */
function words(element: Element): string {
  const attributes = [...element.querySelectorAll('[aria-label], [title], [placeholder]')].flatMap((node) =>
    ['aria-label', 'title', 'placeholder'].map((name) => node.getAttribute(name) ?? ''),
  );
  return [element.textContent ?? '', ...attributes].join('\n');
}

function key(init: KeyboardEventInit, target: EventTarget = document) {
  target.dispatchEvent(new KeyboardEvent('keydown', { bubbles: true, cancelable: true, ...init }));
}

describe('every fixture', () => {
  for (const name of fixtureNames) {
    for (const language of ['ko', 'en'] as Language[]) {
      it(`draws ${name} (${language}) on every page its phase has`, () => {
        const start = fixture(name, language);
        const kind = start.phase.kind;
        const tabs: (Tab | undefined)[] = kind === 'setup' || kind === 'quit' ? [undefined] : [...TABS];
        for (const tab of tabs) {
          root = document.createElement('div');
          document.body.replaceChildren(root);
          const { app } = mounted(start, tab);
          if (kind === 'running' && start.engine !== null) app.updateMeters(start.engine);
          expect(root.querySelector('.cn-app')).not.toBeNull();
          expect(root.querySelector('[data-region="health"]'), `${name} ${tab}`).not.toBeNull();
          if (language === 'en') {
            const found = words(root).match(HANGUL);
            expect(found, `${name} ${tab ?? kind}: Hangul ${found?.[0]} on an English screen`).toBeNull();
          }
        }
      });
    }
  }
});

describe('the hooks every skin shares (ADR 0041)', () => {
  it('puts the health line first, then the error band', () => {
    mounted(fixture('ready-start-failed'), 'status');
    const content = $('.cn-content')!;
    expect(content.firstElementChild?.getAttribute('data-region')).toBe('health');
    const band = $('[data-region="error-band"]');
    expect(band?.getAttribute('role')).toBe('alert');
    // The band's retry is the power button already on screen: one control, not two.
    expect($$('[data-intent="enable"]')).toHaveLength(1);
  });

  it('has the power button, the output list and its toggle on the overview', () => {
    mounted(fixture('ready'), 'status');
    expect($('[data-key="power"]')?.classList.contains('is-ink')).toBe(true);
    expect($('[data-region="output-list"]')).toBeNull();
    $('[data-key="outputs-toggle"]')!.click();
    expect($$('[data-region="output-list"] button').length).toBeGreaterThan(0);
  });

  it('has the install control in the driver step', () => {
    mounted(fixture('setup-driver-not-installed'));
    expect($('[data-key="install"]')?.dataset.intent).toBe('install_driver');
    expect($('[data-key="power"]')).toBeNull();
  });

  it('opens each output kind from its tab', () => {
    mounted(fixture('ready'), 'profile');
    expect($('[data-region="profile-speakers"]')).not.toBeNull();
    $('[data-key="profile-tab-headphones"]')!.click();
    expect($('[data-region="profile-headphones"]')).not.toBeNull();
    $('[data-key="profile-tab-speakers"]')!.click();
    expect($('[data-region="profile-speakers"]')).not.toBeNull();
  });

  it('shows a refused diagnostic recording in an error band', async () => {
    mounted(fixture('ready'), 'settings', async () => {
      throw { message: 'refused', next: 'none' };
    });
    $('[data-key="record"]')!.click();
    await tick();
    const band = $('[data-region="error-band"]');
    expect(band?.getAttribute('role')).toBe('alert');
    expect(band?.textContent).toContain('refused');
  });
});

describe('sending', () => {
  it('turns the engine on, busy while it goes', async () => {
    let finish: () => void = () => undefined;
    const sent: Intent[] = [];
    mounted(fixture('ready'), 'status', (intent) => {
      sent.push(intent);
      return new Promise<void>((resolve) => (finish = resolve));
    });
    $('[data-key="power"]')!.click();
    expect(sent).toEqual([{ kind: 'enable' }]);
    expect($('[data-key="power"]')?.getAttribute('aria-busy')).toBe('true');
    finish();
  });

  it('chooses an output from the list', async () => {
    const start = fixture('ready');
    const { sent } = mounted(start, 'status');
    $('[data-key="outputs-toggle"]')!.click();
    const other = start.outputs.findIndex((output) => output.id !== start.chosen_output?.id);
    $(`[data-key="status-output-${other}"]`)!.click();
    await tick();
    expect(sent).toEqual([{ kind: 'choose_output', value: start.outputs[other]!.id }]);
  });

  it('switches the output kind', async () => {
    const { sent } = mounted(fixture('ready-headphones'), 'profile');
    $('[data-key="profile-tab-speakers"]')!.click();
    $('[data-key="use-speakers"]')!.click();
    await tick();
    expect(sent).toEqual([{ kind: 'set_profile_kind', value: 'speakers' }]);
  });

  it('sets the gain when the slider is let go', async () => {
    const { sent } = mounted(fixture('ready'), 'profile');
    const slider = $<HTMLInputElement>('[data-key="gain"]')!;
    slider.value = '-4.5';
    slider.dispatchEvent(new Event('input'));
    slider.dispatchEvent(new Event('change'));
    await tick();
    expect(sent).toEqual([{ kind: 'set_gain', value: -4.5 }]);
  });

  it('chooses a skin, showing the one in use', async () => {
    const { sent } = mounted({ ...fixture('ready'), skin: 'console' }, 'settings');
    const select = $<HTMLSelectElement>('[data-key="skin"]')!;
    expect(select.querySelector('option[selected]')?.getAttribute('value')).toBe('console');
    select.value = 'standard';
    select.dispatchEvent(new Event('change'));
    await tick();
    expect(sent).toEqual([{ kind: 'set_skin', value: 'standard' }]);
  });
});

describe('the command palette (law 5)', () => {
  const layer = () => document.querySelector<HTMLElement>('.cn-palette-layer');
  const input = () => document.querySelector<HTMLInputElement>('.cn-palette-input');

  it('opens on Ctrl+K, lists the power command and runs it with Enter', async () => {
    const { sent } = mounted(fixture('ready'), 'status');
    key({ key: 'k', ctrlKey: true });
    expect(layer()?.querySelector('[role="dialog"]')?.getAttribute('aria-modal')).toBe('true');
    expect(document.activeElement).toBe(input());
    const power = layer()!.querySelector('[data-command="power"]');
    expect(power?.querySelector('.cn-palette-label')?.textContent).toBe(ko.status.powerOn);

    input()!.value = ko.status.powerOn;
    input()!.dispatchEvent(new Event('input'));
    const first = layer()!.querySelector('[role="option"]');
    expect(first?.getAttribute('data-command')).toBe('power');
    expect(input()!.getAttribute('aria-activedescendant')).toBe(first?.id);
    key({ key: 'Enter' }, input()!);
    await tick();
    expect(sent).toEqual([{ kind: 'enable' }]);
    expect(layer()).toBeNull();
  });

  it('opens from the trigger, moves with the arrows, closes on Escape and gives the focus back', () => {
    mounted(fixture('ready', 'en'), 'status');
    const nav = $('[data-key="nav-profile"]')!;
    nav.focus();
    key({ key: 'k', ctrlKey: true });
    expect(layer()).not.toBeNull();
    const options = () => [...layer()!.querySelectorAll('[role="option"]')];
    expect(options()[0]?.getAttribute('aria-selected')).toBe('true');
    key({ key: 'ArrowDown' }, input()!);
    expect(options()[1]?.getAttribute('aria-selected')).toBe('true');
    expect(layer()!.textContent).toContain(en.console.goTo(en.console.overview));
    key({ key: 'Escape' }, input()!);
    expect(layer()).toBeNull();
    expect(document.activeElement).toBe(nav);

    const trigger = $('[data-key="cn-palette"]')!;
    trigger.focus();
    trigger.click();
    expect(layer()).not.toBeNull();
    key({ key: 'Escape' }, input()!);
    expect(document.activeElement?.getAttribute('data-key')).toBe('cn-palette');
  });

  it('shows a command that cannot run with its reason, and does not run it', async () => {
    const { sent } = mounted(fixture('ready'), 'profile');
    $('[data-key="profile-tab-headphones"]')!.click();
    key({ key: 'k', ctrlKey: true });
    input()!.value = ko.profile.useHeadphones;
    input()!.dispatchEvent(new Event('input'));
    const option = layer()!.querySelector('[data-command="use-headphones"]');
    expect(option?.getAttribute('aria-disabled')).toBe('true');
    expect(option?.textContent).toContain(ko.profile.needsBrir);
    key({ key: 'Enter' }, input()!);
    await tick();
    expect(sent).toEqual([]);
    key({ key: 'Escape' }, input()!);
  });
});

describe('the computer and the status history (law 8)', () => {
  it('names the computer the engine runs on, or this PC when the name is unknown', () => {
    const start = fixture('running');
    mounted(start, 'status');
    expect($('[data-region="computer"]')?.textContent).toContain(start.platform!.computer_name!);
    expect($('[data-region="computer"]')?.textContent).toContain(ko.console.computer);

    root = document.createElement('div');
    document.body.replaceChildren(root);
    mounted({ ...start, platform: { ...start.platform!, computer_name: '' } }, 'status');
    expect($('[data-region="computer"]')?.textContent).toContain(ko.settings.platform);
  });

  it('shows four parts with 90 minutes each, padded on the left, and the uptime share', () => {
    const start = fixture('running');
    mounted(start, 'status');
    const rows = $$('.cn-history-row');
    expect(rows.map((row) => row.dataset.part)).toEqual(['driver', 'engine', 'output', 'plugins']);
    for (const row of rows) expect(row.querySelectorAll('.cn-bar')).toHaveLength(90);
    // Unwatched minutes stay outlines, never filled.
    const minutes = start.health!.minutes;
    const unwatched = minutes.filter((minute) => minute.driver === 'no_record').length;
    expect(rows[0]!.querySelectorAll('.cn-bar.is-no_record')).toHaveLength(90 - minutes.length + unwatched);
    expect(rows[3]!.querySelector('.cn-num')?.textContent).toBe('-');
    expect(rows[0]!.querySelector('.cn-num')?.textContent).toBe('100.0%');
    // One focusable stop per part, not one per bar.
    expect($$('.cn-history [tabindex="0"]')).toHaveLength(4);
  });

  it('lists an open incident as ongoing, newest first', () => {
    mounted(fixture('running-reconnecting'), 'status');
    const rows = $$('.cn-incident-table tbody tr');
    expect(rows[0]?.classList.contains('is-open')).toBe(true);
    expect(rows[0]?.textContent).toContain(ko.console.incidentOpen);
    expect(rows[0]?.textContent).toContain(ko.console.partState(ko.console.device, ko.console.minuteDown));
    expect(rows.at(-1)?.textContent).toContain(ko.console.incidentClosed);
  });

  it('says there was no incident when there was none', () => {
    mounted(fixture('ready', 'en'), 'status');
    expect($('.cn-history')?.textContent).toContain(en.console.incidentsNone);
    expect($('.cn-history-summary')?.textContent).toBe(en.console.historyGood);
  });

  it('reads the minute the arrow keys move to', () => {
    const start = fixture('running-reconnecting');
    mounted(start, 'status');
    const output = $('[data-part="output"]')!;
    const readout = $('.cn-history-readout')!;
    expect(readout.getAttribute('aria-live')).toBe('polite');
    output.focus();
    expect(readout.textContent).toContain(ko.console.checkNow);
    expect(readout.textContent).toContain(ko.console.minuteDown);
    const selected = () => output.querySelector('.cn-bar.is-selected')?.getAttribute('data-slot');
    expect(selected()).toBe('89');
    key({ key: 'ArrowLeft' }, output);
    expect(selected()).toBe('88');
    expect(readout.textContent).toContain(ko.console.minutesAgo(1));
    key({ key: 'Home' }, output);
    expect(selected()).toBe('0');
    expect(readout.textContent).toContain(ko.console.minuteNoRecord);
    key({ key: 'End' }, output);
    key({ key: 'ArrowLeft' }, output);
    key({ key: 'ArrowLeft' }, output);
    key({ key: 'ArrowLeft' }, output);
    const minute = start.health!.minutes.at(-4)!;
    expect(readout.textContent).toContain(ko.console.minutesAgo(3));
    expect(readout.textContent).toContain(
      ko.console.readoutCounts(String(minute.underruns), String(minute.output_recoveries)),
    );
  });
});
