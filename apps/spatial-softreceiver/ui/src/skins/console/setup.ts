// The first run (phase `setup`) as a checklist: Windows support, test
// signing, the driver and the output, each with a status the supervisor's
// step decides, then the current step's own action area. The supervisor
// decides the step; this only names it and offers the intent that moves it.

import type { SetupStep, Snapshot } from '../../bindings';
import { type Child, h } from '../../dom';
import { driverRows, platformRows } from '../../screens/facts';
import { text } from '../../text';
import { type Build, act, badge, cls, entrance, hint, laneMark, para, row, section, table, toneGlyph } from './kit';
import type { Page } from './overview';
import { outputList } from './outputs';

type Check = 'done' | 'now' | 'problem' | 'waiting';

/** The checklist row a step works on; welcome works on none yet. */
function current(step: SetupStep): number {
  switch (step) {
    case 'welcome':
      return -1;
    case 'unsupported':
      return 0;
    case 'not_test_signing':
      return 1;
    case 'driver':
    case 'reboot_required':
      return 2;
    case 'choose_output':
      return 3;
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

function values(snapshot: Snapshot): string[] {
  const unknown = text.settings.unknown;
  const f = text.facts;
  const { platform, driver } = snapshot;
  return [
    platform === null ? unknown : joined([[f.osBuild, platform.os_build], [f.arch, f.archNames[platform.arch]]]),
    platform === null
      ? unknown
      : joined([
          [f.testSigning, platform.test_signing ? f.on : f.off],
          [f.secureBoot, f.secureBootNames[platform.secure_boot]],
        ]),
    driver === null ? unknown : joined(driverRows(driver)),
    snapshot.chosen_output?.name ??
      (snapshot.outputs.length > 0 ? text.console.outputsFound(snapshot.outputs.length) : text.status.noOutput),
  ];
}

function checklist(b: Build, step: SetupStep): HTMLElement {
  const c = text.console;
  const { snapshot } = b.ctx;
  const at = current(step);
  // A restart Windows asks for is the step's work, not a fault.
  const failed =
    step === 'unsupported' || step === 'not_test_signing' || (snapshot.error !== null && step !== 'reboot_required');
  const names = [c.checkWindows, text.facts.testSigning, text.settings.driver, text.status.output];
  const read = values(snapshot);
  const rows = names.map((name, index) => {
    const check: Check = index < at ? 'done' : index > at ? 'waiting' : failed ? 'problem' : 'now';
    return h(
      'tr',
      { class: cls(index === at && 'is-current'), 'aria-current': index === at ? 'step' : null },
      h('th', { scope: 'row', class: 'cn-cell-name' }, name),
      h('td', { class: 'cn-cell-state' }, checkBadge(check)),
      h('td', { class: 'cn-cell-value' }, read[index] ?? ''),
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

function stepArea(b: Build, step: SetupStep): HTMLElement {
  const { ctx } = b;
  const { snapshot } = ctx;
  const w = text.wizard;
  const actions = (...children: Child[]) => h('div', { class: 'cn-actions' }, ...children);
  let title: string;
  let body: Child[];
  switch (step) {
    case 'welcome':
      title = w.welcomeTitle;
      body = [...w.welcomeBody.map((line) => para(line, 'cn-measure')), actions(act(b, w.begin, { kind: 'begin_setup' }, { key: 'begin', variant: 'ink' }))];
      break;
    case 'unsupported':
      title = w.unsupportedTitle;
      body = [para(w.unsupportedBody, 'cn-measure'), snapshot.platform === null ? null : facts(platformRows(snapshot.platform))];
      break;
    case 'not_test_signing':
      title = w.testSigningTitle;
      body = [
        para(w.testSigningBody, 'cn-measure'),
        snapshot.platform === null ? null : facts(platformRows(snapshot.platform)),
        actions(act(b, text.banner.recheck, { kind: 'recheck' }, { key: 'recheck', variant: 'ink' })),
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
  }
  return section(b, 'step', text.console.stepTitle, [h('h3', { class: 'cn-sub-title' }, title), ...body], { className: 'cn-step' });
}

export function setupPage(b: Build, step: SetupStep): Page {
  b.group = text.console.checkTitle;
  const element = h('div', { class: 'cn-page cn-setup' }, checklist(b, step), stepArea(b, step));
  b.group = null;
  return { element, fill: () => undefined };
}
