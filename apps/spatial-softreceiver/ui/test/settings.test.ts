// The settings screen's output path and per-app entrance panels (ADR 0028).

import { beforeEach, describe, expect, it } from 'vitest';
import type { Api } from '../src/api';
import { mountApp } from '../src/app';
import type { Intent, Language, Snapshot } from '../src/bindings';
import { en as catalogEn, ko as catalogKo } from '../src/catalog';
import { fixture } from '../src/fixtures';
import { createMockApi } from '../src/mock';
import { en, ko } from '../src/text';

let root: HTMLElement;

beforeEach(() => {
  root = document.createElement('div');
  document.body.replaceChildren(root);
});

const tick = () => new Promise((resolve) => setTimeout(resolve, 0));

/** The settings tab over a bridge that records what it is sent. */
function mounted(start: Snapshot, dispatch?: Api['dispatch']) {
  const sent: Intent[] = [];
  const api: Api = {
    ...createMockApi(start),
    dispatch: dispatch ?? (async (intent) => void sent.push(intent)),
  };
  const app = mountApp(root, api, { tab: 'settings' });
  app.draw(start);
  return { app, sent };
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

const panelTitles = () => $$('.settings > .panel > .panel-title').map((title) => title.textContent);
const appRows = () => $$<HTMLTableRowElement>('[aria-labelledby="routing-apps-title"] tbody tr');
const otherRows = () => $$<HTMLTableRowElement>('[aria-labelledby="routing-others-title"] tbody tr');

describe('the settings panels', () => {
  it('stand in order, the ASIO input after the input per app', () => {
    mounted(fixture('ready'));
    expect(panelTitles()).toEqual([
      ko.settings.startup,
      ko.settings.language,
      ko.skins.label,
      ko.entrance.label,
      ko.settings.outputPath,
      ko.settings.routing,
      ko.asio,
      ko.settings.update,
      ko.settings.diagnostics,
    ]);
  });

  it('put each heading beside its controls, the heading naming a lone select', () => {
    mounted(fixture('ready'));
    const groups = $$('.settings .settings-group');
    // The nine above and the PC and driver facts.
    expect(groups).toHaveLength(11);
    for (const group of groups) {
      expect([...group.children].map((child) => child.className)).toEqual(['panel-title', 'settings-group-body']);
      expect(group.children[0]?.tagName).toBe('H2');
    }
    const lone: [string, string][] = [
      ['language', ko.settings.language],
      ['skin', ko.skins.label],
      ['entrance', ko.entrance.label],
      ['output-path', ko.settings.outputPath],
    ];
    for (const [key, title] of lone) {
      // Seen once, as the heading; the label stays the select's name.
      const label = $(`label[for="${key}"]`);
      expect(label?.textContent, key).toBe(title);
      expect(label?.classList.contains('visually-hidden'), key).toBe(true);
      expect($(`[data-key="${key}"]`)?.closest('.settings-group')?.querySelector('h2')?.textContent, key).toBe(title);
    }
    // A label that says more than its heading stays in sight.
    expect($('label[for="asio-layout"]')?.classList.contains('visually-hidden')).toBe(false);
  });
});

describe('the skin (ADR 0041)', () => {
  const select = () => $<HTMLSelectElement>('[data-key="skin"]');

  it.each<Language>(['ko', 'en'])('names every skin and shows the chosen one (%s)', (language) => {
    const catalog = language === 'ko' ? ko : en;
    mounted({ ...fixture('ready', language), skin: 'console' });
    expect(optionLabels(select())).toEqual([catalog.skins.standard, catalog.skins.panel, catalog.skins.console]);
    expect(chosen(select())).toBe('console');
  });

  it('reads a snapshot without the field as the standard skin', () => {
    const { skin: _, ...older } = fixture('ready');
    mounted(older);
    expect(chosen(select())).toBe('standard');
  });

  it('sends the choice in every phase the screen draws', () => {
    for (const name of ['ready', 'running', 'faulted']) {
      const { sent } = mounted(fixture(name));
      choose(select(), 'panel');
      expect(sent, name).toEqual([{ kind: 'set_skin', value: 'panel' }]);
    }
  });
});

describe('the output path', () => {
  const select = () => $<HTMLSelectElement>('[data-key="output-path"]');

  it('shows the path in the snapshot, WASAPI exclusive first and the default', () => {
    mounted(fixture('ready'));
    expect(chosen(select())).toBe('wasapi');
    expect(optionLabels(select())).toEqual(['WASAPI 독점 (기본)', '커널 스트리밍']);
    expect($('label[for="output-path"]')?.textContent).toBe('출력 방식');
    expect(select()?.disabled).toBe(false);
    expect(select()?.getAttribute('aria-describedby')).toBe('output-path-hint');
    expect($('#output-path-hint')?.textContent).toBe(ko.settings.outputPathHint);
    expect($('#output-path-locked')).toBeNull();

    mounted(fixture('ready-kernel-streaming'));
    expect(chosen(select())).toBe('kernel_streaming');
  });

  it('sends the path chosen while off', async () => {
    const { app, sent } = mounted(fixture('ready'));
    choose(select(), 'kernel_streaming');
    await tick();
    app.draw(fixture('ready-kernel-streaming'));
    await tick();
    choose(select(), 'wasapi');
    expect(sent).toEqual([
      { kind: 'set_output_path', value: 'kernel_streaming' },
      { kind: 'set_output_path', value: 'wasapi' },
    ]);
  });

  it('can be changed when stopped by a fault, as the supervisor allows', () => {
    mounted(fixture('faulted'));
    expect(select()?.disabled).toBe(false);
    expect($('#output-path-locked')).toBeNull();
  });

  it.each<Language>(['ko', 'en'])('is locked while running and says to turn off first (%s)', (language) => {
    mounted(fixture('running', language));
    const catalog = language === 'ko' ? catalogKo : catalogEn;
    expect(select()?.disabled).toBe(true);
    expect(select()?.getAttribute('aria-describedby')).toBe('output-path-locked output-path-hint');
    // The supervisor's own refusal, said before the choice is tried.
    expect($('#output-path-locked')?.textContent).toBe(catalog.supervisor.disable_before_changing_output);
  });

  it('is locked while an intent is in flight', async () => {
    let release: () => void = () => {};
    const { app } = mounted(fixture('ready'), () => new Promise<void>((resolve) => (release = resolve)));
    choose(select(), 'kernel_streaming');
    expect(select()?.disabled).toBe(true);
    release();
    await tick();
    app.draw(fixture('ready-kernel-streaming'));
    await tick();
    expect(select()?.disabled).toBe(false);
    expect(chosen(select())).toBe('kernel_streaming');
  });

  it('speaks English with the English snapshot', () => {
    mounted(fixture('running', 'en'));
    expect(optionLabels(select())).toEqual(['WASAPI exclusive (default)', 'Kernel streaming']);
    expect($('label[for="output-path"]')?.textContent).toBe('Output method');
    expect($('#output-path-hint')?.textContent).toBe(en.settings.outputPathHint);
    expect($('#output-path-locked')?.textContent).toBe('Turn it off before changing the output.');
  });
});

describe('the per-app entrance', () => {
  const checkbox = () => $<HTMLInputElement>('[data-key="routing-enabled"]');
  const ruleSelect = (exe: string) => $<HTMLSelectElement>(`[data-key="routing-rule-${exe}"]`);
  const remove = (exe: string) => $<HTMLButtonElement>(`[data-key="routing-remove-${exe}"]`);

  it('switches routing with its checkbox', async () => {
    const { app, sent } = mounted(fixture('ready'));
    expect(checkbox()?.checked).toBe(true);
    expect(checkbox()?.disabled).toBe(false);
    expect($('label[for="routing-enabled"]')?.textContent).toBe('앱을 알맞은 입구로 보내기');
    expect($('#routing-hint')?.textContent).toBe(
      ko.settings.routingHint(catalogKo.names.endpoint_pass, catalogKo.names.endpoint_main),
    );
    checkbox()?.click();
    await tick();
    const off = fixture('ready');
    off.routing.enabled = false;
    app.draw(off);
    await tick();
    expect(checkbox()?.checked).toBe(false);
    checkbox()?.click();
    expect(sent).toEqual([
      { kind: 'set_routing_enabled', value: false },
      { kind: 'set_routing_enabled', value: true },
    ]);
  });

  it('lists the apps with sound now: name, program file, where each plays and its rule', () => {
    mounted(fixture('ready'));
    const [music, browser] = appRows();
    expect(appRows()).toHaveLength(2);
    expect(music?.querySelector('.routing-app-name')?.textContent).toBe('Apple Music');
    expect(music?.querySelector('.routing-exe')?.textContent).toBe('applemusic.exe');
    expect(music?.querySelector('.routing-now')?.textContent).toBe('공간 소프트리시버');
    expect(chosen(ruleSelect('applemusic.exe'))).toBe('music');
    expect(browser?.querySelector('.routing-app-name')?.textContent).toBe('Browser');
    expect(chosen(ruleSelect('browser.exe'))).toBe('');
    expect(optionLabels(ruleSelect('browser.exe'))).toEqual([
      '따로 정하지 않음',
      catalogKo.names.endpoint_main,
      catalogKo.names.endpoint_music,
      catalogKo.names.endpoint_pass,
    ]);
    // Each choice is named by its row's app and the column.
    expect(ruleSelect('applemusic.exe')?.getAttribute('aria-labelledby')).toBe('routing-app-0 routing-rule-head');
    expect($('#routing-rule-head')?.textContent).toBe('보낼 입구');
    expect($('.routing-empty')).toBeNull();
  });

  it('marks only the app whose output Spatial SoftReceiver set', () => {
    mounted(fixture('ready'));
    const [music, browser] = appRows();
    expect(music?.querySelector('.routing-moved')?.textContent).toBe('공간 소프트리시버가 설정함');
    expect(browser?.querySelector('.routing-moved')).toBeNull();
  });

  it('shows a dash where an app plays on no entrance', () => {
    const start = fixture('ready');
    start.routing.apps[1]!.entrance_now = null;
    mounted(start);
    expect(appRows()[1]?.querySelector('.routing-now')?.textContent).toBe('-');
  });

  it('sets the rule chosen for a playing app, null for "not set"', () => {
    const { sent } = mounted(fixture('ready'));
    choose(ruleSelect('browser.exe'), 'pass');
    expect(sent).toEqual([{ kind: 'set_app_rule', value: { exe: 'browser.exe', entrance: 'pass' } }]);
    // Not RemoveAppRule: null also turns Apple Music's built-in rule off.
    const { sent: again } = mounted(fixture('ready'));
    choose(ruleSelect('applemusic.exe'), '');
    expect(again).toEqual([{ kind: 'set_app_rule', value: { exe: 'applemusic.exe', entrance: null } }]);
  });

  describe('the rules for the other apps', () => {
    const row = (exe: string) => otherRows().find((candidate) => candidate.cells[0]?.textContent === exe);
    /** The entrance, the default under it (or null), and the row's last cell. */
    const shown = (exe: string) => {
      const found = row(exe);
      return [
        found?.querySelector('.routing-entrance > span:first-child')?.textContent,
        found?.querySelector('.routing-built-in-was')?.textContent ?? null,
        found?.cells[2]?.textContent,
      ];
    };
    const { endpoint_main: main, endpoint_music: music, endpoint_pass: pass } = catalogKo.names;

    it('lists every built-in rule and every rule of the user, by program file', () => {
      mounted(fixture('ready'));
      // Apple Music has sound, so its rule is in its row above, not here.
      expect(otherRows().map((candidate) => candidate.cells[0]?.textContent)).toEqual([
        'foobar2000.exe',
        'kodi.exe',
        'mpc-be.exe',
        'mpc-be64.exe',
        'mpc-hc.exe',
        'mpc-hc64.exe',
        'player.exe',
      ]);
    });

    it('says "default" without a button where a built-in rule is as it ships', () => {
      mounted(fixture('ready'));
      for (const exe of ['foobar2000.exe', 'mpc-be.exe', 'mpc-be64.exe', 'mpc-hc64.exe']) {
        expect(shown(exe), exe).toEqual([pass, null, '기본값']);
        expect(row(exe)?.querySelector('button'), exe).toBeNull();
      }
    });

    it('names the default and offers it back where a built-in rule was turned off or changed', () => {
      const { sent } = mounted(fixture('ready'));
      expect(shown('kodi.exe')).toEqual(['따로 정하지 않음', `기본값: ${pass}`, '기본값으로 되돌리기']);
      expect(shown('mpc-hc.exe')).toEqual([main, `기본값: ${pass}`, '기본값으로 되돌리기']);
      expect(remove('kodi.exe')?.getAttribute('aria-describedby')).toBe('routing-other-1');
      expect($('#routing-other-1')?.textContent).toBe('kodi.exe');
      remove('kodi.exe')?.click();
      expect(sent).toEqual([{ kind: 'remove_app_rule', value: 'kodi.exe' }]);
    });

    it('removes a rule of the user own, which has no default', () => {
      const { sent } = mounted(fixture('ready'));
      expect(shown('player.exe')).toEqual([music, null, '지우기']);
      remove('player.exe')?.click();
      expect(sent).toEqual([{ kind: 'remove_app_rule', value: 'player.exe' }]);
    });

    it('shows the default again once the mock supervisor restores it', async () => {
      const start = fixture('ready');
      const api = createMockApi(start);
      const app = mountApp(root, api, { tab: 'settings' });
      app.draw(start);
      remove('kodi.exe')?.click();
      await tick();
      app.draw(await api.snapshot());
      await tick();
      expect(shown('kodi.exe')).toEqual([pass, null, '기본값']);
      expect(remove('kodi.exe')).toBeNull();
    });

    it('speaks English', () => {
      mounted(fixture('ready', 'en'));
      const englishPass = catalogEn.names.endpoint_pass;
      expect(shown('foobar2000.exe')).toEqual([englishPass, null, 'Default']);
      expect(shown('kodi.exe')).toEqual(['Not set', `Default: ${englishPass}`, 'Restore default']);
      expect(shown('player.exe')).toEqual([catalogEn.names.endpoint_music, null, 'Remove']);
    });
  });

  it('says when no app has sound now and lists every rule below', () => {
    mounted(fixture('ready-kernel-streaming'));
    expect(appRows()).toHaveLength(0);
    expect($('[aria-labelledby="routing-apps-title"] p')?.textContent).toBe('지금 재생 중인 앱이 없습니다.');
    expect(otherRows().map((row) => row.cells[0]?.textContent)).toEqual(
      fixture('ready-kernel-streaming').routing.rules.map((rule) => rule.exe),
    );
    expect(otherRows().find((row) => row.cells[0]?.textContent === 'applemusic.exe')?.cells[1]?.textContent).toBe(
      catalogKo.names.endpoint_music,
    );
  });

  it.each<Language>(['ko', 'en'])(
    'where Windows cannot set an app output, says so in the supervisor sentence and locks the controls (%s)',
    (language) => {
      mounted(fixture('ready-routing-unavailable', language));
      const catalog = language === 'ko' ? catalogKo : catalogEn;
      expect($('#routing-unavailable')?.textContent).toBe(catalog.supervisor.entrance_routing_unavailable);
      expect(checkbox()?.disabled).toBe(true);
      expect(checkbox()?.checked).toBe(true);
      expect(checkbox()?.getAttribute('aria-describedby')).toBe('routing-unavailable routing-hint');
      // The supervisor lists no app here, so the screen does not claim none plays.
      expect($('#routing-apps-title')).toBeNull();
      expect(otherRows()).toHaveLength(7);
      // The built-in rules as they ship: nothing to press.
      expect($$('.routing-default')).toHaveLength(7);
      expect($$('[data-intent="remove_app_rule"]')).toHaveLength(0);
    },
  );

  it('keeps apps and rule buttons in view while unavailable, locked', () => {
    const start = fixture('ready');
    start.routing.available = false;
    mounted(start);
    expect(appRows()).toHaveLength(2);
    expect(ruleSelect('browser.exe')?.disabled).toBe(true);
    const buttons = $$<HTMLButtonElement>('[data-intent="remove_app_rule"]');
    expect(buttons.map((button) => button.dataset.key)).toEqual([
      'routing-remove-kodi.exe',
      'routing-remove-mpc-hc.exe',
      'routing-remove-player.exe',
    ]);
    expect(buttons.every((button) => button.disabled)).toBe(true);
  });

  it('locks every routing control while an intent is in flight', async () => {
    let release: () => void = () => {};
    const { app } = mounted(fixture('ready'), () => new Promise<void>((resolve) => (release = resolve)));
    choose(ruleSelect('browser.exe'), 'music');
    expect(checkbox()?.disabled).toBe(true);
    expect(ruleSelect('applemusic.exe')?.disabled).toBe(true);
    expect(remove('kodi.exe')?.disabled).toBe(true);
    release();
    await tick();
    app.draw(fixture('ready'));
    await tick();
    expect(checkbox()?.disabled).toBe(false);
    expect(ruleSelect('browser.exe')?.disabled).toBe(false);
    expect(remove('kodi.exe')?.disabled).toBe(false);
  });

  it('follows the mock supervisor: a rule set for a playing app moves it once running', async () => {
    const start = fixture('running');
    const api = createMockApi(start);
    const app = mountApp(root, api, { tab: 'settings' });
    app.draw(start);
    expect(appRows()[1]?.querySelector('.routing-moved')).toBeNull();
    await api.dispatch({ kind: 'set_app_rule', value: { exe: 'browser.exe', entrance: 'music' } });
    app.draw(await api.snapshot());
    expect(chosen(ruleSelect('browser.exe'))).toBe('music');
    expect(appRows()[1]?.querySelector('.routing-moved')).not.toBeNull();
  });

  it('speaks English with the English snapshot, entrance names from the names table', () => {
    mounted(fixture('ready', 'en'));
    expect($('label[for="routing-enabled"]')?.textContent).toBe('Send apps to the right input');
    expect($('#routing-apps-title')?.textContent).toBe('Apps playing now');
    expect($('#routing-others-title')?.textContent).toBe('Inputs set for other apps');
    expect(appRows()[0]?.querySelector('.routing-now')?.textContent).toBe(catalogEn.names.endpoint_main);
    expect(appRows()[0]?.querySelector('.routing-moved')?.textContent).toBe('Set by Spatial SoftReceiver');
    expect(optionLabels(ruleSelect('browser.exe'))).toEqual([
      'Not set',
      'Spatial SoftReceiver',
      'Spatial SoftReceiver Music',
      'Spatial SoftReceiver PASS',
    ]);
    expect($('#routing-hint')?.textContent).toBe(
      'While Spatial SoftReceiver is on, an app with an input set is sent there through the Windows per-app output device setting. An app without one that plays on Spatial SoftReceiver PASS is moved to Spatial SoftReceiver.',
    );
    mounted(fixture('ready-kernel-streaming', 'en'));
    expect($('[aria-labelledby="routing-apps-title"] p')?.textContent).toBe('No app is playing now.');
  });
});

describe('the mock supervisor follows the routing rules (routing.rs)', () => {
  const rowOf = (snapshot: Snapshot, exe: string) => snapshot.routing.rules.find((row) => row.exe === exe) ?? null;

  it('turns a built-in rule off with null, saves only differences and restores it', async () => {
    const start = fixture('ready-kernel-streaming');
    const api = createMockApi(start);
    await api.dispatch({ kind: 'set_app_rule', value: { exe: 'Kodi.exe', entrance: null } });
    expect(rowOf(await api.snapshot(), 'kodi.exe')).toEqual({ exe: 'kodi.exe', entrance: null, built_in: 'pass' });
    // The built-in entrance itself is no override.
    await api.dispatch({ kind: 'set_app_rule', value: { exe: 'kodi.exe', entrance: 'pass' } });
    expect(rowOf(await api.snapshot(), 'kodi.exe')).toEqual({ exe: 'kodi.exe', entrance: 'pass', built_in: 'pass' });

    await api.dispatch({ kind: 'set_app_rule', value: { exe: 'mpc-hc.exe', entrance: 'main' } });
    expect(rowOf(await api.snapshot(), 'mpc-hc.exe')).toEqual({ exe: 'mpc-hc.exe', entrance: 'main', built_in: 'pass' });
    await api.dispatch({ kind: 'remove_app_rule', value: 'mpc-hc.exe' });
    expect(rowOf(await api.snapshot(), 'mpc-hc.exe')).toEqual({ exe: 'mpc-hc.exe', entrance: 'pass', built_in: 'pass' });

    // A rule of the user's own; null for it is the same as no rule.
    await api.dispatch({ kind: 'set_app_rule', value: { exe: 'player.exe', entrance: 'music' } });
    expect(rowOf(await api.snapshot(), 'player.exe')).toEqual({ exe: 'player.exe', entrance: 'music', built_in: null });
    await api.dispatch({ kind: 'set_app_rule', value: { exe: 'player.exe', entrance: null } });
    expect(rowOf(await api.snapshot(), 'player.exe')).toBeNull();

    expect((await api.snapshot()).routing.rules).toEqual(start.routing.rules);
  });

  it('gives a playing app its built-in rule back after RemoveAppRule', async () => {
    const api = createMockApi(fixture('running'));
    await api.dispatch({ kind: 'set_app_rule', value: { exe: 'applemusic.exe', entrance: null } });
    let snapshot = await api.snapshot();
    expect(snapshot.routing.apps[0]?.rule).toBeNull();
    expect(rowOf(snapshot, 'applemusic.exe')).toEqual({ exe: 'applemusic.exe', entrance: null, built_in: 'music' });
    await api.dispatch({ kind: 'remove_app_rule', value: 'applemusic.exe' });
    snapshot = await api.snapshot();
    expect(snapshot.routing.apps[0]?.rule).toBe('music');
    expect(rowOf(snapshot, 'applemusic.exe')).toEqual({ exe: 'applemusic.exe', entrance: 'music', built_in: 'music' });
  });

  it('moves an app whose built-in rule is off off the PASS entrance, like one without a rule', async () => {
    // tests/machine.rs routing_a_built_in_rule_turned_off_treats_the_app_like_one_without_a_rule
    const start = fixture('running');
    expect(rowOf(start, 'kodi.exe')?.entrance).toBeNull();
    start.routing.apps.push({ exe: 'kodi.exe', display_name: 'Kodi', entrance_now: 'pass', rule: null, moved: false });
    const api = createMockApi(start);
    await api.dispatch({ kind: 'set_routing_enabled', value: true });
    let snapshot = await api.snapshot();
    const kodi = () => snapshot.routing.apps.find((app) => app.exe === 'kodi.exe');
    expect(kodi()?.rule).toBeNull();
    expect(kodi()?.moved).toBe(true);
    expect(snapshot.notes).toContain(catalogKo.supervisor.app_entrance_moved('Kodi'));
    await api.dispatch({ kind: 'remove_app_rule', value: 'kodi.exe' });
    snapshot = await api.snapshot();
    expect(kodi()?.rule).toBe('pass');
  });
});
