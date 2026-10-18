// The settings screen: login autostart, the language, the output path, the
// entrance each app plays into, the app's update, diagnostic recording, and
// what the supervisor knows about the PC and the driver.

import { toUserError } from '../api';
import type { Entrance, Language, OutputPath, RoutedApp, RuleView } from '../bindings';
import { type Child, facts, h } from '../dom';
import { text } from '../text';
import { type Context, type Drawn, button, described, intentButton, panel } from './common';
import { driverRows, platformRows } from './facts';

function autostart(ctx: Context): HTMLElement {
  const t = text.settings;
  const native = ctx.shell.native;
  // A change in flight lives in view state, not in this checkbox: a redraw
  // (a snapshot, a tab switch) builds a new checkbox that must stay locked.
  const inFlight = ctx.view.autostartWanted !== null;
  const checkbox = h('input', {
    type: 'checkbox',
    id: 'autostart',
    'data-key': 'autostart',
    checked: (ctx.view.autostartWanted ?? ctx.view.autostart) === true,
    disabled: !native || ctx.view.autostart === null || inFlight,
    'aria-busy': inFlight ? 'true' : null,
    'aria-describedby': 'autostart-hint',
  });
  checkbox.addEventListener('change', () => {
    // `ctx.view` is the app's live view state, so a checkbox from an older
    // drawing also sees a change that is already in flight.
    if (ctx.view.autostartWanted !== null || ctx.view.autostart === null) {
      checkbox.checked = !checkbox.checked;
      return;
    }
    const wanted = checkbox.checked;
    const before = ctx.view.autostart;
    ctx.setView({ autostartWanted: wanted });
    ctx.shell
      .setAutostart(wanted)
      .then(() => ctx.shell.autostartEnabled())
      .then(
        (enabled) => ctx.setView({ autostart: enabled, autostartWanted: null }),
        (reason: unknown) => {
          ctx.report({ message: String(reason), next: 'none' });
          ctx.setView({ autostart: before, autostartWanted: null });
        },
      );
  });
  return h(
    'div',
    { class: 'check' },
    checkbox,
    h('label', { for: 'autostart' }, t.autostart),
    h('p', { id: 'autostart-hint', class: 'hint' }, native ? t.autostartHint : t.autostartUnavailable),
  );
}

/** The running version, and the downloaded one with the button that applies it. */
function update(ctx: Context): HTMLElement {
  const t = text.settings;
  const state = ctx.view.update;
  const children: Child[] = [];
  if (!ctx.shell.native) {
    children.push(h('p', { class: 'hint' }, t.updateUnavailable));
  } else if (state === null) {
    children.push(h('p', { class: 'hint' }, t.updateUnknown));
  } else {
    children.push(h('p', { class: 'body-text' }, t.updateCurrent(state.current)));
    if (state.ready === null) {
      children.push(h('p', { class: 'hint' }, t.updateNone));
    } else {
      children.push(h('p', { class: 'body-text' }, t.updateReady(state.ready)));
      children.push(
        h(
          'div',
          { class: 'actions' },
          button(
            t.updateApply,
            () => {
              ctx.shell.applyUpdate().catch((reason: unknown) => ctx.report(toUserError(reason)));
            },
            { key: 'update-apply', variant: 'primary', disabled: ctx.busy },
          ),
        ),
      );
    }
  }
  return h('div', { class: 'update' }, ...children);
}

/** The language: automatic (the Windows display language) or one of the two. */
function language(ctx: Context): HTMLElement {
  const t = text.settings;
  const current = ctx.snapshot.language_choice ?? 'auto';
  const options: Array<[value: 'auto' | Language, label: string]> = [
    ['auto', t.languageAuto],
    ['ko', t.languageKo],
    ['en', t.languageEn],
  ];
  const select = h('select', {
    id: 'language',
    'data-key': 'language',
    disabled: ctx.busy,
    'aria-describedby': 'language-hint',
  });
  for (const [value, label] of options) {
    select.append(h('option', { value, selected: value === current }, label));
  }
  select.addEventListener('change', () => {
    const value = select.value;
    ctx.send({ kind: 'set_language', value: value === 'auto' ? null : (value as Language) });
  });
  return h(
    'div',
    { class: 'field' },
    h('label', { for: 'language' }, t.language),
    select,
    h('p', { id: 'language-hint', class: 'hint' }, t.languageHint),
  );
}

