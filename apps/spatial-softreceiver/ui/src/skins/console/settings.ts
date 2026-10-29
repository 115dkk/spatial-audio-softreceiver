// The settings page: login autostart, language, skin, the entrance sound
// comes in through with the app-sound entrance's discard output (ADR 0043),
// the output path, the entrance per app (tables), the ASIO input, the update,
// diagnostic recording and the PC and driver facts, each a section of
// label | control rows.

import { toUserError } from '../../api';
import type { AsioLayout, Entrance, EntranceKind, Language, OutputPath, RoutedApp, RuleView, Skin } from '../../bindings';
import { type Child, h } from '../../dom';
import { ASIO_ATTENUATION_MAX, ASIO_DEFAULTS, ASIO_LAYOUTS } from '../../guard';
import { described } from '../../screens/common';
import {
  ENTRANCE_KINDS,
  appMode,
  appSoundBlocked,
  captureOf,
  entranceGets,
  entranceName as kindName,
  entranceNeeds,
  entranceOf,
} from '../../screens/entrance';
import { driverRows, platformRows } from '../../screens/facts';
import { text } from '../../text';
import { SKINS, skinOf } from '../names';
import { type Build, act, choose, cls, hint, para, plain, range, row, section, table, toggle } from './kit';
import type { Page } from './overview';
import { discardList } from './setup';

const ENTRANCES: readonly Entrance[] = ['main', 'music', 'pass'];
const entranceName = (entrance: Entrance) => text.endpoints[ENTRANCES.indexOf(entrance)]!.name;

