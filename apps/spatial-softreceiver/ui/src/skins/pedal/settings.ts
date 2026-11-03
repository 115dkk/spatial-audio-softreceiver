// The settings face (ADR 0046): the on/off options as a bank of DIP
// switches, the entrance and the output path as slide switches, the
// language and the skin as rotary selectors, the input each app plays into
// as a routing matrix (an app a row, an input a column, an illuminated key
// at each crossing), the ASIO input's layouts as keys in family lines and
// its attenuation as a knob, the update's momentary key, the diagnostic
// recording's key, and what the supervisor knows about this PC and the
// driver on spec plates. A group's sentences are parted from its parts by
// printed hairlines, and the entrance switch prints only the chosen
// entrance's description, in a caption its stop's leader runs to; a
// caution stands on the face's legend plate by its number (법칙 5).
// The machine's signal flow is etched in the spare room. Rust decides
// whether a change is taken.

import { toUserError } from '../../api';
import type { AsioLayout, Entrance, EntranceKind, Language, OutputPath } from '../../bindings';
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
  type NotePlate,
  dip,
  encoder,
  fine,
  flowRoom,
  group,
  hairline,
  ikey,
  keyRow,
  led,
  mkey,
  notePlate,
  print,
  readout,
  rotary,
  slide,
  spec,
  sr,
  tape,
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

/**
 * The on/off options: login autostart, the input per app, the ASIO input.
 * Where this PC cannot send apps to an input, the routing position carries a
 * numbered mark and the sentence stands on the face's legend plate (법칙 5).
 */
