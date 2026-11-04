// The app-sound entrance (ADR 0043) in every skin (ADR 0041): the entrance
// choice at the first step, the switch to the other entrance where one cannot
// go on, the discard output step and its empty list, the lanes with their
// pickers on the status page (locked while running), and the entrance, the
// discard output and the exclusive switch in the settings. Each control sends
// what the contract says, through the shared handles, and the English screens
// have no Hangul.

import { beforeEach, describe, expect, it } from 'vitest';
import type { Api } from '../src/api';
import { mountApp } from '../src/app';
import type { Intent, Language, Snapshot } from '../src/bindings';
import { fixture } from '../src/fixtures';
import { createMockApi } from '../src/mock';
import type { Tab } from '../src/screens/common';
import { appSoundBlocked, entranceBlocked, switchTarget } from '../src/screens/entrance';
import type { App } from '../src/skins/types';
import { mount as mountConsole } from '../src/skins/console';
import { mount as mountPedal } from '../src/skins/pedal';
import { en, ko } from '../src/text';

const HANGUL = /[ᄀ-ᇿ㄰-㆏가-힯]/;

type SkinName = 'standard' | 'pedal' | 'console';
const SKINS: readonly SkinName[] = ['standard', 'pedal', 'console'];
const MOUNT: Record<SkinName, (root: HTMLElement, api: Api, options: { tab?: Tab }) => App> = {
  standard: mountApp,
  pedal: mountPedal,
  console: mountConsole,
};

const APP_FIXTURES = [
  'setup-welcome',
  'setup-apps-unsupported',
  'setup-apps-unsupported-old',
  'setup-choose-discard',
  'setup-choose-discard-empty',
  'ready-apps',
  'ready-apps-no-app',
  'ready-apps-not-running',
  'running-apps',
  'running-apps-waiting',
];

let root: HTMLElement;

beforeEach(() => {
  root = document.createElement('div');
  document.body.replaceChildren(root);
});

const tick = () => new Promise((resolve) => setTimeout(resolve, 0));
const $ = <T extends Element = HTMLElement>(selector: string) => root.querySelector<T>(selector);
const $$ = <T extends Element = HTMLElement>(selector: string) => [...root.querySelectorAll<T>(selector)];

function mounted(skin: SkinName, start: Snapshot, tab?: Tab) {
  root = document.createElement('div');
  document.body.replaceChildren(root);
  const sent: Intent[] = [];
  const api: Api = { ...createMockApi(start), dispatch: async (intent) => void sent.push(intent) };
  const app = MOUNT[skin](root, api, tab === undefined ? {} : { tab });
  app.draw(start);
  if (start.phase.kind === 'running' && start.engine !== null) app.updateMeters(start.engine);
  return { app, sent };
}

/** Everything a person reads or hears on the page. */
function words(element: Element): string {
  const attributes = [...element.querySelectorAll('[aria-label], [title], [placeholder]')].flatMap((node) =>
    ['aria-label', 'title', 'placeholder'].map((name) => node.getAttribute(name) ?? ''),
  );
  return [element.textContent ?? '', ...attributes].join('\n');
}

/** A ready snapshot with a second app running, so the music lane has one to take. */
function withPlayer(name: string, language: Language = 'ko'): Snapshot {
  const snapshot = fixture(name, language);
  snapshot.app_capture!.apps.push({ exe: 'player.exe', display_name: 'Player', active: true });
  return snapshot;
}

/** Chooses a lane's app the way the skin offers it: a select, or the pedal's key and its row of apps. */
async function pickApp(skin: SkinName, lane: number, exe: string) {
  if (skin === 'pedal') {
    $(`[data-key="capture-app-${lane}"]`)!.click();
    const list = $(`[data-region="capture-apps-${lane}"]`)!;
    const option = [...list.querySelectorAll<HTMLButtonElement>('button')].find((button) => button.textContent?.includes(exe));
    option!.click();
  } else {
    const select = $<HTMLSelectElement>(`[data-key="capture-app-${lane}"]`)!;
    select.value = exe;
    select.dispatchEvent(new Event('change'));
  }
  await tick();
}

