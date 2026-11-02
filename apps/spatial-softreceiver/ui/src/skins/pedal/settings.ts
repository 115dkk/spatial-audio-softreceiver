// The settings face (ADR 0046): the on/off options as a bank of DIP
// switches, the entrance and the output path as slide switches, the
// language and the skin as rotary selectors, the input each app plays into
// as rows of lane keys, the ASIO input's layouts as keys in family lines and
// its attenuation as a knob, the update's momentary key, the diagnostic
// recording's key, and what the supervisor knows about this PC and the
// driver, printed. The machine's signal flow is etched in the spare room.
// Rust decides whether a change is taken.

import { toUserError } from '../../api';
import type { AsioLayout, Entrance, EntranceKind, Language, OutputPath, RoutedApp, RuleView } from '../../bindings';
import { type Child, h } from '../../dom';
import { ASIO_ATTENUATION_MAX, ASIO_DEFAULTS, ASIO_LAYOUTS } from '../../guard';
import { text } from '../../text';
import { type Context, type Drawn, described } from '../../screens/common';
import {
  ENTRANCE_KINDS,
  appMode,
  captureOf,
  entranceBlocked,
  entranceGets,
  entranceKey,
  entranceName,
  entranceNeeds,
  entranceOf,
} from '../../screens/entrance';
import { driverRows, platformRows } from '../../screens/facts';
import { SKINS, skinOf } from '../names';
import { discardRow, layoutFamilies } from './controls';
import {
  type Choice,
  type DipRow,
  dip,
  encoder,
  facts,
  fine,
  flowRoom,
  group,
  ikey,
  intentKey,
  keyRow,
  mkey,
  print,
  readout,
  rotary,
  slide,
} from './parts';
import type { Scope } from './scope';

const ENTRANCES: readonly Entrance[] = ['main', 'music', 'pass'];
const ASIO_LAYOUT_LIST = Object.keys(ASIO_LAYOUTS) as AsioLayout[];
/** The eleven ASIO layouts one family a line. */
const ASIO_LAYOUT_LINES = layoutFamilies(ASIO_LAYOUT_LIST);

/** An entrance by its endpoint's name, as Windows shows it. */
const windowsName = (entrance: Entrance) => text.endpoints[ENTRANCES.indexOf(entrance)]!.name;
/** An entrance by the name printed by its jack. */
const shortName = (entrance: Entrance) => text.pedal.lamps[ENTRANCES.indexOf(entrance)] ?? windowsName(entrance);

/** The on/off options: login autostart, the input per app, the ASIO input. */
function options(ctx: Context): Child[] {
  const t = text.settings;
  const native = ctx.shell.native;
  const inFlight = ctx.view.autostartWanted !== null;
  const routing = ctx.snapshot.routing;
  const asio = ctx.snapshot.asio ?? ASIO_DEFAULTS;
  const asioSending = ctx.sending?.kind === 'set_asio_enabled' ? ctx.sending.value : null;
  const rows: DipRow[] = [
    {
      key: 'autostart',
      label: t.autostart,
      on: (ctx.view.autostartWanted ?? ctx.view.autostart) === true,
      off: !native || ctx.view.autostart === null,
      locked: inFlight,
      busy: inFlight,
      describedBy: 'autostart-hint',
      toggle: (wanted) => {
        if (ctx.view.autostartWanted !== null || ctx.view.autostart === null) return;
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
      },
    },
  ];
  // The app-sound entrance has no endpoints to send apps to (ADR 0043).
  if (!appMode(ctx.snapshot)) {
    ctx.offer('set_routing_enabled');
    rows.push({
      key: 'routing-enabled',
      label: t.routingEnabled,
      on: routing.enabled,
      off: !routing.available,
      locked: ctx.busy,
      busy: ctx.pending === 'set_routing_enabled',
      describedBy: described(!routing.available && 'routing-unavailable', 'routing-hint'),
      toggle: (on) => ctx.send({ kind: 'set_routing_enabled', value: on }),
    });
  }
  ctx.offer('set_asio_enabled');
  rows.push({
    key: 'asio-enabled',
    label: t.asioEnabled,
    on: asioSending ?? asio.enabled,
    off: false,
    locked: ctx.busy,
    busy: asioSending !== null,
    describedBy: 'asio-hint asio-limits',
    toggle: (on) => ctx.send({ kind: 'set_asio_enabled', value: on }),
  });
  return [dip(rows, text.facts.on), fine(native ? t.autostartHint : t.autostartUnavailable, 'autostart-hint')];
}

