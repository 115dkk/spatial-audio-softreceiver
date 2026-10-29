// The settings page in the display, as a receiver's setup menu: login
// autostart, the language, the skin, the entrance sound comes in through and
// the app-sound entrance's discard output, the output path, the input each app
// plays into, the ASIO input, the app's update, diagnostic recording, and
// what the supervisor knows about the PC and the driver. Each setting is a
// list whose chosen row is marked; Rust decides whether a change is taken.

import { toUserError } from '../../api';
import type { AsioLayout, Entrance, EntranceKind, Language, OutputPath, RoutedApp, RuleView } from '../../bindings';
import { type Child, h } from '../../dom';
import { ASIO_ATTENUATION_MAX, ASIO_DEFAULTS, ASIO_LAYOUTS } from '../../guard';
import { text } from '../../text';
import { type Context, type Drawn, described } from '../../screens/common';
import {
  ENTRANCE_KINDS,
  appMode,
  appSoundBlocked,
  captureOf,
  entranceGets,
  entranceKey,
  entranceName,
  entranceNeeds,
  entranceOf,
} from '../../screens/entrance';
import { driverRows, platformRows } from '../../screens/facts';
import { SKINS, skinOf } from '../names';
import type { Scope } from './scope';
import { type Choice, choices, discardRows, facts, hint, intentRow, layoutFamilies, level, line, row, section, toggle } from './ui';

const ENTRANCES: readonly Entrance[] = ['main', 'music', 'pass'];
const ASIO_LAYOUT_LIST = Object.keys(ASIO_LAYOUTS) as AsioLayout[];
/** The eleven ASIO layouts one family a line (ui.ts layoutFamilies). */
const ASIO_LAYOUT_LINES = layoutFamilies(ASIO_LAYOUT_LIST);

/** An entrance by its endpoint's name, as Windows shows it. */
const windowsName = (entrance: Entrance) => text.endpoints[ENTRANCES.indexOf(entrance)]!.name;
/** An entrance by the name printed by its lamp. */
const shortName = (entrance: Entrance) => text.panel.lamps[ENTRANCES.indexOf(entrance)] ?? windowsName(entrance);

/** A setting: its title on the left, its rows on the right. */
const setting = (id: string, title: string, children: Child[]) =>
  section(id, title, [h('div', { class: 'pn-setting-body' }, ...children)], 'pn-setting');

function autostart(ctx: Context): Child[] {
  const t = text.settings;
  const native = ctx.shell.native;
  // A change in flight lives in view state, so a redraw keeps the row locked.
  const inFlight = ctx.view.autostartWanted !== null;
  return [
    toggle({
      key: 'autostart',
      label: t.autostart,
      on: (ctx.view.autostartWanted ?? ctx.view.autostart) === true,
      disabled: !native || ctx.view.autostart === null || inFlight,
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
    }),
    hint(native ? t.autostartHint : t.autostartUnavailable, 'autostart-hint'),
  ];
}

function language(ctx: Context): Child[] {
  const t = text.settings;
  ctx.offer('set_language');
  const options: Choice<'auto' | Language>[] = [
    { value: 'auto', label: t.languageAuto, key: 'language-auto' },
    { value: 'ko', label: t.languageKo, key: 'language-ko' },
    { value: 'en', label: t.languageEn, key: 'language-en' },
  ];
  return [
    choices<'auto' | Language>({
      labelledBy: 'pn-language-title',
      describedBy: 'language-hint',
      options,
      chosen: ctx.snapshot.language_choice ?? 'auto',
      disabled: ctx.busy,
      busy: ctx.pending === 'set_language',
      intent: 'set_language',
      inline: true,
      choose: (value) => ctx.send({ kind: 'set_language', value: value === 'auto' ? null : value }),
    }),
    hint(t.languageHint, 'language-hint'),
  ];
}

/** The skin (ADR 0041): the supervisor saves it and the page reloads in it. */
function skin(ctx: Context): Child[] {
  ctx.offer('set_skin');
  return [
    choices({
      labelledBy: 'pn-skin-title',
      describedBy: 'skin-hint',
      options: SKINS.map((value) => ({ value, label: text.skins[value], key: `skin-${value}` })),
      chosen: skinOf(ctx.snapshot),
      disabled: ctx.busy,
      busy: ctx.pending === 'set_skin',
      intent: 'set_skin',
      inline: true,
      choose: (value) => ctx.send({ kind: 'set_skin', value }),
    }),
    hint(text.skins.hint, 'skin-hint'),
  ];
}

/**
 * The entrance sound comes in through (ADR 0043), a row each with what it
 * receives; taken only while the engine is off, and the app-sound one dark
 * beside the reason where the PC cannot run it. What the chosen one needs
 * stands under the list.
 */
