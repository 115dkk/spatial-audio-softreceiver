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
import type { App } from '../src/skins/types';
import { mount as mountConsole } from '../src/skins/console';
import { mount as mountPanel } from '../src/skins/panel';
import { en, ko } from '../src/text';

const HANGUL = /[ᄀ-ᇿ㄰-㆏가-힯]/;

type SkinName = 'standard' | 'panel' | 'console';
const SKINS: readonly SkinName[] = ['standard', 'panel', 'console'];
const MOUNT: Record<SkinName, (root: HTMLElement, api: Api, options: { tab?: Tab }) => App> = {
  standard: mountApp,
  panel: mountPanel,
  console: mountConsole,
};

const APP_FIXTURES = [
  'setup-welcome',
  'setup-apps-unsupported',
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

/** Chooses a lane's app the way the skin offers it: a select, or the panel's row and list. */
async function pickApp(skin: SkinName, lane: number, exe: string) {
  if (skin === 'panel') {
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
    const apps = $<HTMLButtonElement>('[data-key="entrance-app-sound"]')!;
    expect(apps.disabled).toBe(true);
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
    expect(root.textContent).toContain(ko.entrance.exclusiveHint);
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

describe('the front panel with the app-sound entrance', () => {
  it('lights the lanes with an app and keeps PASS dark', () => {
    mounted('panel', fixture('running-apps'), 'status');
    const lamp = (lane: number) => $(`.pn-lamp[data-lane="${lane}"]`)!.dataset.signal;
    expect(lamp(0)).not.toBe('none');
    // The fixture's engine has sound on the music and PASS endpoints, but no app feeds them.
    expect(lamp(1)).toBe('none');
    expect(lamp(2)).toBe('none');
  });

  it('opens a lane’s list of apps in the display and closes it after a choice', async () => {
    mounted('panel', withPlayer('ready-apps'), 'status');
    const toggle = $('[data-key="capture-app-1"]')!;
    expect(toggle.getAttribute('aria-expanded')).toBe('false');
    toggle.click();
    expect($('[data-key="capture-app-1"]')!.getAttribute('aria-expanded')).toBe('true');
    const list = $('[data-region="capture-apps-1"]')!;
    expect(list.getAttribute('role')).toBe('radiogroup');
    // "None" is marked: the music lane has no app.
    expect(list.querySelector('[aria-checked="true"]')?.textContent).toContain(ko.profile.none);
    [...list.querySelectorAll<HTMLButtonElement>('button')].find((button) => button.textContent?.includes('player.exe'))!.click();
    await tick();
    expect($('[data-region="capture-apps-1"]')).toBeNull();
  });
});