describe('every app-sound state draws in every skin', () => {
  for (const skin of SKINS) {
    for (const language of ['ko', 'en'] as Language[]) {
      it(`${skin} (${language})`, () => {
        for (const name of APP_FIXTURES) {
          const start = fixture(name, language);
          const tabs: (Tab | undefined)[] =
            start.phase.kind === 'setup' ? [undefined] : ['status', 'profile', 'upmix', 'settings'];
          for (const tab of tabs) {
            mounted(skin, start, tab);
            expect(root.dataset.ready, `${name} ${tab}`).toBe('true');
            if (language === 'en') {
              const found = words(root).match(HANGUL);
              expect(found, `${skin} ${name} ${tab ?? 'wizard'}: Hangul ${found?.[0]}`).toBeNull();
            }
          }
        }
      });
    }
  }
});

describe.each(SKINS)('the %s skin', (skin) => {
  it('chooses the entrance at the first step', async () => {
    const { sent } = mounted(skin, fixture('setup-welcome'));
    const driver = $<HTMLButtonElement>('[data-key="entrance-driver"]')!;
    const apps = $<HTMLButtonElement>('[data-key="entrance-app-sound"]')!;
    expect(driver.dataset.intent).toBe('choose_entrance');
    // Before the PC is read the supervisor answers whether the app-sound entrance can run.
    expect(apps.disabled).toBe(false);
    // What each needs is on the page.
    expect(root.textContent).toContain(ko.entrance.driverNeeds);
    expect(root.textContent).toContain(ko.entrance.appSoundDiscard);
    apps.click();
    await tick();
    expect(sent).toEqual([{ kind: 'choose_entrance', value: 'app_sound' }]);
  });

  it('shows the app-sound entrance as unavailable, with the reason, on an older build', () => {
    const start = fixture('setup-welcome');
    start.platform = { computer_name: 'PC', os_build: 19045, arch: 'x64', test_signing: false, secure_boot: 'enabled' };
    start.app_capture = { ...start.app_capture!, supported: false, min_build: 20348 };
    mounted(skin, start);
    expect($('[data-key="entrance-app-sound"]')).toBeNull();
    expect(root.textContent).toContain(ko.entrance.osRequirement(20348));
    expect(root.textContent).toContain(ko.entrance.unavailable);
    expect($<HTMLButtonElement>('[data-key="entrance-driver"]')!.disabled).toBe(false);
  });

  it('offers the driver entrance where the app-sound entrance cannot run', async () => {
    const start = fixture('setup-apps-unsupported');
    const { sent } = mounted(skin, start);
    expect(root.textContent).toContain(start.error!.message);
    expect(root.textContent).toContain(ko.wizard.appUnsupportedTitle);
    $('[data-key="entrance-driver"]')!.click();
    await tick();
    expect(sent).toEqual([{ kind: 'choose_entrance', value: 'driver' }]);
  });

  for (const language of ['ko', 'en'] as const) {
    for (const name of ['setup-unsupported', 'setup-apps-unsupported', 'setup-apps-unsupported-old']) {
      it(`${name} (${language}) offers only an entrance this PC can run`, () => {
        const start = fixture(name, language);
        mounted(skin, start);
        const t = language === 'ko' ? ko : en;
        const canSwitch = name === 'setup-apps-unsupported';
        const heading = $(skin === 'console' ? '.cn-step h3' : 'h1')!;
        expect(heading.textContent).toBe(canSwitch ? t.wizard.appUnsupportedTitle : t.wizard.unsupportedTitle);
        expect(root.textContent).toContain(canSwitch ? t.wizard.appUnsupportedBody : t.wizard.unsupportedBody);
        expect(root.textContent).toContain(String(start.platform!.os_build));
        expect($$('[data-intent="choose_entrance"]')).toHaveLength(canSwitch ? 1 : 0);
        expect($('#entrance-driver-blocked, #entrance-app-sound-blocked')).toBeNull();
        expect($('[aria-describedby*="entrance-driver-blocked"], [aria-describedby*="entrance-app-sound-blocked"]')).toBeNull();
        if (canSwitch) {
          expect($<HTMLButtonElement>('[data-key="entrance-driver"]')!.disabled).toBe(false);
        } else {
          expect(root.textContent).not.toContain(t.wizard.appUnsupportedTitle);
          expect(root.textContent).not.toContain(t.wizard.appUnsupportedBody);
          expect(root.textContent).not.toContain(t.entrance.appSoundBuild(start.app_capture!.min_build));
          expect(root.textContent).not.toContain(t.entrance.osRequirement(start.app_capture!.min_build));
        }
      });
    }
  }

  it('keeps blocked welcome choices and their reasons without offering their keys', () => {
    const start = fixture('setup-apps-unsupported-old');
    start.phase = { kind: 'setup', value: 'welcome' };
    start.error = null;
    mounted(skin, start);
    expect($$('[data-entrance]')).toHaveLength(2);
    expect($$('[data-intent="choose_entrance"]')).toHaveLength(0);
    expect(root.textContent).toContain(ko.entrance.unavailable);
    expect(root.textContent).toContain(start.driver_unsupported);
    expect(root.textContent).toContain(ko.entrance.appSoundBuild(start.app_capture!.min_build));
    expect(root.textContent).not.toContain(ko.entrance.osRequirement(start.app_capture!.min_build));
  });

  it('does not invite a blocked app-sound entrance from the test-signing step', () => {
    const start = fixture('setup-not-test-signing');
    start.platform!.os_build = 19045;
    start.app_capture!.supported = false;
    mounted(skin, start);
    expect($('[data-key="entrance-app-sound"]')).toBeNull();
    expect($('#entrance-app-sound-blocked')).toBeNull();
    expect(root.textContent).not.toContain(ko.wizard.testSigningOther);
    expect(root.textContent).not.toContain(ko.entrance.osRequirement(start.app_capture!.min_build));
    expect($<HTMLButtonElement>('[data-key="recheck"]')!.disabled).toBe(false);
  });

  it('keeps a blocked driver option and its reason in settings', () => {
    const start = fixture('ready-apps');
    start.driver_unsupported = fixture('setup-apps-unsupported-old').driver_unsupported!;
    mounted(skin, start, 'settings');
    const driver = $<HTMLButtonElement | HTMLOptionElement>('[data-key="entrance-driver"], [data-key="entrance"] option[value="driver"]')!;
    expect(driver.disabled).toBe(true);
    expect(root.textContent).toContain(start.driver_unsupported);
  });

  it('offers the app-sound entrance where test signing is off', async () => {
    const { sent } = mounted(skin, fixture('setup-not-test-signing'));
    expect(root.textContent).toContain(ko.wizard.testSigningOther);
    $('[data-key="entrance-app-sound"]')!.click();
    await tick();
    expect(sent).toEqual([{ kind: 'choose_entrance', value: 'app_sound' }]);
  });

  it('lists the discard outputs, a control each, and sends the one chosen', async () => {
    const start = fixture('setup-choose-discard');
    const { sent } = mounted(skin, start);
    expect(root.textContent).toContain(ko.wizard.discardBody);
    const list = $('[data-region="discard-list"]')!;
    const discards = start.app_capture!.discards;
    expect(list.querySelectorAll('button')).toHaveLength(discards.length);
    for (const output of discards) expect(list.textContent).toContain(output.name);
    $('[data-key="discard-1"]')!.click();
    await tick();
    expect(sent).toEqual([{ kind: 'choose_discard', value: discards[1]!.id }]);
  });

  it('with no discard output says the supervisor sentence and offers the recheck', async () => {
    const start = fixture('setup-choose-discard-empty');
    const { sent } = mounted(skin, start);
    expect($('[data-region="discard-list"]')).toBeNull();
    expect(root.textContent).toContain(start.error!.message);
    const recheck = $$('[data-intent="recheck"]');
    expect(recheck).toHaveLength(1);
    recheck[0]!.click();
    await tick();
    expect(sent).toEqual([{ kind: 'recheck' }]);
  });

  it('shows the lanes, their apps and states, and PASS as the driver entrance’s', () => {
    const start = fixture('ready-apps');
    mounted(skin, start, 'status');
    const text = root.textContent ?? '';
    expect(text).toContain(ko.entrance.lanes[0]);
    expect(text).toContain(ko.entrance.lanes[1]);
    expect(text).toContain(ko.entrance.lanes[2]);
    expect(text).toContain(ko.entrance.stateRunning);
    expect(text).toContain(ko.entrance.driverOnly);
    expect(text).toContain(ko.entrance.closedApp);
    // Driver-only items are not shown: the default roles and the endpoint names.
    expect(text).not.toContain(ko.status.rolesTaken);
    expect(text).not.toContain(ko.endpoints[2]!.name);
    for (const lane of [0, 1]) expect($<HTMLButtonElement>(`[data-key="capture-app-${lane}"]`)!.disabled, `lane ${lane}`).toBe(false);
  });

  it('chooses an app for the music lane', async () => {
    const { sent } = mounted(skin, withPlayer('ready-apps'), 'status');
    await pickApp(skin, 1, 'player.exe');
    expect(sent).toEqual([{ kind: 'choose_capture_app', value: { lane: 1, exe: 'player.exe' } }]);
  });

  it('chooses an app for the main lane when none is chosen', async () => {
    const { sent } = mounted(skin, withPlayer('ready-apps-no-app'), 'status');
    expect(root.textContent).toContain(ko.entrance.chooseApp);
    await pickApp(skin, 0, 'game.exe');
    expect(sent).toEqual([{ kind: 'choose_capture_app', value: { lane: 0, exe: 'game.exe' } }]);
  });

  it('locks the pickers while running, beside the reason', () => {
    mounted(skin, withPlayer('running-apps'), 'status');
    for (const lane of [0, 1]) expect($<HTMLButtonElement>(`[data-key="capture-app-${lane}"]`)!.disabled, `lane ${lane}`).toBe(true);
    expect(root.textContent).toContain(ko.entrance.appsLocked);
    expect(root.textContent).toContain(ko.entrance.stateCapturing);
  });

  it('says the lane waits for its app', () => {
    mounted(skin, fixture('running-apps-waiting'), 'status');
    expect(root.textContent).toContain(ko.entrance.stateWaiting);
  });

  it('keeps the entrance, the discard output and the exclusive switch in the settings', async () => {
    const start = fixture('ready-apps');
    const { sent } = mounted(skin, start, 'settings');
    expect($('[data-key="entrance"], [data-key="entrance-driver"]')).not.toBeNull();
    const list = $('[data-region="discard-list"]')!;
    expect(list.textContent).toContain(start.app_capture!.discards[1]!.name);
    // The pedal prints the exclusive switch's current position alone (ADR 0046, 법칙 5).
    const exclusiveWords = skin === 'pedal' ? (start.app_capture!.exclusive ? ko.entrance.exclusiveOn : ko.entrance.exclusiveOff) : ko.entrance.exclusiveHint;
    expect(root.textContent).toContain(exclusiveWords);
    expect(root.textContent).toContain(ko.entrance.ksNotUsed);
    // Kernel streaming is not offered with the app-sound entrance.
    const kernel = $<HTMLButtonElement | HTMLOptionElement>('[data-key="output-path-kernel_streaming"], #output-path option[value="kernel_streaming"]');
    expect(kernel?.disabled).toBe(true);
    const exclusive = $<HTMLElement>('[data-key="app-capture-exclusive"]')!;
    if (exclusive instanceof HTMLInputElement) {
      exclusive.checked = true;
      exclusive.dispatchEvent(new Event('change'));
    } else {
      exclusive.click();
    }
    await tick();
    expect(sent).toEqual([{ kind: 'set_app_capture_exclusive', value: true }]);
  });

  it('sends the discard output chosen in the settings', async () => {
    const start = fixture('ready-apps');
    const { sent } = mounted(skin, start, 'settings');
    $('[data-key="settings-discard-1"]')!.click();
    await tick();
    expect(sent).toEqual([{ kind: 'choose_discard', value: start.app_capture!.discards[1]!.id }]);
  });

  it('locks the entrance and the discard output while running', () => {
    mounted(skin, fixture('running-apps'), 'settings');
    expect(root.textContent).toContain(ko.entrance.locked);
    const entrance = $$<HTMLButtonElement | HTMLSelectElement>('[data-key="entrance"], [data-key="entrance-driver"], [data-key="entrance-app-sound"]');
    expect(entrance.length).toBeGreaterThan(0);
    for (const control of entrance) expect(control.disabled).toBe(true);
    for (const control of $$<HTMLButtonElement>('[data-region="discard-list"] button')) expect(control.disabled).toBe(true);
  });

  it('changes the entrance from the settings of the driver entrance', async () => {
    const { sent } = mounted(skin, fixture('ready'), 'settings');
    const select = $<HTMLSelectElement>('[data-key="entrance"]');
    if (select !== null) {
      select.value = 'app_sound';
      select.dispatchEvent(new Event('change'));
    } else {
      $('[data-key="entrance-app-sound"]')!.click();
    }
    await tick();
    expect(sent).toEqual([{ kind: 'choose_entrance', value: 'app_sound' }]);
  });

  it('says the driver part is not in use in English too', () => {
    mounted(skin, fixture('ready-apps', 'en'), 'settings');
    expect(root.textContent).toContain(en.entrance.driverOnly);
  });
});

