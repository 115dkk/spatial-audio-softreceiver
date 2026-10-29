// The first run (phase `setup`) as a checklist: the entrance, then what that
// entrance needs (ADR 0043). The driver entrance checks Windows support,
// test signing, the driver and the output; the app-sound entrance checks the
// Windows build, the output and the discard output. Each has a status the
// supervisor's step decides, then the current step's own action area. The
// supervisor decides the step; this only names it and offers the intent that
// moves it.

import type { EntranceKind, SetupStep, Snapshot } from '../../bindings';
import { type Child, h } from '../../dom';
import {
  ENTRANCE_KINDS,
  captureOf,
  entranceBlocked,
  entranceGets,
  entranceKey,
  entranceName,
  entranceNeeds,
  entranceOf,
} from '../../screens/entrance';
import { driverRows, platformRows } from '../../screens/facts';
import { text } from '../../text';
import { type Build, act, badge, cls, entrance, hint, laneMark, para, row, section, table, toneGlyph } from './kit';
import type { Page } from './overview';
import { outputList } from './outputs';

type Check = 'done' | 'now' | 'problem' | 'waiting';

/** The checklist row a step works on; the welcome step works on the entrance. */
function current(step: SetupStep, kind: EntranceKind): number {
  if (kind === 'app_sound') {
    switch (step) {
      case 'welcome':
        return 0;
      case 'unsupported':
        return 1;
      case 'choose_output':
        return 2;
      case 'choose_discard':
        return 3;
      default:
        break;
    }
  }
  switch (step) {
    case 'welcome':
      return 0;
    case 'unsupported':
      return 1;
    case 'not_test_signing':
      return 2;
    case 'driver':
    case 'reboot_required':
      return 3;
    case 'choose_output':
    case 'choose_discard':
      return 4;
  }
}

function checkBadge(check: Check): HTMLElement {
  const c = text.console;
  switch (check) {
    case 'done':
      return badge('good', text.wizard.stepDone);
    case 'problem':
      return badge('crit', c.checkProblem);
    case 'waiting':
      return badge('off', c.waiting);
    case 'now':
      return h('span', { class: 'cn-badge tone-now' }, toneGlyph('off'), h('span', null, c.checkNow));
  }
}

const joined = (rows: [string, Child][]) => rows.map(([label, value]) => `${label} ${String(value)}`).join(' · ');

/** The checklist's rows for the entrance chosen: each one's name and what was read. */
function checks(snapshot: Snapshot, step: SetupStep, kind: EntranceKind): [name: string, value: string][] {
  const unknown = text.settings.unknown;
  const f = text.facts;
  const c = text.console;
  const { platform, driver } = snapshot;
  // Nothing is chosen until the welcome step is passed.
  const chosen: [string, string] = [text.entrance.label, step === 'welcome' ? '-' : entranceName(kind)];
  const windows: [string, string] = [
    c.checkWindows,
    platform === null ? unknown : joined([[f.osBuild, platform.os_build], [f.arch, f.archNames[platform.arch]]]),
  ];
  const output: [string, string] = [
    text.status.output,
    snapshot.chosen_output?.name ??
      (snapshot.outputs.length > 0 ? text.console.outputsFound(snapshot.outputs.length) : text.status.noOutput),
  ];
  if (kind === 'app_sound') {
    const view = captureOf(snapshot);
    return [
      chosen,
      windows,
      output,
      [
        text.entrance.discard,
        view.discard?.name ?? (view.discards.length > 0 ? text.console.outputsFound(view.discards.length) : text.entrance.noDiscard),
      ],
    ];
  }
  return [
    chosen,
    windows,
    [
      f.testSigning,
      platform === null
        ? unknown
        : joined([
            [f.testSigning, platform.test_signing ? f.on : f.off],
            [f.secureBoot, f.secureBootNames[platform.secure_boot]],
          ]),
    ],
    [text.settings.driver, driver === null ? unknown : joined(driverRows(driver))],
    output,
  ];
}