function options(ctx: Context, plate: NotePlate): Child[] {
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
    const unavailable = plate.note('wait', { id: 'routing-unavailable' });
    unavailable.set(routing.available ? null : t.routingUnavailable);
    rows.push({
      key: 'routing-enabled',
      label: t.routingEnabled,
      on: routing.enabled,
      off: !routing.available,
      locked: ctx.busy,
      busy: ctx.pending === 'set_routing_enabled',
      describedBy: described(!routing.available && 'routing-unavailable', 'routing-hint'),
      mark: unavailable.mark,
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
  return [dip(rows, text.facts.on), hairline(), fine(native ? t.autostartHint : t.autostartUnavailable, 'autostart-hint')];
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
  // Only the chosen entrance's description is printed; the other's stays with its stop for a screen reader.
  const getsOf = (kind: EntranceKind) => `entrance-gets-${kind}`;
  const stops: Choice<EntranceKind>[] = ENTRANCE_KINDS.map((kind) => ({
    value: kind,
    label: entranceName(kind),
    key: entranceKey(kind),
    off: kind !== current && blocked !== null,
    describedBy: kind === current ? null : described(kind === other && blocked !== null && 'entrance-blocked', getsOf(kind)),
  }));
  const control = slide({
    labelledBy: 'pd-entrance-title',
    describedBy: described(running && 'entrance-locked', 'entrance-gets', 'entrance-hint'),
    stops,
    at: current,
    off: running,
    locked: ctx.busy,
    busy: ctx.pending === 'choose_entrance',
    intent: 'choose_entrance',
    choose: (value) => ctx.send({ kind: 'choose_entrance', value }),
  });
  control.classList.add('pd-leads');
  return [
    control,
    h('p', { class: 'pd-callout', id: 'entrance-gets', 'data-chosen': current }, entranceGets(current)),
    ...ENTRANCE_KINDS.filter((kind) => kind !== current).map((kind) => sr(entranceGets(kind), getsOf(kind))),
    running ? fine(e.locked, 'entrance-locked') : null,
    blocked === null ? null : h('p', { class: 'pd-fine', id: 'entrance-blocked' }, print(e.unavailable, 'pd-tag'), ' ', blocked),
    hairline(),
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
    hairline(),
    fine(apps ? e.ksNotUsed : t.outputPathHint, hintId),
  ];
  if (apps) {
    const sending = ctx.sending?.kind === 'set_app_capture_exclusive' ? ctx.sending.value : null;
    ctx.offer('set_app_capture_exclusive');
    children.push(
      hairline(),
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
    hairline(),
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
    hairline(),
    fine(text.skins.hint, 'skin-hint'),
  ];
}

/** One row of the routing matrix: an app with sound now, or a rule saved for one without. */
interface MatrixRow {
  exe: string;
  /** The name on its tape or in print: the app's own name, or the program file. */
  name: string;
  rule: Entrance | null;
  /** The factory's rule for this program, if one ships. */
  builtIn: Entrance | null;
  /** Where the app plays now; undefined for a saved rule of an app without sound now. */
  now?: Entrance | null | undefined;
  moved?: boolean | undefined;
  /** The program file as well, when the name is the app's own. */
  file?: string | undefined;
}

/**
 * A row of the matrix: the app's name on label-maker tape (a name this PC
 * gives), or in the factory's print where a built-in rule ships for it, with
 * a printed mark under its factory input. A key per input whose ring is lit
 * in that input's colour on the app's rule; pressing the lit key takes the
 * rule away, another moves it. While the app has sound, the key of the
 * input it plays into now glows dim where no rule lights it. At the row's
 * end a small momentary key: back to the factory's rule where it was
 * changed, or away with a rule of the user's own. What the eye does not read (the program
 * file, the input now, the factory's rule) is said to a screen reader.
 */
function matrixRow(ctx: Context, row: MatrixRow, id: string, locked: boolean): HTMLElement {
  const t = text.settings;
  const available = ctx.snapshot.routing.available;
  const factory = row.builtIn !== null;
  const changed = factory && row.rule !== row.builtIn;
  const sending = ctx.sending?.kind === 'set_app_rule' && ctx.sending.value.exe === row.exe ? ctx.sending.value : null;
  const said = [
    row.file,
    row.now === undefined ? null : `${t.routingNow} ${row.now === null ? '-' : shortName(row.now)}`,
    row.moved ? t.routingMoved : null,
    row.rule === null ? t.routingNoRule : null,
    row.builtIn === null ? null : changed ? t.routingBuiltInWas(shortName(row.builtIn)) : t.routingBuiltIn,
  ].filter((words): words is string => words !== null && words !== undefined);
  const cells = ENTRANCES.map((entrance, lane) => {
    const on = row.rule === entrance;
    const key = ikey({
      key: `routing-rule-${row.exe}-${entrance}`,
      label: shortName(entrance),
      bare: true,
      lit: on,
      dim: row.now === entrance,
      chosen: on,
      tone: `lane-${lane}`,
      size: 'sm',
      off: !available,
      locked,
      busy: sending !== null && (sending.entrance === entrance || (sending.entrance === null && on)),
      intent: 'set_app_rule',
      describedBy: id,
      attrs: { 'aria-pressed': on ? 'true' : 'false' },
      // Pressing the lit key takes the rule away: the app plays where Windows sends it.
      onClick: () => ctx.send({ kind: 'set_app_rule', value: { exe: row.exe, entrance: on ? null : entrance } }),
    });
    return h('td', { class: `pd-mx-cell${row.builtIn === entrance ? ' is-factory' : ''}` }, key);
  });
  // Back to the factory's rule where it was changed; away with a rule of the user's own.
  let end: HTMLElement | null = null;
  if (changed || (!factory && row.rule !== null)) {
    ctx.offer('remove_app_rule');
    end = mkey({
      key: `routing-remove-${row.exe}`,
      label: changed ? t.routingRestore : t.routingRemove,
      size: 'sm',
      off: locked,
      locked: ctx.busy,
      busy: ctx.sending?.kind === 'remove_app_rule' && ctx.sending.value === row.exe,
      intent: 'remove_app_rule',
      describedBy: id,
      onClick: () => ctx.send({ kind: 'remove_app_rule', value: row.exe }),
    });
  }
  return h(
    'tr',
    null,
    h(
      'th',
      { scope: 'row', class: 'pd-mx-name', id },
      factory ? h('span', { class: 'pd-silk' }, row.name) : tape(row.name),
      said.length === 0 ? null : sr(` · ${said.join(' · ')}`),
    ),
    ...cells,
    h('td', { class: 'pd-mx-end' }, end),
  );
}

/** A printed section of the matrix: its name across the row, then its rows. */
function matrixPart(title: string, id: string, rows: HTMLElement[]): HTMLElement {
  return h(
    'tbody',
    { 'aria-labelledby': id },
    h('tr', { class: 'pd-mx-part' }, h('th', { scope: 'colgroup', colspan: 5, id }, title)),
    ...rows,
  );
}

/**
 * Which input each app plays into (ADR 0028) as a routing matrix (법칙 2):
 * a row per app, the apps with sound now first and then the rules saved for
 * others, a column per input with its name printed once at the head, and in
 * each crossing an illuminated key. The switch is in the options bank, and
 * what routing does is the group's description, for a screen reader.
 */
function routing(ctx: Context): Child[] {
  const t = text.settings;
  if (appMode(ctx.snapshot)) return [fine(text.entrance.driverOnly)];
  const view = ctx.snapshot.routing;
  const locked = ctx.busy || !view.available;
  ctx.offer('set_app_rule');
  const playing = new Set(view.apps.map((app) => app.exe));
  const saved = new Map(view.rules.map((rule) => [rule.exe, rule]));
  const others = view.rules.filter((rule) => !playing.has(rule.exe));
  // The supervisor lists no app where it cannot route them; "none playing" would then be a guess.
  const showApps = view.available || view.apps.length > 0;
  const parts: HTMLElement[] = [];
  if (showApps) {
    const rows =
      view.apps.length === 0
        ? [h('tr', null, h('td', { colspan: 5, class: 'pd-small pd-mx-none' }, t.routingNoApps))]
        : view.apps.map((app, index) =>
            matrixRow(
              ctx,
              {
                exe: app.exe,
                name: app.display_name === '' ? app.exe : app.display_name,
                rule: app.rule,
                builtIn: saved.get(app.exe)?.built_in ?? null,
                now: app.entrance_now,
                moved: app.moved,
                file: app.display_name === '' || app.display_name === app.exe ? undefined : app.exe,
              },
              `routing-app-${index}`,
              locked,
            ),
          );
    parts.push(matrixPart(t.routingApps, 'routing-apps-title', rows));
  }
  if (others.length > 0) {
    parts.push(
      matrixPart(
        t.routingOthers,
        'routing-others-title',
        others.map((rule, index) =>
          matrixRow(ctx, { exe: rule.exe, name: rule.exe, rule: rule.entrance, builtIn: rule.built_in }, `routing-other-${index}`, locked),
        ),
      ),
    );
  }
  return [
    sr(t.routingHint(windowsName('pass'), windowsName('main')), 'routing-hint'),
    parts.length === 0
      ? null
      : h(
          'table',
          { class: 'pd-matrix', 'aria-labelledby': 'pd-routing-title' },
          h(
            'thead',
            null,
            h('tr', null, h('td', null), ...ENTRANCES.map((entrance) => h('th', { scope: 'col' }, shortName(entrance))), h('td', null)),
          ),
          ...parts,
        ),
    // What a lit and a dim ring say, printed once under the matrix as a panel prints its lamps' legend.
    view.apps.length > 0
      ? h(
          'p',
          { class: 'pd-mx-key', 'aria-hidden': 'true' },
          led('white', 'on', 'sm').element,
          print(t.routingRule, 'pd-small'),
          led('white', 'dim', 'sm').element,
          print(t.routingNow, 'pd-small'),
        )
      : null,
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
    hairline(),
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
    hairline(),
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
    hairline(),
    fine(t.recordHint, 'record-hint'),
  ];
}

export function settingsFace(scope: Scope): Drawn {
  const { ctx } = scope;
  const t = text.settings;
  const { platform, driver } = ctx.snapshot;
  const apps = appMode(ctx.snapshot);
  const plate = notePlate(text.banner.notesLabel, 'pd-settings-notes');
  const optionParts = options(ctx, plate);
  plate.renumber();
  const routingGroup = group('pd-routing', t.routing, routing(ctx));
  if (!apps) routingGroup.setAttribute('aria-describedby', 'routing-hint');
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
          group('pd-options', text.pedal.options, optionParts),
          group('pd-entrance', text.entrance.label, entrance(ctx)),
          group('pd-output-path', t.outputPath, outputPath(ctx)),
          plate.element,
        ),
        h('div', { class: 'pd-stack' }, group('pd-language', t.language, language(ctx)), group('pd-skin', text.skins.label, skin(ctx))),
      ),
      flowRoom(),
    ),
    h(
      'div',
      { class: 'pd-stack' },
      routingGroup,
      apps ? group('pd-discard', text.entrance.discard, [fine(text.entrance.discardHint), hairline(), discardRow(ctx, 'settings-discard', ctx.snapshot.phase.kind === 'running' ? t.outputPathLocked : null)]) : null,
      group('pd-asio', text.asio, asio(scope)),
    ),
    h(
      'div',
      { class: 'pd-stack' },
      group('pd-update', t.update, update(ctx)),
      group('pd-diagnostics', t.diagnostics, diagnostics(ctx)),
      group('pd-platform', t.platform, [platform === null ? fine(t.unknown) : spec(platformRows(platform))]),
      group('pd-driver', t.driver, [driver !== null ? spec(driverRows(driver)) : fine(apps ? text.entrance.driverOnly : t.unknown)]),
    ),
  );
  return { element };
}