/**
 * The entrance sound comes in through (ADR 0043) as a slide switch; taken
 * only while the engine is off, the app-sound stop off beside the reason
 * where the PC cannot run it. What each receives and what the chosen one
 * needs are printed under it.
 */
function entrance(ctx: Context): Child[] {
  const e = text.entrance;
  const { snapshot } = ctx;
  const current = entranceOf(snapshot);
  const running = snapshot.phase.kind === 'running';
  const other = current === 'driver' ? 'app_sound' : 'driver';
  const blocked = running ? null : entranceBlocked(snapshot, other);
  ctx.offer('choose_entrance');
  const stops: Choice<EntranceKind>[] = ENTRANCE_KINDS.map((kind) => ({
    value: kind,
    label: entranceName(kind),
    key: entranceKey(kind),
    off: kind !== current && blocked !== null,
    describedBy: kind === other && blocked !== null ? 'entrance-blocked' : null,
  }));
  return [
    slide({
      labelledBy: 'pd-entrance-title',
      describedBy: described(running && 'entrance-locked', 'entrance-gets', 'entrance-hint'),
      stops,
      at: current,
      off: running,
      locked: ctx.busy,
      busy: ctx.pending === 'choose_entrance',
      intent: 'choose_entrance',
      choose: (value) => ctx.send({ kind: 'choose_entrance', value }),
    }),
    running ? fine(e.locked, 'entrance-locked') : null,
    blocked === null ? null : h('p', { class: 'pd-fine', id: 'entrance-blocked' }, print(e.unavailable, 'pd-tag'), ' ', blocked),
    h(
      'dl',
      { class: 'pd-facts pd-legend', id: 'entrance-gets' },
      ...ENTRANCE_KINDS.map((kind) => h('div', { class: 'pd-fact' }, h('dt', null, entranceName(kind)), h('dd', null, entranceGets(kind)))),
    ),
    fine(entranceNeeds(current, snapshot).join(' '), 'entrance-hint'),
  ];
}

/**
 * WASAPI exclusive or kernel streaming (ADR 0028) as a slide switch; taken
 * only while the engine is off. The app-sound entrance opens WASAPI, shared
 * unless its exclusive switch says otherwise, and never kernel streaming
 * (ADR 0043): that stop stays off beside the reason.
 */
function outputPath(ctx: Context): Child[] {
  const t = text.settings;
  const e = text.entrance;
  const apps = appMode(ctx.snapshot);
  const running = ctx.snapshot.phase.kind === 'running';
  ctx.offer('set_output_path');
  const current = ctx.snapshot.output_path;
  const hintId = apps ? 'output-path-apps' : 'output-path-hint';
  const stops: Choice<OutputPath>[] = [
    { value: 'wasapi', label: apps ? e.wasapi : t.outputPathWasapi, key: 'output-path-wasapi' },
    {
      value: 'kernel_streaming',
      label: text.meters.kernelStreaming,
      key: 'output-path-kernel_streaming',
      off: apps && current !== 'kernel_streaming',
      describedBy: apps ? hintId : null,
    },
  ];
  const children: Child[] = [
    slide({
      labelledBy: 'pd-output-path-title',
      describedBy: described(running && 'output-path-locked', hintId),
      stops,
      at: current,
      off: running,
      locked: ctx.busy,
      busy: ctx.pending === 'set_output_path',
      intent: 'set_output_path',
      choose: (value) => ctx.send({ kind: 'set_output_path', value }),
    }),
    running ? fine(t.outputPathLocked, 'output-path-locked') : null,
    fine(apps ? e.ksNotUsed : t.outputPathHint, hintId),
  ];
  if (apps) {
    const sending = ctx.sending?.kind === 'set_app_capture_exclusive' ? ctx.sending.value : null;
    ctx.offer('set_app_capture_exclusive');
    children.push(
      dip(
        [
          {
            key: 'app-capture-exclusive',
            label: e.exclusive,
            on: sending ?? captureOf(ctx.snapshot).exclusive,
            off: false,
            locked: ctx.busy,
            busy: sending !== null,
            describedBy: described('app-capture-exclusive-hint', running && 'app-capture-exclusive-when'),
            toggle: (on) => ctx.send({ kind: 'set_app_capture_exclusive', value: on }),
          },
        ],
        text.facts.on,
      ),
      fine(e.exclusiveHint, 'app-capture-exclusive-hint'),
      running ? fine(e.exclusiveWhen, 'app-capture-exclusive-when') : null,
    );
  }
  return children;
}

