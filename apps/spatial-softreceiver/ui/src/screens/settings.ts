// The settings screen: login autostart, the language, the entrance sound
// comes in through (with the app-sound entrance's discard output), the
// output path, the entrance each app plays into, the ASIO input, the app's
// update, diagnostic recording, and what the supervisor knows about the PC
// and the driver.

import { toUserError } from '../api';
import type { AsioLayout, Entrance, Language, OutputPath, RoutedApp, RuleView } from '../bindings';
import { type Child, facts, h } from '../dom';
import { ASIO_ATTENUATION_MAX, ASIO_DEFAULTS, ASIO_LAYOUTS } from '../guard';
import { SKINS, isSkin, skinOf } from '../skins/names';
import { text } from '../text';
import { type Context, type Drawn, button, described, intentButton } from './common';
import {
  ENTRANCE_KINDS,
  appMode,
  appSoundBlocked,
  captureOf,
  entranceGets,
  entranceName as kindName,
  entranceNeeds,
  entranceOf,
} from './entrance';
import { driverRows, platformRows } from './facts';
import { discardList } from './wizard';

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
    // The group's heading says it beside the select; the label stays its name.
    h('label', { for: 'language', class: 'visually-hidden' }, t.language),
    select,
    h('p', { id: 'language-hint', class: 'hint' }, t.languageHint),
  );
}

/** The skin (ADR 0041): the supervisor saves it and the page reloads in it. */
function skin(ctx: Context): HTMLElement {
  const t = text.skins;
  const current = skinOf(ctx.snapshot);
  const select = h('select', {
    id: 'skin',
    'data-key': 'skin',
    disabled: ctx.busy,
    'aria-describedby': 'skin-hint',
  });
  for (const value of SKINS) {
    select.append(h('option', { value, selected: value === current }, t[value]));
  }
  select.addEventListener('change', () => {
    if (isSkin(select.value)) ctx.send({ kind: 'set_skin', value: select.value });
  });
  return h(
    'div',
    { class: 'field' },
    h('label', { for: 'skin', class: 'visually-hidden' }, t.label),
    select,
    h('p', { id: 'skin-hint', class: 'hint' }, t.hint),
  );
}

/**
 * The entrance sound comes in through (ADR 0043): the driver's or the
 * app-sound one, what the chosen one receives and needs, and with the
 * app-sound entrance the discard output it sends the apps' original sound
 * to. The supervisor takes a change of entrance only while the engine is
 * off; the app-sound entrance stays in the list, locked, where the PC cannot
 * run it.
 */
function entrance(ctx: Context): Child[] {
  const e = text.entrance;
  const { snapshot } = ctx;
  const current = entranceOf(snapshot);
  const running = snapshot.phase.kind === 'running';
  const blocked = appSoundBlocked(snapshot);
  ctx.offer('choose_entrance');
  const select = h('select', {
    id: 'entrance',
    'data-key': 'entrance',
    'data-intent': 'choose_entrance',
    disabled: running || ctx.busy,
    'aria-busy': ctx.pending === 'choose_entrance' ? 'true' : null,
    'aria-describedby': described(running && 'entrance-locked', blocked !== null && 'entrance-blocked', 'entrance-hint'),
  });
  for (const kind of ENTRANCE_KINDS) {
    select.append(
      h('option', { value: kind, selected: kind === current, disabled: kind !== current && kind === 'app_sound' && blocked !== null }, kindName(kind)),
    );
  }
  select.addEventListener('change', () => {
    const value = ENTRANCE_KINDS.find((kind) => kind === select.value);
    if (value !== undefined && value !== current) ctx.send({ kind: 'choose_entrance', value });
  });
  const children: Child[] = [
    h(
      'div',
      { class: 'field' },
      h('label', { for: 'entrance', class: 'visually-hidden' }, e.label),
      select,
      // Why the choice is locked comes first, next to it.
      running ? h('p', { id: 'entrance-locked', class: 'hint' }, e.locked) : null,
      blocked === null ? null : h('p', { id: 'entrance-blocked', class: 'hint' }, blocked),
      h('p', { id: 'entrance-hint', class: 'hint' }, [entranceGets(current), ...entranceNeeds(current, snapshot)].join(' ')),
    ),
  ];
  if (current === 'app_sound') {
    const locked = running ? text.settings.outputPathLocked : null;
    children.push(
      h(
        'section',
        { class: 'routing-section', 'aria-labelledby': 'discard-title' },
        h('h3', { id: 'discard-title', class: 'routing-section-title' }, e.discard),
        h('p', { class: 'hint' }, e.discardHint),
        discardList(ctx, 'settings-discard', locked),
      ),
    );
  }
  return children;
}

/**
 * The app-sound entrance opens the output shared unless this is on (ADR
 * 0043): exclusive lowers the latency and silences the other apps on that
 * device. The supervisor takes it in any phase and uses it when it turns on.
 */