function checklist(b: Build, step: SetupStep): HTMLElement {
  const c = text.console;
  const { snapshot } = b.ctx;
  const kind = entranceOf(snapshot);
  const at = current(step, kind);
  // A restart Windows asks for is the step's work, not a fault.
  const failed =
    step === 'unsupported' || step === 'not_test_signing' || (snapshot.error !== null && step !== 'reboot_required');
  const rows = checks(snapshot, step, kind).map(([name, value], index) => {
    const check: Check = index < at ? 'done' : index > at ? 'waiting' : failed ? 'problem' : 'now';
    return h(
      'tr',
      { class: cls(index === at && 'is-current'), 'aria-current': index === at ? 'step' : null },
      h('th', { scope: 'row', class: 'cn-cell-name' }, name),
      h('td', { class: 'cn-cell-state' }, checkBadge(check)),
      h('td', { class: 'cn-cell-value' }, value),
    );
  });
  return section(b, 'checks', c.checkTitle, [
    table({ labelledBy: 'cn-sec-checks-title', head: [c.checkItem, text.status.stateLabel, c.checkValue], rows, className: 'cn-check-table' }),
  ]);
}

function facts(rows: [string, Child][]): HTMLElement {
  return h('div', { class: 'cn-rows cn-rows-facts' }, ...rows.map(([label, value]) => row(label, [h('span', { class: 'cn-value' }, value)])));
}

function endpointNames(): HTMLElement {
  const rows = [0, 1, 2].map((lane) =>
    h(
      'tr',
      null,
      h('th', { scope: 'row', class: 'cn-cell-name' }, h('span', { class: 'cn-entrance' }, laneMark(lane), h('span', null, entrance(lane).name))),
      h('td', null, entrance(lane).purpose),
    ),
  );
  return table({ label: text.status.entrances, head: [text.status.entrances, text.console.purpose], rows });
}

/**
 * The two entrances as a table (law 6): what each receives and needs, and
 * the button that chooses it (also the palette's command of that name). One
 * the PC cannot run is listed with the supervisor's reason, its command too.
 */
function entranceTable(b: Build): HTMLElement {
  const e = text.entrance;
  const { snapshot } = b.ctx;
  const saved = b.group;
  const rows = ENTRANCE_KINDS.map((kind) => {
    const blocked = entranceBlocked(snapshot, kind);
    b.group = e.label;
    const control = act(b, kind === 'driver' ? e.chooseDriver : e.chooseAppSound, { kind: 'choose_entrance', value: kind }, {
      key: entranceKey(kind),
      // Neither entrance is the main action: the two weigh the same.
      variant: 'line',
      locked: blocked,
      describedBy: blocked === null ? null : `${entranceKey(kind)}-blocked`,
    });
    return h(
      'tr',
      { 'data-entrance': kind },
      h(
        'th',
        { scope: 'row', class: 'cn-cell-name' },
        h('span', null, entranceName(kind)),
        blocked === null ? null : badge('warn', e.unavailable, 'cn-cell-badge'),
      ),
      h('td', null, entranceGets(kind)),
      h(
        'td',
        null,
        ...entranceNeeds(kind, snapshot).map((need) => h('span', { class: 'cn-cell-line' }, need)),
        blocked === null ? null : h('span', { id: `${entranceKey(kind)}-blocked`, class: 'cn-cell-sub tone-text-warn' }, blocked),
      ),
      h('td', { class: 'cn-cell-end' }, control),
    );
  });
  b.group = saved;
  return table({ label: e.label, head: [e.label, e.gets, e.needs, null], rows, className: 'cn-entrance-choice' });
}

/** The button that moves to the other entrance, with why it cannot when the PC cannot run that one. */
function switchEntrance(b: Build, kind: EntranceKind): Child[] {
  const blocked = entranceBlocked(b.ctx.snapshot, kind);
  const w = text.wizard;
  return [
    h(
      'div',
      { class: 'cn-actions' },
      act(b, kind === 'driver' ? w.switchToDriver : w.switchToAppSound, { kind: 'choose_entrance', value: kind }, {
        key: entranceKey(kind),
        locked: blocked,
        describedBy: blocked === null ? null : `${entranceKey(kind)}-blocked`,
      }),
    ),
    blocked === null ? null : hint(blocked, `${entranceKey(kind)}-blocked`, 'cn-measure'),
  ];
}

/**
 * The devices that can take the apps' original sound (ADR 0043) as a table
 * (law 6), the one in use badged and a button on each other. With none the
 * band says the supervisor's sentence and this offers the recheck; outside
 * the wizard the sentence stands here. `locked` says why the buttons cannot
 * be used (a running engine).
 */