describe('shared entrance availability', () => {
  it('treats an absent driver restriction as unknown, not blocked', () => {
    const start = fixture('setup-apps-unsupported');
    delete start.driver_unsupported;
    mounted('standard', start);
    expect(entranceBlocked(start, 'driver')).toBeNull();
    expect(switchTarget(start)).toBe('driver');
    expect(appSoundBlocked(start)).toBe(ko.entrance.osRequirement(start.app_capture!.min_build));
  });

  it('keeps both entrances locked while running', () => {
    const start = fixture('running-apps');
    mounted('standard', start);
    for (const kind of ['driver', 'app_sound'] as const) expect(entranceBlocked(start, kind)).toBe(ko.entrance.locked);
    expect(switchTarget(start)).toBeNull();
  });

  it('offers app sound when the driver is blocked but capture is supported', () => {
    const start = fixture('setup-unsupported');
    start.app_capture!.supported = true;
    expect(switchTarget(start)).toBe('app_sound');
  });
});

describe('the console with the app-sound entrance', () => {
  const layer = () => document.querySelector<HTMLElement>('.cn-palette-layer');
  const openPalette = () => document.dispatchEvent(new KeyboardEvent('keydown', { key: 'k', ctrlKey: true, bubbles: true }));

  it('labels the driver part as not in use and keeps the health line off a failure', () => {
    mounted('console', fixture('ready-apps'), 'status');
    const driverRow = $('[data-part="driver"]')!;
    expect(driverRow.textContent).toContain(ko.console.partDriverUnused);
    expect($('[data-region="health"]')!.className).not.toContain('tone-crit');
    // The map's first node is the app-sound entrance with the lanes' apps.
    expect($('.cn-flow-caption')!.textContent).toBe(ko.entrance.appSound);
    expect($$('.cn-flow-in .cn-node').map((node) => node.dataset.lane)).toEqual(['0', '1', '3']);
    expect($('.cn-flow-in [data-lane="0"]')!.textContent).toContain('Game');
  });

  it('files every new intent as a palette command, and the locked ones say why', () => {
    mounted('console', withPlayer('ready-apps'), 'settings');
    openPalette();
    const palette = layer()!;
    expect(palette.querySelector('[data-command="entrance=app_sound"]')?.getAttribute('aria-disabled')).toBe('true');
    expect(palette.querySelector('[data-command="entrance=driver"]')?.getAttribute('aria-disabled')).toBeNull();
    expect(palette.querySelector('[data-command="settings-discard-1"]')).not.toBeNull();
    expect(palette.querySelector('[data-command="app-capture-exclusive"]')).not.toBeNull();
    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
    palette.querySelector<HTMLInputElement>('.cn-palette-input')?.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));

    root = document.createElement('div');
    document.body.replaceChildren(root);
    mounted('console', withPlayer('running-apps'), 'status');
    openPalette();
    const running = layer()!;
    const music = running.querySelector('[data-command="capture-app-1=player.exe"]');
    expect(music?.getAttribute('aria-disabled')).toBe('true');
    expect(music?.textContent).toContain(ko.entrance.appsLocked);
  });

  it('runs the music lane picker from the palette', async () => {
    const { sent } = mounted('console', withPlayer('ready-apps'), 'status');
    openPalette();
    const option = layer()!.querySelector<HTMLElement>('[data-command="capture-app-1=player.exe"]')!;
    expect(option.getAttribute('aria-disabled')).toBeNull();
    // The main lane's app is not offered twice.
    expect(layer()!.querySelector('[data-command="capture-app-1=game.exe"]')?.textContent).toContain(ko.entrance.sameApp);
    option.click();
    await tick();
    expect(sent).toEqual([{ kind: 'choose_capture_app', value: { lane: 1, exe: 'player.exe' } }]);
  });
});