function exclusive(ctx: Context): HTMLElement {
  const e = text.entrance;
  const sending = ctx.sending?.kind === 'set_app_capture_exclusive' ? ctx.sending.value : null;
  const running = ctx.snapshot.phase.kind === 'running';
  ctx.offer('set_app_capture_exclusive');
  const checkbox = h('input', {
    type: 'checkbox',
    id: 'app-capture-exclusive',
    'data-key': 'app-capture-exclusive',
    'data-intent': 'set_app_capture_exclusive',
    checked: sending ?? captureOf(ctx.snapshot).exclusive,
    disabled: ctx.busy,
    'aria-busy': sending === null ? null : 'true',
    'aria-describedby': described('app-capture-exclusive-hint', running && 'app-capture-exclusive-when'),
  });
  checkbox.addEventListener('change', () => ctx.send({ kind: 'set_app_capture_exclusive', value: checkbox.checked }));
  return h(
    'div',
    { class: 'check exclusive-check' },
    checkbox,
    h('label', { for: 'app-capture-exclusive' }, e.exclusive),
    h('p', { id: 'app-capture-exclusive-hint', class: 'hint' }, e.exclusiveHint),
    running ? h('p', { id: 'app-capture-exclusive-when', class: 'hint' }, e.exclusiveWhen) : null,
  );
}

/**
 * How the engine reaches the DAC: WASAPI exclusive (the default) or kernel
 * streaming with the DAC's endpoint disabled while it runs (ADR 0028). The
 * supervisor takes a change only while the engine is off. The app-sound
 * entrance opens WASAPI, shared unless its switch says exclusive, and never
 * kernel streaming (ADR 0043), so that option stays in view, locked, beside
 * the reason.
 */