export function discardList(b: Build, keyPrefix: string, locked: string | null = null): HTMLElement {
  const { discards, discard } = captureOf(b.ctx.snapshot);
  if (discards.length === 0) {
    return b.ctx.snapshot.phase.kind === 'setup'
      ? h('div', { class: 'cn-empty' }, h('div', { class: 'cn-actions' }, act(b, text.banner.recheck, { kind: 'recheck' }, { key: `${keyPrefix}-recheck` })))
      : h('div', { class: 'cn-empty' }, para(text.entrance.noDiscards, 'cn-muted'));
  }
  const saved = b.group;
  const rows = discards.map((output, index) => {
    const current = discard?.id === output.id;
    b.group = `${text.entrance.discard} · ${output.name}`;
    const control = current
      ? badge('good', text.status.currentOutput)
      : act(b, text.wizard.useOutput, { kind: 'choose_discard', value: output.id }, { key: `${keyPrefix}-${index}`, locked });
    return h(
      'tr',
      { class: current ? 'is-current' : null },
      h('th', { scope: 'row', class: 'cn-cell-name' }, output.name),
      h('td', { class: 'cn-cell-end' }, control),
    );
  });
  b.group = saved;
  return h(
    'div',
    { class: 'cn-output-list', 'data-region': 'discard-list' },
    table({ label: text.wizard.discardsLabel, head: [text.wizard.discardsLabel, null], rows }),
  );
}

function stepArea(b: Build, step: SetupStep): HTMLElement {
  const { ctx } = b;
  const { snapshot } = ctx;
  const w = text.wizard;
  const apps = entranceOf(snapshot) === 'app_sound';
  const actions = (...children: Child[]) => h('div', { class: 'cn-actions' }, ...children);
  let title: string;
  let body: Child[];
  switch (step) {
    case 'welcome':
      title = w.welcomeTitle;
      body = [...w.welcomeBody.map((line) => para(line, 'cn-measure')), entranceTable(b)];
      break;
    case 'unsupported':
      title = apps ? w.appUnsupportedTitle : w.unsupportedTitle;
      body = [
        para(apps ? w.appUnsupportedBody : w.unsupportedBody, 'cn-measure'),
        snapshot.platform === null ? null : facts(platformRows(snapshot.platform)),
        ...switchEntrance(b, apps ? 'driver' : 'app_sound'),
      ];
      break;
    case 'not_test_signing':
      title = w.testSigningTitle;
      body = [
        para(w.testSigningBody, 'cn-measure'),
        snapshot.platform === null ? null : facts(platformRows(snapshot.platform)),
        actions(act(b, text.banner.recheck, { kind: 'recheck' }, { key: 'recheck', variant: 'ink' })),
        para(w.testSigningOther, 'cn-measure cn-step-other'),
        ...switchEntrance(b, 'app_sound'),
      ];
      break;
    case 'driver': {
      const again = snapshot.error !== null || snapshot.driver?.kind === 'installed';
      title = w.driverTitle;
      body = [
        para(w.driverBody, 'cn-measure'),
        endpointNames(),
        snapshot.driver === null ? null : facts(driverRows(snapshot.driver)),
        actions(
          act(b, again ? w.driverReinstall : w.driverInstall, { kind: 'install_driver' }, {
            key: 'install',
            variant: 'ink',
            pendingLabel: w.driverInstalling,
            spinner: true,
            describedBy: 'install-hint',
          }),
        ),
        hint(w.driverAdminHint, 'install-hint', 'cn-measure'),
      ];
      break;
    }
    case 'reboot_required':
      title = w.rebootTitle;
      body = [para(w.rebootBody, 'cn-measure'), actions(act(b, text.banner.recheck, { kind: 'recheck' }, { key: 'recheck', variant: 'ink' }))];
      break;
    case 'choose_output':
      title = w.outputTitle;
      body = [para(w.outputBody, 'cn-measure'), outputList(b, 'output')];
      break;
    case 'choose_discard':
      title = w.discardTitle;
      body = [para(w.discardBody, 'cn-measure'), discardList(b, 'discard')];
      break;
  }
  return section(b, 'step', text.console.stepTitle, [h('h3', { class: 'cn-sub-title' }, title), ...body], { className: 'cn-step' });
}

export function setupPage(b: Build, step: SetupStep): Page {
  b.group = text.console.checkTitle;
  const element = h('div', { class: 'cn-page cn-setup' }, checklist(b, step), stepArea(b, step));
  b.group = null;
  return { element, fill: () => undefined };
}