describe('the pedal with the app-sound entrance', () => {
  it('lights the jacks of the lanes with an app and keeps PASS dark', () => {
    mounted('pedal', fixture('running-apps'), 'status');
    const jack = (lane: number) => $(`.pd-jack[data-lane="${lane}"]`)!;
    expect(jack(0).dataset.signal).not.toBe('none');
    expect(jack(0).querySelector('.pd-led')?.getAttribute('data-glow')).not.toBe('off');
    // The fixture's engine has sound on the music and PASS endpoints, but no app feeds them.
    expect(jack(1).dataset.signal).toBe('none');
    expect(jack(2).dataset.signal).toBe('none');
    expect(jack(2).querySelector('.pd-led')?.getAttribute('data-glow')).toBe('off');
  });

  it('opens a lane’s row of apps with its key latched down and closes it after a choice', async () => {
    mounted('pedal', withPlayer('ready-apps'), 'status');
    const toggle = $('[data-key="capture-app-1"]')!;
    expect(toggle.getAttribute('aria-expanded')).toBe('false');
    expect(toggle.classList.contains('is-down')).toBe(false);
    toggle.click();
    const opened = $('[data-key="capture-app-1"]')!;
    expect(opened.getAttribute('aria-expanded')).toBe('true');
    // The ring is the lane's state (no app on the music lane: dark); the open row is the key held in.
    expect(opened.classList.contains('is-down')).toBe(true);
    expect(opened.classList.contains('is-lit')).toBe(false);
    const list = $('[data-region="capture-apps-1"]')!;
    expect(list.getAttribute('role')).toBe('radiogroup');
    // "None" is lit: the music lane has no app.
    const chosen = list.querySelector('[aria-checked="true"]')!;
    expect(chosen.textContent).toContain(ko.profile.none);
    expect(chosen.classList.contains('is-lit')).toBe(true);
    [...list.querySelectorAll<HTMLButtonElement>('button')].find((button) => button.textContent?.includes('player.exe'))!.click();
    await tick();
    expect($('[data-region="capture-apps-1"]')).toBeNull();
  });
});