function language(ctx: Context): Child[] {
  const t = text.settings;
  ctx.offer('set_language');
  const stops: Choice<'auto' | Language>[] = [
    { value: 'auto', label: t.languageAuto, key: 'language-auto' },
    { value: 'ko', label: t.languageKo, key: 'language-ko' },
    { value: 'en', label: t.languageEn, key: 'language-en' },
  ];
  return [
    rotary<'auto' | Language>({
      labelledBy: 'pd-language-title',
      describedBy: 'language-hint',
      stops,
      at: ctx.snapshot.language_choice ?? 'auto',
      off: false,
      locked: ctx.busy,
      busy: ctx.pending === 'set_language',
      intent: 'set_language',
      choose: (value) => ctx.send({ kind: 'set_language', value: value === 'auto' ? null : value }),
    }),
    fine(t.languageHint, 'language-hint'),
  ];
}

/** The skin (ADR 0041): the supervisor saves it and the page reloads in it. */
function skin(ctx: Context): Child[] {
  ctx.offer('set_skin');
  return [
    rotary({
      labelledBy: 'pd-skin-title',
      describedBy: 'skin-hint',
      stops: SKINS.map((value) => ({ value, label: text.skins[value], key: `skin-${value}` })),
      at: skinOf(ctx.snapshot),
      off: false,
      locked: ctx.busy,
      busy: ctx.pending === 'set_skin',
      intent: 'set_skin',
      choose: (value) => ctx.send({ kind: 'set_skin', value }),
    }),
    fine(text.skins.hint, 'skin-hint'),
  ];
}

/** An app with sound now: its name, where it plays, and a key per input, the rule's lit in its colour. */
function appRule(ctx: Context, app: RoutedApp, index: number, locked: boolean): HTMLElement {
  const t = text.settings;
  const nameId = `routing-app-${index}`;
  const sending = ctx.sending?.kind === 'set_app_rule' && ctx.sending.value.exe === app.exe;
  return h(
    'div',
    { class: 'pd-app' },
    h(
      'div',
      { class: 'pd-app-head' },
      h('span', { id: nameId, class: 'pd-app-name' }, app.display_name),
      h('span', { class: 'pd-small' }, app.exe),
      app.moved ? print(t.routingMoved, 'pd-tag') : null,
      h('span', { class: 'pd-small' }, `${t.routingNow} ${app.entrance_now === null ? '-' : shortName(app.entrance_now)}`),
    ),
    h(
      'div',
      { class: 'pd-lanekeys', role: 'group', 'aria-labelledby': nameId },
      ...ENTRANCES.map((entrance, lane) => {
        const on = app.rule === entrance;
        return ikey({
          key: `routing-rule-${app.exe}-${entrance}`,
          label: shortName(entrance),
          lit: on,
          chosen: on,
          tone: `lane-${lane}`,
          size: 'sm',
          off: !ctx.snapshot.routing.available,
          locked,
          busy: sending && on,
          intent: 'set_app_rule',
          attrs: { 'aria-pressed': on ? 'true' : 'false' },
          // Pressing the lit key takes the rule away: the app plays where Windows sends it.
          onClick: () => ctx.send({ kind: 'set_app_rule', value: { exe: app.exe, entrance: on ? null : entrance } }),
        });
      }),
    ),
  );
}

/** A rule for an app without sound now: a built-in one as shipped, changed, or the user's own. */
function otherRule(ctx: Context, rule: RuleView, index: number, locked: boolean): HTMLElement {
  const t = text.settings;
  const exeId = `routing-other-${index}`;
  const changed = rule.built_in !== null && rule.entrance !== rule.built_in;
  return h(
    'div',
    { class: 'pd-app' },
    h(
      'div',
      { class: 'pd-app-head' },
      h('span', { id: exeId, class: 'pd-app-name' }, rule.exe),
      h('span', { class: 'pd-small' }, rule.entrance === null ? t.routingNoRule : shortName(rule.entrance)),
      changed && rule.built_in !== null ? h('span', { class: 'pd-small' }, t.routingBuiltInWas(shortName(rule.built_in))) : null,
    ),
    rule.built_in !== null && !changed
      ? fine(t.routingBuiltIn)
      : intentKey(ctx, changed ? t.routingRestore : t.routingRemove, { kind: 'remove_app_rule', value: rule.exe }, {
          key: `routing-remove-${rule.exe}`,
          off: locked,
          describedBy: exeId,
        }),
  );
}