function entrance(ctx: Context): Child[] {
  const e = text.entrance;
  const { snapshot } = ctx;
  const current = entranceOf(snapshot);
  const running = snapshot.phase.kind === 'running';
  const blocked = appSoundBlocked(snapshot);
  ctx.offer('choose_entrance');
  const options: Choice<EntranceKind>[] = ENTRANCE_KINDS.map((kind) => ({
    value: kind,
    label: entranceName(kind),
    key: entranceKey(kind),
    detail: entranceGets(kind),
    tag: kind === 'app_sound' && blocked !== null ? e.unavailable : undefined,
    disabled: kind !== current && kind === 'app_sound' && blocked !== null,
    describedBy: kind === 'app_sound' && blocked !== null ? 'entrance-blocked' : null,
  }));
  return [
    choices({
      labelledBy: 'pn-entrance-title',
      describedBy: described(running && 'entrance-locked', 'entrance-hint'),
      options,
      chosen: current,
      disabled: running || ctx.busy,
      busy: ctx.pending === 'choose_entrance',
      intent: 'choose_entrance',
      choose: (value) => ctx.send({ kind: 'choose_entrance', value }),
    }),
    running ? hint(e.locked, 'entrance-locked') : null,
    blocked === null ? null : hint(blocked, 'entrance-blocked'),
    hint(entranceNeeds(current, snapshot).join(' '), 'entrance-hint'),
  ];
}

/** The app-sound entrance's discard output (ADR 0043); taken while the engine is off. */
function discard(ctx: Context): Child[] {
  const locked = ctx.snapshot.phase.kind === 'running' ? text.settings.outputPathLocked : null;
  return [hint(text.entrance.discardHint), discardRows(ctx, 'settings-discard', locked)];
}

/**
 * WASAPI exclusive or kernel streaming (ADR 0028); taken only while the
 * engine is off. The app-sound entrance opens WASAPI, shared unless its row
 * below says exclusive, and never kernel streaming (ADR 0043): that row stays
 * dark beside the reason.
 */
function outputPath(ctx: Context): Child[] {
  const t = text.settings;
  const e = text.entrance;
  const apps = appMode(ctx.snapshot);
  const running = ctx.snapshot.phase.kind === 'running';
  ctx.offer('set_output_path');
  const current = ctx.snapshot.output_path;
  const hintId = apps ? 'output-path-apps' : 'output-path-hint';
  const options: Choice<OutputPath>[] = [
    { value: 'wasapi', label: apps ? e.wasapi : t.outputPathWasapi, key: 'output-path-wasapi' },
    {
      value: 'kernel_streaming',
      label: text.meters.kernelStreaming,
      key: 'output-path-kernel_streaming',
      tag: apps ? e.unavailable : undefined,
      disabled: apps && current !== 'kernel_streaming',
      describedBy: apps ? hintId : null,
    },
  ];
  const children: Child[] = [
    choices({
      labelledBy: 'pn-output-path-title',
      describedBy: described(running && 'output-path-locked', hintId),
      options,
      chosen: current,
      disabled: running || ctx.busy,
      busy: ctx.pending === 'set_output_path',
      intent: 'set_output_path',
      choose: (value) => ctx.send({ kind: 'set_output_path', value }),
    }),
    running ? hint(t.outputPathLocked, 'output-path-locked') : null,
    hint(apps ? e.ksNotUsed : t.outputPathHint, hintId),
  ];
  if (apps) {
    const sending = ctx.sending?.kind === 'set_app_capture_exclusive' ? ctx.sending.value : null;
    ctx.offer('set_app_capture_exclusive');
    children.push(
      toggle({
        key: 'app-capture-exclusive',
        label: e.exclusive,
        on: sending ?? captureOf(ctx.snapshot).exclusive,
        disabled: ctx.busy,
        busy: sending !== null,
        describedBy: described('app-capture-exclusive-hint', running && 'app-capture-exclusive-when'),
        toggle: (on) => ctx.send({ kind: 'set_app_capture_exclusive', value: on }),
      }),
      hint(e.exclusiveHint, 'app-capture-exclusive-hint'),
      running ? hint(e.exclusiveWhen, 'app-capture-exclusive-when') : null,
    );
  }
  return children;
}