/**
 * How the engine reaches the DAC: WASAPI exclusive (the default) or kernel
 * streaming with the DAC's endpoint disabled while it runs (ADR 0028). The
 * supervisor takes a change only while the engine is off.
 */
function outputPath(ctx: Context): HTMLElement {
  const t = text.settings;
  const running = ctx.snapshot.phase.kind === 'running';
  const current = ctx.snapshot.output_path;
  const options: Array<[value: OutputPath, label: string]> = [
    ['wasapi', t.outputPathWasapi],
    ['kernel_streaming', text.meters.kernelStreaming],
  ];
  const select = h('select', {
    id: 'output-path',
    'data-key': 'output-path',
    disabled: running || ctx.busy,
    'aria-describedby': described(running && 'output-path-locked', 'output-path-hint'),
  });
  for (const [value, label] of options) {
    select.append(h('option', { value, selected: value === current }, label));
  }
  select.addEventListener('change', () => {
    ctx.send({ kind: 'set_output_path', value: select.value as OutputPath });
  });
  return h(
    'div',
    { class: 'field' },
    h('label', { for: 'output-path' }, t.outputPath),
    select,
    // Why the choice is locked comes first, next to it.
    running ? h('p', { id: 'output-path-locked', class: 'hint' }, t.outputPathLocked) : null,
    h('p', { id: 'output-path-hint', class: 'hint' }, t.outputPathHint),
  );
}

const ENTRANCES: readonly Entrance[] = ['main', 'music', 'pass'];

/** An entrance by its endpoint's name, as Windows shows it. */
function entranceName(entrance: Entrance): string {
  return text.endpoints[ENTRANCES.indexOf(entrance)]!.name;
}

/**
 * The apps with sound now: where each plays and the entrance it is sent to.
 * Every choice sets the app's rule; "not set" sets none, which also turns a
 * built-in rule off, and the built-in entrance itself drops the override.
 */
function appTable(ctx: Context, apps: RoutedApp[], locked: boolean): HTMLElement {
  const t = text.settings;
  const rows = apps.map((app, index) => {
    const nameId = `routing-app-${index}`;
    const select = h('select', {
      id: `routing-rule-${index}`,
      'data-key': `routing-rule-${app.exe}`,
      disabled: locked,
      'aria-labelledby': `${nameId} routing-rule-head`,
    });
    select.append(h('option', { value: '', selected: app.rule === null }, t.routingNoRule));
    for (const entrance of ENTRANCES) {
      select.append(h('option', { value: entrance, selected: app.rule === entrance }, entranceName(entrance)));
    }
    select.addEventListener('change', () => {
      const value = select.value;
      ctx.send({ kind: 'set_app_rule', value: { exe: app.exe, entrance: value === '' ? null : (value as Entrance) } });
    });
    return h(
      'tr',
      null,
      h(
        'th',
        { scope: 'row', class: 'routing-app' },
        h('span', { id: nameId, class: 'routing-app-name' }, app.display_name),
        h('span', { class: 'routing-exe' }, app.exe),
        app.moved ? h('span', { class: 'routing-moved' }, t.routingMoved) : null,
      ),
      h('td', { class: 'routing-now' }, app.entrance_now === null ? '-' : entranceName(app.entrance_now)),
      h('td', { class: 'routing-choice' }, select),
    );
  });
  return h(
    'table',
    { class: 'routing-table', 'aria-labelledby': 'routing-apps-title' },
    h(
      'thead',
      null,
      h(
        'tr',
        null,
        h('th', { scope: 'col' }, t.routingApp),
        h('th', { scope: 'col' }, t.routingNow),
        h('th', { scope: 'col', id: 'routing-rule-head' }, t.routingRule),
      ),
    ),
    h('tbody', null, ...rows),
  );
}

/**
 * The rules for apps without sound now, built-in ones among them. A built-in
 * rule as it ships says so; one changed or turned off names its default and
 * offers it back; a rule of the user's own can be removed. Both buttons drop
 * the saved override (RemoveAppRule).
 */