/** Which input each app plays into (ADR 0028); the switch is in the options bank. */
function routing(ctx: Context): Child[] {
  const t = text.settings;
  if (appMode(ctx.snapshot)) return [fine(text.entrance.driverOnly)];
  const view = ctx.snapshot.routing;
  const locked = ctx.busy || !view.available;
  ctx.offer('set_app_rule');
  const playing = new Set(view.apps.map((app) => app.exe));
  const others = view.rules.filter((rule) => !playing.has(rule.exe));
  const showApps = view.available || view.apps.length > 0;
  return [
    view.available ? null : fine(t.routingUnavailable, 'routing-unavailable'),
    fine(t.routingHint(windowsName('pass'), windowsName('main')), 'routing-hint'),
    showApps
      ? h(
          'div',
          { class: 'pd-apps', role: 'group', 'aria-labelledby': 'routing-apps-title' },
          h('h3', { id: 'routing-apps-title', class: 'pd-label' }, t.routingApps),
          ...(view.apps.length === 0 ? [h('p', { class: 'pd-small' }, t.routingNoApps)] : view.apps.map((app, index) => appRule(ctx, app, index, locked))),
        )
      : null,
    others.length === 0
      ? null
      : h(
          'div',
          { class: 'pd-apps', role: 'group', 'aria-labelledby': 'routing-others-title' },
          h('h3', { id: 'routing-others-title', class: 'pd-label' }, t.routingOthers),
          fine(`${t.routingExe} · ${t.routingRule}`),
          ...others.map((rule, index) => otherRule(ctx, rule, index, locked)),
        ),
  ];
}

const layoutChannels = (layout: AsioLayout) => layout.split('.').reduce((sum, part) => sum + Number(part), 0);

/** How much the system sound is lowered: "12 dB", or muted at the end. */
function attenuationText(db: number): string {
  return db >= ASIO_ATTENUATION_MAX ? text.status.muted : `${Math.round(db)} ${text.profile.gainUnit}`;
}

/** The ASIO input (ADR 0025): the layout the DAW sends, and how much the system sound is lowered. */
function asio(scope: Scope): Child[] {
  const { ctx } = scope;
  const t = text.settings;
  const saved = ctx.snapshot.asio ?? ASIO_DEFAULTS;
  const sending = ctx.sending;
  const enabled = sending?.kind === 'set_asio_enabled' ? sending.value : saved.enabled;
  const layout = sending?.kind === 'set_asio_layout' ? sending.value : saved.layout;
  const attenuation = sending?.kind === 'set_asio_system_attenuation' ? sending.value : saved.system_attenuation_db;
  ctx.offer('set_asio_layout');
  ctx.offer('set_asio_system_attenuation');
  const shown = scope.asioHold.drag ?? attenuation;
  const muted = print(text.status.muted, 'pd-tag');
  muted.hidden = shown < ASIO_ATTENUATION_MAX;
  const digits = readout(2, 22, text.profile.gainUnit, String(Math.round(shown)));
  const follow = (db: number) => {
    digits.set(String(Math.round(db)), attenuationText(db));
    muted.hidden = db < ASIO_ATTENUATION_MAX;
  };
  follow(shown);
  return [
    fine(t.asioHint(t.asioDevice), 'asio-hint'),
    fine(t.asioLimits, 'asio-limits'),
    h('h3', { id: 'asio-layout-label', class: 'pd-label' }, t.asioLayout),
    keyRow({
      labelledBy: 'asio-layout-label',
      describedBy: 'asio-layout-hint',
      choices: ASIO_LAYOUT_LIST.map((value) => ({
        value,
        label: value,
        detail: text.format.channels(layoutChannels(value)),
        key: `asio-layout-${value}`,
      })),
      chosen: layout,
      off: !enabled,
      locked: ctx.busy,
      busy: sending?.kind === 'set_asio_layout',
      intent: 'set_asio_layout',
      lines: ASIO_LAYOUT_LINES,
      size: 'sm',
      choose: (value) => ctx.send({ kind: 'set_asio_layout', value }),
    }),
    fine(t.asioLayoutHint, 'asio-layout-hint'),
    h('h3', { id: 'asio-attenuation-label', class: 'pd-label' }, t.asioAttenuation),
    h(
      'div',
      { class: 'pd-attenuation' },
      encoder({
        key: 'asio-attenuation',
        label: t.asioAttenuation,
        min: 0,
        max: ASIO_ATTENUATION_MAX,
        step: 1,
        settled: attenuation,
        value: shown,
        ring: 0,
        size: 44,
        off: !enabled,
        locked: ctx.busy,
        busy: sending?.kind === 'set_asio_system_attenuation',
        intent: 'set_asio_system_attenuation',
        describedBy: 'asio-attenuation-hint',
        say: attenuationText,
        hold: scope.asioHold,
        turned: follow,
        commit: (value) => ctx.send({ kind: 'set_asio_system_attenuation', value }),
      }),
      h('span', { class: 'pd-attenuation-say' }, digits.element, muted),
    ),
    fine(t.asioAttenuationHint, 'asio-attenuation-hint'),
  ];
}