/** An app with sound now: where it plays, and the input to send it to. */
function appRule(ctx: Context, app: RoutedApp, index: number, locked: boolean): HTMLElement {
  const t = text.settings;
  const nameId = `routing-app-${index}`;
  const ruleId = `routing-rule-${index}`;
  return h(
    'div',
    { class: 'pn-app' },
    h(
      'p',
      { class: 'pn-app-head' },
      h('span', { id: nameId, class: 'pn-app-name' }, app.display_name),
      h('span', { class: 'pn-app-exe' }, app.exe),
      app.moved ? h('span', { class: 'pn-tag' }, t.routingMoved) : null,
    ),
    facts([[t.routingNow, app.entrance_now === null ? '-' : shortName(app.entrance_now)]]),
    h('p', { id: ruleId, class: 'pn-sub-title' }, t.routingRule),
    choices<Entrance | null>({
      labelledBy: `${nameId} ${ruleId}`,
      options: [
        { value: null, label: t.routingNoRule, key: `routing-rule-${app.exe}-none` },
        ...ENTRANCES.map((entrance, lane) => ({ value: entrance, label: shortName(entrance), key: `routing-rule-${app.exe}-${entrance}`, lane })),
      ],
      chosen: app.rule,
      disabled: locked,
      busy: ctx.sending?.kind === 'set_app_rule' && ctx.sending.value.exe === app.exe,
      intent: 'set_app_rule',
      inline: true,
      choose: (entrance) => ctx.send({ kind: 'set_app_rule', value: { exe: app.exe, entrance } }),
    }),
  );
}

/** A rule for an app without sound now: a built-in one as shipped, changed, or the user's own. */
function otherRule(ctx: Context, rule: RuleView, index: number, locked: boolean): HTMLElement {
  const t = text.settings;
  const exeId = `routing-other-${index}`;
  const changed = rule.built_in !== null && rule.entrance !== rule.built_in;
  return h(
    'div',
    { class: 'pn-app' },
    h(
      'p',
      { class: 'pn-app-head' },
      h('span', { id: exeId, class: 'pn-app-name' }, rule.exe),
      h('span', { class: 'pn-app-exe' }, rule.entrance === null ? t.routingNoRule : shortName(rule.entrance)),
      changed && rule.built_in !== null ? h('span', { class: 'pn-app-exe' }, t.routingBuiltInWas(shortName(rule.built_in))) : null,
    ),
    rule.built_in !== null && !changed
      ? hint(t.routingBuiltIn)
      : intentRow(ctx, changed ? t.routingRestore : t.routingRemove, { kind: 'remove_app_rule', value: rule.exe }, {
          key: `routing-remove-${rule.exe}`,
          disabled: locked,
          describedBy: exeId,
        }),
  );
}

/** Which input each app plays into (ADR 0028); locked beside the reason where Windows cannot do it. */
function routing(ctx: Context): Child[] {
  const t = text.settings;
  // The app-sound entrance has no endpoints to send apps to (ADR 0043).
  if (appMode(ctx.snapshot)) return [hint(text.entrance.driverOnly)];
  const view = ctx.snapshot.routing;
  const locked = ctx.busy || !view.available;
  ctx.offer('set_routing_enabled');
  ctx.offer('set_app_rule');
  const playing = new Set(view.apps.map((app) => app.exe));
  const others = view.rules.filter((rule) => !playing.has(rule.exe));
  const showApps = view.available || view.apps.length > 0;
  return [
    toggle({
      key: 'routing-enabled',
      label: t.routingEnabled,
      on: view.enabled,
      disabled: locked,
      busy: ctx.pending === 'set_routing_enabled',
      describedBy: described(!view.available && 'routing-unavailable', 'routing-hint'),
      toggle: (on) => ctx.send({ kind: 'set_routing_enabled', value: on }),
    }),
    view.available ? null : hint(t.routingUnavailable, 'routing-unavailable'),
    hint(t.routingHint(windowsName('pass'), windowsName('main')), 'routing-hint'),
    showApps
      ? h(
          'div',
          { class: 'pn-apps', role: 'group', 'aria-labelledby': 'routing-apps-title' },
          h('h3', { id: 'routing-apps-title', class: 'pn-sub-title' }, t.routingApps),
          ...(view.apps.length === 0 ? [line(t.routingNoApps)] : view.apps.map((app, index) => appRule(ctx, app, index, locked))),
        )
      : null,
    others.length === 0
      ? null
      : h(
          'div',
          { class: 'pn-apps', role: 'group', 'aria-labelledby': 'routing-others-title' },
          h('h3', { id: 'routing-others-title', class: 'pn-sub-title' }, t.routingOthers),
          h('p', { class: 'pn-hint' }, `${t.routingExe} · ${t.routingRule}`),
          ...others.map((rule, index) => otherRule(ctx, rule, index, locked)),
        ),
  ];
}

const layoutChannels = (layout: AsioLayout) => layout.split('.').reduce((sum, part) => sum + Number(part), 0);

/** How much the system sound is lowered: "12 dB", or muted at the end. */
function attenuationText(db: number): string {
  return db >= ASIO_ATTENUATION_MAX ? text.status.muted : `${Math.round(db)} ${text.profile.gainUnit}`;
}