function autostart(b: Build): HTMLElement {
  const { ctx } = b;
  const t = text.settings;
  const native = ctx.shell.native;
  const inFlight = ctx.view.autostartWanted !== null;
  const locked = !native ? t.autostartUnavailable : ctx.view.autostart === null ? t.unknown : inFlight ? text.console.busy : null;
  return row(t.startup, [
    toggle(b, {
      key: 'autostart',
      label: t.autostart,
      on: (ctx.view.autostartWanted ?? ctx.view.autostart) === true,
      locked,
      followsBusy: false,
      busy: inFlight,
      describedBy: 'autostart-hint',
      onToggle: (wanted) => {
        // The view state is live, so an older drawing also sees a change in flight.
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
  ]);
}

function language(b: Build): HTMLElement {
  const { ctx } = b;
  const t = text.settings;
  type Choice = 'auto' | Language;
  return row(
    t.language,
    [
      choose<Choice>(b, {
        id: 'language',
        key: 'language',
        label: t.language,
        options: [
          ['auto', t.languageAuto],
          ['ko', t.languageKo],
          ['en', t.languageEn],
        ],
        current: ctx.snapshot.language_choice ?? 'auto',
        describedBy: 'language-hint',
        onChoose: (value) => ctx.send({ kind: 'set_language', value: value === 'auto' ? null : value }),
      }),
      hint(t.languageHint, 'language-hint'),
    ],
    { for: 'language' },
  );
}

function skin(b: Build): HTMLElement {
  const { ctx } = b;
  const t = text.skins;
  return row(
    t.label,
    [
      choose<Skin>(b, {
        id: 'skin',
        key: 'skin',
        label: t.label,
        options: SKINS.map((value) => [value, t[value]] as const),
        current: skinOf(ctx.snapshot),
        describedBy: 'skin-hint',
        onChoose: (value) => ctx.send({ kind: 'set_skin', value }),
      }),
      hint(t.hint, 'skin-hint'),
    ],
    { for: 'skin' },
  );
}

/**
 * The entrance sound comes in through (ADR 0043): a select whose options are
 * also palette commands, the app-sound one listed with the supervisor's
 * reason where the PC cannot run it, locked while the engine runs. What the
 * chosen one receives and needs stands under it.
 */
function entranceRows(b: Build): HTMLElement[] {
  const { ctx } = b;
  const e = text.entrance;
  const { snapshot } = ctx;
  const current = entranceOf(snapshot);
  const running = snapshot.phase.kind === 'running';
  const blocked = appSoundBlocked(snapshot);
  ctx.offer('choose_entrance');
  const rows: HTMLElement[] = [
    row(
      e.label,
      [
        choose<EntranceKind>(b, {
          id: 'entrance',
          key: 'entrance',
          label: e.label,
          options: ENTRANCE_KINDS.map((kind) => [kind, kindName(kind)] as const),
          current,
          unavailable: new Map<EntranceKind, string>(blocked === null || current === 'app_sound' ? [] : [['app_sound', blocked]]),
          locked: running ? e.locked : null,
          busy: ctx.pending === 'choose_entrance',
          describedBy: described(running && 'entrance-locked', blocked !== null && 'entrance-blocked', 'entrance-hint'),
          onChoose: (value) => ctx.send({ kind: 'choose_entrance', value }),
        }),
        running ? hint(e.locked, 'entrance-locked') : null,
        blocked === null ? null : hint(blocked, 'entrance-blocked', 'cn-measure tone-text-warn'),
        hint([entranceGets(current), ...entranceNeeds(current, snapshot)].join(' '), 'entrance-hint', 'cn-measure'),
      ],
      { for: 'entrance' },
    ),
  ];
  if (current === 'app_sound') {
    rows.push(
      row(e.discard, [
        hint(e.discardHint, null, 'cn-measure'),
        discardList(b, 'settings-discard', running ? text.settings.outputPathLocked : null),
        running ? hint(text.settings.outputPathLocked, null, 'cn-measure') : null,
      ]),
    );
  }
  return rows;
}

function outputPath(b: Build): HTMLElement[] {
  const { ctx } = b;
  const t = text.settings;
  const e = text.entrance;
  const apps = appMode(ctx.snapshot);
  const running = ctx.snapshot.phase.kind === 'running';
  const hintId = apps ? 'output-path-apps' : 'output-path-hint';
  const current = ctx.snapshot.output_path;
  const rows: HTMLElement[] = [
    row(
      t.outputPath,
      [
        choose<OutputPath>(b, {
          id: 'output-path',
          key: 'output-path',
          label: t.outputPath,
          options: [
            ['wasapi', apps ? e.wasapi : t.outputPathWasapi],
            ['kernel_streaming', text.meters.kernelStreaming],
          ],
          current,
          // The app-sound entrance never opens kernel streaming (ADR 0043).
          unavailable: new Map<OutputPath, string>(apps && current !== 'kernel_streaming' ? [['kernel_streaming', e.ksNotUsed]] : []),
          locked: running ? t.outputPathLocked : null,
          describedBy: described(running && 'output-path-locked', hintId),
          onChoose: (value) => ctx.send({ kind: 'set_output_path', value }),
        }),
        running ? hint(t.outputPathLocked, 'output-path-locked') : null,
        hint(apps ? e.ksNotUsed : t.outputPathHint, hintId, 'cn-measure'),
      ],
      { for: 'output-path' },
    ),
  ];
  if (apps) {
    const sending = ctx.sending?.kind === 'set_app_capture_exclusive' ? ctx.sending.value : null;
    ctx.offer('set_app_capture_exclusive');
    rows.push(
      row(text.meters.outputMode, [
        toggle(b, {
          key: 'app-capture-exclusive',
          label: e.exclusive,
          on: sending ?? captureOf(ctx.snapshot).exclusive,
          busy: sending !== null,
          describedBy: described('app-capture-exclusive-hint', running && 'app-capture-exclusive-when'),
          onToggle: (on) => ctx.send({ kind: 'set_app_capture_exclusive', value: on }),
        }),
        hint(e.exclusiveHint, 'app-capture-exclusive-hint', 'cn-measure'),
        running ? hint(e.exclusiveWhen, 'app-capture-exclusive-when', 'cn-measure') : null,
      ]),
    );
  }
  return rows;
}

function appTable(b: Build, apps: RoutedApp[], locked: string | null): HTMLElement {
  const { ctx } = b;
  const t = text.settings;
  type Rule = '' | Entrance;
  const rows = apps.map((app, index) => {
    const nameId = `routing-app-${index}`;
    b.group = app.display_name;
    const select = choose<Rule>(b, {
      id: `routing-rule-${index}`,
      key: `routing-rule-${app.exe}`,
      label: t.routingRule,
      options: [['', t.routingNoRule], ...ENTRANCES.map((entrance) => [entrance, entranceName(entrance)] as const)],
      current: app.rule ?? '',
      locked,
      labelledBy: `${nameId} routing-rule-head`,
      onChoose: (value) => ctx.send({ kind: 'set_app_rule', value: { exe: app.exe, entrance: value === '' ? null : value } }),
    });
    return h(
      'tr',
      null,
      h(
        'th',
        { scope: 'row', class: 'cn-cell-name' },
        h('span', { id: nameId }, app.display_name),
        h('span', { class: 'cn-cell-sub' }, app.exe),
        app.moved ? h('span', { class: 'cn-tag' }, t.routingMoved) : null,
      ),
      h('td', null, app.entrance_now === null ? '-' : entranceName(app.entrance_now)),
      h('td', { class: 'cn-cell-end' }, select),
    );
  });
  b.group = null;
  return h(
    'div',
    { class: 'cn-routing-apps' },
    h('h3', { id: 'routing-apps-title', class: 'cn-sub-title' }, t.routingApps),
    h(
      'div',
      { class: 'cn-table-wrap' },
      h(
        'table',
        { class: 'cn-table', 'aria-labelledby': 'routing-apps-title' },
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
      ),
    ),
  );
}

function ruleTable(b: Build, rules: RuleView[], locked: string | null): HTMLElement {
  const t = text.settings;
  const rows = rules.map((rule, index) => {
    const exeId = `routing-other-${index}`;
    const changed = rule.built_in !== null && rule.entrance !== rule.built_in;
    let end: Child;
    if (rule.built_in !== null && !changed) {
      end = h('span', { class: 'cn-muted' }, t.routingBuiltIn);
    } else {
      b.group = rule.exe;
      end = act(b, changed ? t.routingRestore : t.routingRemove, { kind: 'remove_app_rule', value: rule.exe }, {
        key: `routing-remove-${rule.exe}`,
        variant: 'quiet',
        locked,
        describedBy: exeId,
      });
      b.group = null;
    }
    return h(
      'tr',
      null,
      h('th', { scope: 'row', id: exeId, class: 'cn-cell-name' }, rule.exe),
      h(
        'td',
        null,
        h('span', null, rule.entrance === null ? t.routingNoRule : entranceName(rule.entrance)),
        changed && rule.built_in !== null ? h('span', { class: 'cn-cell-sub' }, t.routingBuiltInWas(entranceName(rule.built_in))) : null,
      ),
      h('td', { class: 'cn-cell-end' }, end),
    );
  });
  return h(
    'div',
    { class: 'cn-routing-others' },
    h('h3', { id: 'routing-others-title', class: 'cn-sub-title' }, t.routingOthers),
    table({ labelledBy: 'routing-others-title', head: [t.routingExe, t.routingRule, null], rows }),
  );
}

function routing(b: Build): Child[] {
  const { ctx } = b;
  const t = text.settings;
  // The app-sound entrance has no endpoints to send apps to (ADR 0043).
  if (appMode(ctx.snapshot)) return [para(text.entrance.driverOnly, 'cn-muted')];
  const view = ctx.snapshot.routing;
  const locked = view.available ? null : t.routingUnavailable;
  const playing = new Set(view.apps.map((app) => app.exe));
  const others = view.rules.filter((rule) => !playing.has(rule.exe));
  const showApps = view.available || view.apps.length > 0;
  return [
    h(
      'div',
      { class: 'cn-rows' },
      row(text.status.stateLabel, [
        toggle(b, {
          key: 'routing-enabled',
          label: t.routingEnabled,
          on: view.enabled,
          locked,
          busy: ctx.pending === 'set_routing_enabled',
          describedBy: described(!view.available && 'routing-unavailable', 'routing-hint'),
          onToggle: (on) => ctx.send({ kind: 'set_routing_enabled', value: on }),
        }),
        view.available ? null : hint(t.routingUnavailable, 'routing-unavailable', 'tone-text-warn'),
        hint(t.routingHint(entranceName('pass'), entranceName('main')), 'routing-hint', 'cn-measure'),
      ]),
    ),
    showApps
      ? view.apps.length === 0
        ? h(
            'div',
            { class: 'cn-routing-apps' },
            h('h3', { id: 'routing-apps-title', class: 'cn-sub-title' }, t.routingApps),
            para(t.routingNoApps, 'cn-muted'),
          )
        : appTable(b, view.apps, locked)
      : null,
    others.length === 0 ? null : ruleTable(b, others, locked),
  ];
}

const ASIO_LAYOUT_LIST = Object.keys(ASIO_LAYOUTS) as AsioLayout[];
const layoutChannels = (layout: AsioLayout) => layout.split('.').reduce((sum, part) => sum + Number(part), 0);
const attenuationText = (db: number) =>
  db >= ASIO_ATTENUATION_MAX ? text.status.muted : `${Math.round(db)} ${text.profile.gainUnit}`;

function asio(b: Build): HTMLElement[] {
  const { ctx } = b;
  const t = text.settings;
  const saved = ctx.snapshot.asio ?? ASIO_DEFAULTS;
  const sending = ctx.sending;
  const enabled = sending?.kind === 'set_asio_enabled' ? sending.value : saved.enabled;
  const layout = sending?.kind === 'set_asio_layout' ? sending.value : saved.layout;
  const attenuation = sending?.kind === 'set_asio_system_attenuation' ? sending.value : saved.system_attenuation_db;
  const off = enabled ? null : text.console.asioOff;
  return [
    row(text.status.stateLabel, [
      toggle(b, {
        key: 'asio-enabled',
        label: t.asioEnabled,
        on: enabled,
        busy: sending?.kind === 'set_asio_enabled',
        describedBy: 'asio-hint asio-limits',
        onToggle: (on) => ctx.send({ kind: 'set_asio_enabled', value: on }),
      }),
      hint(t.asioHint(t.asioDevice), 'asio-hint', 'cn-measure'),
      hint(t.asioLimits, 'asio-limits', 'cn-measure'),
    ]),
    row(
      t.asioLayout,
      [
        choose<AsioLayout>(b, {
          id: 'asio-layout',
          key: 'asio-layout',
          label: t.asioLayout,
          options: ASIO_LAYOUT_LIST.map((value) => [value, `${value} (${text.format.channels(layoutChannels(value))})`] as const),
          current: layout,
          locked: off,
          busy: sending?.kind === 'set_asio_layout',
          describedBy: 'asio-layout-hint',
          onChoose: (value) => ctx.send({ kind: 'set_asio_layout', value }),
        }),
        hint(t.asioLayoutHint, 'asio-layout-hint', 'cn-measure'),
      ],
      { for: 'asio-layout' },
    ),
    row(
      t.asioAttenuation,
      [
        range(b, {
          id: 'asio-attenuation',
          label: t.asioAttenuation,
          min: 0,
          max: ASIO_ATTENUATION_MAX,
          step: 1,
          value: attenuation,
          say: attenuationText,
          minLabel: '0',
          maxLabel: `${ASIO_ATTENUATION_MAX}`,
          locked: off,
          describedBy: 'asio-attenuation-hint',
          onCommit: (value) => ctx.send({ kind: 'set_asio_system_attenuation', value }),
        }),
        hint(t.asioAttenuationHint, 'asio-attenuation-hint', 'cn-measure'),
      ],
      { for: 'asio-attenuation' },
    ),
  ];
}

function update(b: Build): HTMLElement {
  const { ctx } = b;
  const t = text.settings;
  const state = ctx.view.update;
  const controls: Child[] = [];
  if (!ctx.shell.native) {
    controls.push(hint(t.updateUnavailable));
  } else if (state === null) {
    controls.push(hint(t.updateUnknown));
  } else {
    controls.push(para(t.updateCurrent(state.current)));
    if (state.ready === null) {
      controls.push(hint(t.updateNone));
    } else {
      controls.push(para(t.updateReady(state.ready)));
      controls.push(
        h(
          'div',
          { class: 'cn-actions' },
          plain(
            b,
            t.updateApply,
            () => {
              ctx.shell.applyUpdate().catch((reason: unknown) => ctx.report(toUserError(reason)));
            },
            { key: 'update-apply', followsBusy: true },
          ),
        ),
      );
    }
  }
  return row(t.update, controls);
}

function facts(rows: [string, Child][]): HTMLElement[] {
  return rows.map(([label, value]) => row(label, [h('span', { class: 'cn-value' }, value)]));
}

export function settingsPage(b: Build): Page {
  const { snapshot } = b.ctx;
  const t = text.settings;
  const s = (id: string, title: string, children: Child[]) => {
    b.group = title;
    const element = section(b, id, title, children);
    b.group = null;
    return element;
  };
  const element = h(
    'div',
    { class: 'cn-page cn-settings' },
    s('general', text.console.general, [h('div', { class: 'cn-rows' }, autostart(b), language(b), skin(b))]),
    s('entrance', text.entrance.label, [h('div', { class: 'cn-rows' }, ...entranceRows(b))]),
    s('output-path', text.status.output, [h('div', { class: 'cn-rows' }, ...outputPath(b))]),
    s('routing', t.routing, routing(b)),
    s('asio', text.asio, [h('div', { class: 'cn-rows' }, ...asio(b))]),
    s('maintenance', text.console.maintenance, [
      h(
        'div',
        { class: 'cn-rows' },
        update(b),
        row(t.diagnostics, [
          h('div', { class: 'cn-actions' }, act(b, t.record, { kind: 'start_diagnostic_recording' }, { key: 'record', describedBy: 'record-hint' })),
          hint(t.recordHint, 'record-hint', 'cn-measure'),
        ]),
      ),
    ]),
    h(
      'div',
      { class: 'cn-split' },
      s('platform', t.platform, [
        h('div', { class: cls('cn-rows', 'cn-rows-facts') }, ...(snapshot.platform === null ? [para(t.unknown, 'cn-muted')] : facts(platformRows(snapshot.platform)))),
      ]),
      s('driver', t.driver, [
        h(
          'div',
          { class: cls('cn-rows', 'cn-rows-facts') },
          ...(snapshot.driver !== null
            ? facts(driverRows(snapshot.driver))
            : [para(appMode(snapshot) ? text.entrance.driverOnly : t.unknown, 'cn-muted')]),
        ),
      ]),
    ),
  );
  return { element, fill: () => undefined };
}