function outputPath(ctx: Context): Child[] {
  const t = text.settings;
  const apps = appMode(ctx.snapshot);
  const running = ctx.snapshot.phase.kind === 'running';
  const current = ctx.snapshot.output_path;
  const options: Array<[value: OutputPath, label: string]> = [
    ['wasapi', apps ? text.entrance.wasapi : t.outputPathWasapi],
    ['kernel_streaming', text.meters.kernelStreaming],
  ];
  const hintId = apps ? 'output-path-apps' : 'output-path-hint';
  const select = h('select', {
    id: 'output-path',
    'data-key': 'output-path',
    disabled: running || ctx.busy,
    'aria-describedby': described(running && 'output-path-locked', hintId),
  });
  for (const [value, label] of options) {
    select.append(h('option', { value, selected: value === current, disabled: apps && value === 'kernel_streaming' && value !== current }, label));
  }
  select.addEventListener('change', () => {
    ctx.send({ kind: 'set_output_path', value: select.value as OutputPath });
  });
  return [
    h(
      'div',
      { class: 'field' },
      h('label', { for: 'output-path', class: 'visually-hidden' }, t.outputPath),
      select,
      // Why the choice is locked comes first, next to it.
      running ? h('p', { id: 'output-path-locked', class: 'hint' }, t.outputPathLocked) : null,
      apps
        ? h('p', { id: hintId, class: 'hint' }, text.entrance.ksNotUsed)
        : h('p', { id: hintId, class: 'hint' }, t.outputPathHint),
    ),
    apps ? exclusive(ctx) : null,
  ];
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
  // The app-sound entrance has no endpoints to send apps to (ADR 0043).
  if (appMode(ctx.snapshot)) return [h('p', { class: 'hint' }, text.entrance.driverOnly)];
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

/** The DAW layouts in the contract's order. */
const ASIO_LAYOUT_LIST = Object.keys(ASIO_LAYOUTS) as AsioLayout[];

/** A layout's channels by its name: 7.1.4 is 7 + 1 + 4. */
const layoutChannels = (layout: AsioLayout) => layout.split('.').reduce((sum, part) => sum + Number(part), 0);

/** How much the system path is lowered, as the readout says it: "12 dB", or muted at the end. */
function attenuationText(db: number): string {
  return db >= ASIO_ATTENUATION_MAX ? text.status.muted : `${Math.round(db)} ${text.profile.gainUnit}`;
}

/**
 * The ASIO input (ADR 0025): the switch, the layout the DAW sends, and how
 * much the system sound is lowered while a DAW plays. The supervisor takes
 * each change while the engine runs too. While the input is off the layout
 * and the slider stay in view, locked; a change in flight already stands
 * where it goes.
 */
function asio(ctx: Context): Child[] {
  const t = text.settings;
  const saved = ctx.snapshot.asio ?? ASIO_DEFAULTS;
  const sending = ctx.sending;
  const enabled = sending?.kind === 'set_asio_enabled' ? sending.value : saved.enabled;
  const layout = sending?.kind === 'set_asio_layout' ? sending.value : saved.layout;
  const attenuation =
    sending?.kind === 'set_asio_system_attenuation' ? sending.value : saved.system_attenuation_db;
  const locked = ctx.busy || !enabled;

  const checkbox = h('input', {
    type: 'checkbox',
    id: 'asio-enabled',
    'data-key': 'asio-enabled',
    checked: enabled,
    disabled: ctx.busy,
    'aria-busy': sending?.kind === 'set_asio_enabled' ? 'true' : null,
    'aria-describedby': 'asio-hint asio-limits',
  });
  checkbox.addEventListener('change', () => ctx.send({ kind: 'set_asio_enabled', value: checkbox.checked }));

  const select = h('select', {
    id: 'asio-layout',
    'data-key': 'asio-layout',
    disabled: locked,
    'aria-busy': sending?.kind === 'set_asio_layout' ? 'true' : null,
    'aria-describedby': 'asio-layout-hint',
  });
  for (const value of ASIO_LAYOUT_LIST) {
    select.append(
      h('option', { value, selected: value === layout }, `${value} (${text.format.channels(layoutChannels(value))})`),
    );
  }
  select.addEventListener('change', () => {
    const value = ASIO_LAYOUT_LIST.find((candidate) => candidate === select.value);
    if (value !== undefined) ctx.send({ kind: 'set_asio_layout', value });
  });

  // Drawn like the profile's gain: the readout follows the thumb, and the
  // value is sent once the slider is let go.
  const shown = attenuationText(attenuation);
  const readout = h('output', { class: 'num gain-readout', for: 'asio-attenuation' }, shown);
  const slider = h('input', {
    id: 'asio-attenuation',
    type: 'range',
    min: 0,
    max: ASIO_ATTENUATION_MAX,
    step: 1,
    value: attenuation,
    'data-key': 'asio-attenuation',
    disabled: locked,
    'aria-valuetext': shown,
    'aria-describedby': 'asio-attenuation-hint',
  });
  slider.addEventListener('input', () => {
    const moved = attenuationText(Number(slider.value));
    readout.textContent = moved;
    slider.setAttribute('aria-valuetext', moved);
  });
  // `change` fires when the slider is let go (or a key step ends).
  slider.addEventListener('change', () => ctx.send({ kind: 'set_asio_system_attenuation', value: Number(slider.value) }));

  return [
    h(
      'div',
      { class: 'check' },
      checkbox,
      h('label', { for: 'asio-enabled' }, t.asioEnabled),
      h('p', { id: 'asio-hint', class: 'hint' }, t.asioHint(t.asioDevice)),
      h('p', { id: 'asio-limits', class: 'hint' }, t.asioLimits),
    ),
    h(
      'div',
      { class: 'asio-section' },
      h(
        'div',
        { class: 'field' },
        h('label', { for: 'asio-layout' }, t.asioLayout),
        select,
        h('p', { id: 'asio-layout-hint', class: 'hint' }, t.asioLayoutHint),
      ),
      h(
        'div',
        { class: 'asio-attenuation' },
        h('label', { for: 'asio-attenuation' }, t.asioAttenuation),
        h(
          'div',
          { class: 'gain' },
          h('span', { class: 'gain-scale' }, '0'),
          slider,
          h('span', { class: 'gain-scale' }, `${ASIO_ATTENUATION_MAX}`),
          readout,
        ),
        h('p', { id: 'asio-attenuation-hint', class: 'hint' }, t.asioAttenuationHint),
      ),
    ),
  ];
}

/**
 * One group of settings: its heading on the left and its controls on the
 * right, stacked under the heading in a narrow window. Groups are set apart
 * by space and a hairline, not a box each.
 */
function group(title: string, children: Child[]): HTMLElement {
  return h(
    'section',
    { class: 'panel settings-group' },
    h('h2', { class: 'panel-title' }, title),
    h('div', { class: 'settings-group-body' }, ...children),
  );
}

export function settings(ctx: Context): Drawn {
  const t = text.settings;
  const { platform, driver } = ctx.snapshot;
  const element = h(
    'div',
    { class: 'settings' },
    group(t.startup, [autostart(ctx)]),
    group(t.language, [language(ctx)]),
    group(text.skins.label, [skin(ctx)]),
    group(text.entrance.label, entrance(ctx)),
    group(t.outputPath, outputPath(ctx)),
    group(t.routing, routing(ctx)),
    group(text.asio, asio(ctx)),
    group(t.update, [update(ctx)]),
    group(t.diagnostics, [
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
      group(t.platform, [platform === null ? h('p', { class: 'muted' }, t.unknown) : facts(platformRows(platform))]),
      group(t.driver, [
        driver !== null
          ? facts(driverRows(driver))
          : h('p', { class: 'muted' }, appMode(ctx.snapshot) ? text.entrance.driverOnly : t.unknown),
      ]),
    ),
  );
  return { element };
}