/** The running version, and the downloaded one with the momentary key that applies it (the app restarts). */
function update(ctx: Context): Child[] {
  const t = text.settings;
  const state = ctx.view.update;
  if (!ctx.shell.native) return [fine(t.updateUnavailable)];
  if (state === null) return [fine(t.updateUnknown)];
  return [
    h('p', { class: 'pd-small pd-ink' }, t.updateCurrent(state.current)),
    ...(state.ready === null
      ? [fine(t.updateNone)]
      : [
          h('p', { class: 'pd-small pd-ink' }, t.updateReady(state.ready)),
          mkey({
            key: 'update-apply',
            label: t.updateApply,
            locked: ctx.busy,
            onClick: () => {
              ctx.shell.applyUpdate().catch((reason: unknown) => ctx.report(toUserError(reason)));
            },
          }),
        ]),
  ];
}

/** Diagnostic recording: an illuminated key whose ring is red while the recording is being started. */
function diagnostics(ctx: Context): Child[] {
  const t = text.settings;
  ctx.offer('start_diagnostic_recording');
  const busy = ctx.pending === 'start_diagnostic_recording';
  return [
    ikey({
      key: 'record',
      label: t.record,
      lit: busy,
      tone: 'fault',
      beside: true,
      locked: ctx.busy,
      busy,
      intent: 'start_diagnostic_recording',
      describedBy: 'record-hint',
      onClick: () => ctx.send({ kind: 'start_diagnostic_recording' }),
    }),
    fine(t.recordHint, 'record-hint'),
  ];
}

export function settingsFace(scope: Scope): Drawn {
  const { ctx } = scope;
  const t = text.settings;
  const { platform, driver } = ctx.snapshot;
  const apps = appMode(ctx.snapshot);
  const element = h(
    'div',
    { class: 'pd-settings' },
    h(
      'div',
      { class: 'pd-pair' },
      h(
        'div',
        { class: 'pd-cols2' },
        h(
          'div',
          { class: 'pd-stack' },
          group('pd-options', text.pedal.options, options(ctx)),
          group('pd-entrance', text.entrance.label, entrance(ctx)),
          group('pd-output-path', t.outputPath, outputPath(ctx)),
        ),
        h('div', { class: 'pd-stack' }, group('pd-language', t.language, language(ctx)), group('pd-skin', text.skins.label, skin(ctx))),
      ),
      flowRoom(),
    ),
    h(
      'div',
      { class: 'pd-stack' },
      group('pd-routing', t.routing, routing(ctx)),
      apps ? group('pd-discard', text.entrance.discard, [fine(text.entrance.discardHint), discardRow(ctx, 'settings-discard', ctx.snapshot.phase.kind === 'running' ? t.outputPathLocked : null)]) : null,
      group('pd-asio', text.asio, asio(scope)),
    ),
    h(
      'div',
      { class: 'pd-stack' },
      group('pd-update', t.update, update(ctx)),
      group('pd-diagnostics', t.diagnostics, diagnostics(ctx)),
      group('pd-platform', t.platform, [platform === null ? fine(t.unknown) : facts(platformRows(platform))]),
      group('pd-driver', t.driver, [driver !== null ? facts(driverRows(driver)) : fine(apps ? text.entrance.driverOnly : t.unknown)]),
    ),
  );
  return { element };
}