/** The ASIO input (ADR 0025): the switch, the layout the DAW sends, how much the system sound is lowered. */
function asio(ctx: Context): Child[] {
  const t = text.settings;
  const saved = ctx.snapshot.asio ?? ASIO_DEFAULTS;
  const sending = ctx.sending;
  const enabled = sending?.kind === 'set_asio_enabled' ? sending.value : saved.enabled;
  const layout = sending?.kind === 'set_asio_layout' ? sending.value : saved.layout;
  const attenuation = sending?.kind === 'set_asio_system_attenuation' ? sending.value : saved.system_attenuation_db;
  const locked = ctx.busy || !enabled;
  ctx.offer('set_asio_enabled');
  ctx.offer('set_asio_layout');
  ctx.offer('set_asio_system_attenuation');
  return [
    toggle({
      key: 'asio-enabled',
      label: t.asioEnabled,
      on: enabled,
      disabled: ctx.busy,
      busy: sending?.kind === 'set_asio_enabled',
      describedBy: 'asio-hint asio-limits',
      toggle: (on) => ctx.send({ kind: 'set_asio_enabled', value: on }),
    }),
    hint(t.asioHint(t.asioDevice), 'asio-hint'),
    hint(t.asioLimits, 'asio-limits'),
    h('h3', { id: 'asio-layout-label', class: 'pn-sub-title' }, t.asioLayout),
    choices({
      labelledBy: 'asio-layout-label',
      describedBy: 'asio-layout-hint',
      options: ASIO_LAYOUT_LIST.map((value) => ({
        value,
        label: value,
        detail: text.format.channels(layoutChannels(value)),
        key: `asio-layout-${value}`,
      })),
      chosen: layout,
      disabled: locked,
      busy: sending?.kind === 'set_asio_layout',
      intent: 'set_asio_layout',
      inline: true,
      lines: ASIO_LAYOUT_LINES,
      choose: (value) => ctx.send({ kind: 'set_asio_layout', value }),
    }),
    hint(t.asioLayoutHint, 'asio-layout-hint'),
    h('h3', { class: 'pn-sub-title' }, t.asioAttenuation),
    level({
      key: 'asio-attenuation',
      label: t.asioAttenuation,
      min: 0,
      max: ASIO_ATTENUATION_MAX,
      step: 1,
      value: attenuation,
      disabled: locked,
      describedBy: 'asio-attenuation-hint',
      say: attenuationText,
      commit: (value) => ctx.send({ kind: 'set_asio_system_attenuation', value }),
    }),
    hint(t.asioAttenuationHint, 'asio-attenuation-hint'),
  ];
}

/** The running version, and the downloaded one with the row that applies it. */
function update(ctx: Context): Child[] {
  const t = text.settings;
  const state = ctx.view.update;
  if (!ctx.shell.native) return [hint(t.updateUnavailable)];
  if (state === null) return [hint(t.updateUnknown)];
  return [
    line(t.updateCurrent(state.current)),
    ...(state.ready === null
      ? [hint(t.updateNone)]
      : [
          line(t.updateReady(state.ready)),
          row({ key: 'update-apply', label: t.updateApply, disabled: ctx.busy }, () => {
            ctx.shell.applyUpdate().catch((reason: unknown) => ctx.report(toUserError(reason)));
          }),
        ]),
  ];
}

export function settingsPage(scope: Scope): Drawn {
  const { ctx } = scope;
  const t = text.settings;
  const { platform, driver } = ctx.snapshot;
  const element = h(
    'div',
    { class: 'pn-settings' },
    setting('pn-startup', t.startup, autostart(ctx)),
    setting('pn-language', t.language, language(ctx)),
    setting('pn-skin', text.skins.label, skin(ctx)),
    setting('pn-entrance', text.entrance.label, entrance(ctx)),
    appMode(ctx.snapshot) ? setting('pn-discard', text.entrance.discard, discard(ctx)) : null,
    setting('pn-output-path', t.outputPath, outputPath(ctx)),
    setting('pn-routing', t.routing, routing(ctx)),
    setting('pn-asio', text.asio, asio(ctx)),
    setting('pn-update', t.update, update(ctx)),
    setting('pn-diagnostics', t.diagnostics, [
      hint(t.recordHint),
      intentRow(ctx, t.record, { kind: 'start_diagnostic_recording' }, { key: 'record' }),
    ]),
    setting('pn-platform', t.platform, [platform === null ? hint(t.unknown) : facts(platformRows(platform))]),
    setting('pn-driver', t.driver, [
      driver !== null ? facts(driverRows(driver)) : hint(appMode(ctx.snapshot) ? text.entrance.driverOnly : t.unknown),
    ]),
  );
  return { element };
}