function ruleTable(ctx: Context, rules: RuleView[], locked: boolean): HTMLElement {
  const t = text.settings;
  const rows = rules.map((rule, index) => {
    const exeId = `routing-other-${index}`;
    const changed = rule.built_in !== null && rule.entrance !== rule.built_in;
    let end: HTMLElement;
    if (rule.built_in !== null && !changed) {
      end = h('span', { class: 'routing-default' }, t.routingBuiltIn);
    } else {
      end = intentButton(
        ctx,
        changed ? t.routingRestore : t.routingRemove,
        { kind: 'remove_app_rule', value: rule.exe },
        { key: `routing-remove-${rule.exe}`, variant: 'quiet' },
      );
      if (locked) (end as HTMLButtonElement).disabled = true;
      end.setAttribute('aria-describedby', exeId);
    }
    return h(
      'tr',
      null,
      h('th', { scope: 'row', id: exeId, class: 'routing-app routing-app-name' }, rule.exe),
      h(
        'td',
        { class: 'routing-entrance' },
        h('span', null, rule.entrance === null ? t.routingNoRule : entranceName(rule.entrance)),
        changed && rule.built_in !== null
          ? h('span', { class: 'routing-built-in-was' }, t.routingBuiltInWas(entranceName(rule.built_in)))
          : null,
      ),
      h('td', { class: 'routing-choice' }, end),
    );
  });
  return h(
    'table',
    { class: 'routing-table', 'aria-labelledby': 'routing-others-title' },
    h(
      'thead',
      null,
      h(
        'tr',
        null,
        h('th', { scope: 'col' }, t.routingExe),
        h('th', { scope: 'col' }, t.routingRule),
        h('td', null),
      ),
    ),
    h('tbody', null, ...rows),
  );
}

/**
 * Which entrance each app plays into (ADR 0028). The supervisor sends apps
 * with a rule there through Windows' per-app output device setting and
 * moves apps without one off the PASS entrance. Where Windows has no such
 * setting the controls stay in view, locked, beside the reason.
 */
function routing(ctx: Context): Child[] {
  const t = text.settings;
  const view = ctx.snapshot.routing;
  const locked = ctx.busy || !view.available;
  const checkbox = h('input', {
    type: 'checkbox',
    id: 'routing-enabled',
    'data-key': 'routing-enabled',
    checked: view.enabled,
    disabled: locked,
    'aria-describedby': described(!view.available && 'routing-unavailable', 'routing-hint'),
  });
  checkbox.addEventListener('change', () => ctx.send({ kind: 'set_routing_enabled', value: checkbox.checked }));

  const playing = new Set(view.apps.map((app) => app.exe));
  const others = view.rules.filter((rule) => !playing.has(rule.exe));
  // The supervisor lists no app where it cannot route them; "none playing"
  // would then be a guess.
  const showApps = view.available || view.apps.length > 0;
  return [
    h(
      'div',
      { class: 'check' },
      checkbox,
      h('label', { for: 'routing-enabled' }, t.routingEnabled),
      // Why the controls are locked comes first, next to the switch.
      view.available ? null : h('p', { id: 'routing-unavailable', class: 'hint' }, t.routingUnavailable),
      h('p', { id: 'routing-hint', class: 'hint' }, t.routingHint(entranceName('pass'), entranceName('main'))),
    ),
    showApps
      ? h(
          'section',
          { class: 'routing-section', 'aria-labelledby': 'routing-apps-title' },
          h('h3', { id: 'routing-apps-title', class: 'routing-section-title' }, t.routingApps),
          view.apps.length === 0 ? h('p', { class: 'muted' }, t.routingNoApps) : appTable(ctx, view.apps, locked),
        )
      : null,
    others.length === 0
      ? null
      : h(
          'section',
          { class: 'routing-section', 'aria-labelledby': 'routing-others-title' },
          h('h3', { id: 'routing-others-title', class: 'routing-section-title' }, t.routingOthers),
          ruleTable(ctx, others, locked),
        ),
  ];
}

export function settings(ctx: Context): Drawn {
  const t = text.settings;
  const { platform, driver } = ctx.snapshot;
  const element = h(
    'div',
    { class: 'settings' },
    panel(t.startup, [autostart(ctx)]),
    panel(t.language, [language(ctx)]),
    panel(t.outputPath, [outputPath(ctx)]),
    panel(t.routing, routing(ctx)),
    panel(t.update, [update(ctx)]),
    panel(t.diagnostics, [
      h('p', { class: 'body-text' }, t.recordHint),
      h(
        'div',
        { class: 'actions' },
        intentButton(ctx, t.record, { kind: 'start_diagnostic_recording' }, { key: 'record' }),
      ),
    ]),
    h(
      'div',
      { class: 'settings-facts' },
      panel(t.platform, [platform === null ? h('p', { class: 'muted' }, t.unknown) : facts(platformRows(platform))]),
      panel(t.driver, [driver === null ? h('p', { class: 'muted' }, t.unknown) : facts(driverRows(driver))]),
    ),
  );
  return { element };
}
