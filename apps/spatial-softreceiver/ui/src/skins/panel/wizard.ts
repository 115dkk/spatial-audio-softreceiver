// The first-run wizard as a step screen in the display (ADR 0041, 전면 패널의
// 법칙 5): the steps along the top with the current one marked, the step's
// sentences and facts, and its action rows. The first step lists the two
// entrances (ADR 0043), each with what it receives and needs and the row that
// chooses it; the steps then follow the entrance chosen. The supervisor
// decides the step; the screen only explains it and offers the intent that
// moves it on. After quitting the display says what the standard closing
// page says.

import type { EntranceKind, SetupStep } from '../../bindings';
import { type Child, h } from '../../dom';
import { text } from '../../text';
import type { Context, Drawn } from '../../screens/common';
import {
  ENTRANCE_KINDS,
  entranceBlocked,
  entranceGets,
  entranceKey,
  entranceName,
  entranceNeeds,
  entranceOf,
  stepIndex,
  stepLabels,
} from '../../screens/entrance';
import { driverRows, platformRows } from '../../screens/facts';
import { discardRows, facts, hint, intentRow, line, outputRows } from './ui';

function steps(labels: readonly string[], current: number): HTMLElement {
  return h(
    'ol',
    { class: 'pn-steps', 'aria-label': text.wizard.stepsLabel },
    ...labels.map((label, index) => {
      const state = index < current ? 'done' : index === current ? 'current' : 'upcoming';
      return h(
        'li',
        { class: `pn-step is-${state}`, 'aria-current': state === 'current' ? 'step' : null },
        h('span', { class: 'pn-mark', 'aria-hidden': 'true' }),
        h('span', null, label),
        state === 'done' ? h('span', { class: 'pn-sr' }, ` (${text.wizard.stepDone})`) : null,
      );
    }),
  );
}

const paragraphs = (...sentences: string[]) => sentences.map((sentence) => line(sentence, 'pn-body'));

function platformFacts(ctx: Context): HTMLElement | null {
  return ctx.snapshot.platform === null ? null : facts(platformRows(ctx.snapshot.platform));
}

/**
 * The row that chooses an entrance, with the reason under it when the PC
 * cannot run that one (the row then stays dark).
 */
function entranceRow(ctx: Context, kind: EntranceKind, label: string): Child[] {
  const blocked = entranceBlocked(ctx.snapshot, kind);
  const reasonId = `${entranceKey(kind)}-blocked`;
  return [
    intentRow(ctx, label, { kind: 'choose_entrance', value: kind }, {
      key: entranceKey(kind),
      disabled: blocked !== null,
      describedBy: blocked === null ? null : reasonId,
    }),
    blocked === null ? null : hint(blocked, reasonId),
  ];
}

/** An entrance at the first step: its name, what it receives and needs, and its row. */
function entranceBlock(ctx: Context, kind: EntranceKind): HTMLElement {
  const e = text.entrance;
  const blocked = entranceBlocked(ctx.snapshot, kind) !== null;
  return h(
    'div',
    { class: `pn-entrance${blocked ? ' is-blocked' : ''}`, 'data-entrance': kind },
    h(
      'h2',
      { class: 'pn-sub-title pn-entrance-name' },
      entranceName(kind),
      blocked ? h('span', { class: 'pn-tag' }, e.unavailable) : null,
    ),
    facts([
      [e.gets, entranceGets(kind)],
      [e.needs, h('span', null, ...entranceNeeds(kind, ctx.snapshot).map((need) => h('span', { class: 'pn-need' }, need)))],
    ]),
    ...entranceRow(ctx, kind, kind === 'driver' ? e.chooseDriver : e.chooseAppSound),
  );
}

function body(ctx: Context, step: SetupStep): [title: string, children: Child[]] {
  const w = text.wizard;
  const apps = entranceOf(ctx.snapshot) === 'app_sound';
  switch (step) {
    case 'welcome':
      return [
        w.welcomeTitle,
        [
          ...paragraphs(...w.welcomeBody),
          h(
            'div',
            { class: 'pn-entrances', role: 'group', 'aria-label': text.entrance.label },
            ...ENTRANCE_KINDS.map((kind) => entranceBlock(ctx, kind)),
          ),
        ],
      ];
    case 'unsupported':
      return apps
        ? [w.appUnsupportedTitle, [...paragraphs(w.appUnsupportedBody), platformFacts(ctx), ...entranceRow(ctx, 'driver', w.switchToDriver)]]
        : [w.unsupportedTitle, [...paragraphs(w.unsupportedBody), platformFacts(ctx), ...entranceRow(ctx, 'app_sound', w.switchToAppSound)]];
    case 'not_test_signing':
      return [
        w.testSigningTitle,
        [
          ...paragraphs(w.testSigningBody),
          platformFacts(ctx),
          intentRow(ctx, text.banner.recheck, { kind: 'recheck' }, { key: 'recheck' }),
          line(w.testSigningOther, 'pn-body pn-other'),
          ...entranceRow(ctx, 'app_sound', w.switchToAppSound),
        ],
      ];
    case 'driver': {
      const again = ctx.snapshot.error !== null || ctx.snapshot.driver?.kind === 'installed';
      return [
        w.driverTitle,
        [
          ...paragraphs(w.driverBody),
          h(
            'ul',
            { class: 'pn-endpoints' },
            ...text.endpoints.map((endpoint, lane) =>
              h('li', { 'data-lane': lane }, h('span', { class: 'pn-endpoint-name' }, endpoint.name), h('span', { class: 'pn-dim' }, endpoint.purpose)),
            ),
          ),
          ctx.snapshot.driver === null ? null : facts(driverRows(ctx.snapshot.driver)),
          intentRow(ctx, again ? w.driverReinstall : w.driverInstall, { kind: 'install_driver' }, {
            key: 'install',
            busyLabel: w.driverInstalling,
            describedBy: 'install-hint',
          }),
          hint(w.driverAdminHint, 'install-hint'),
        ],
      ];
    }
    case 'reboot_required':
      return [w.rebootTitle, [...paragraphs(w.rebootBody), intentRow(ctx, text.banner.recheck, { kind: 'recheck' }, { key: 'recheck' })]];
    case 'choose_output':
      return [w.outputTitle, [...paragraphs(w.outputBody), outputRows(ctx, 'output')]];
    case 'choose_discard':
      return [w.discardTitle, [...paragraphs(w.discardBody), discardRows(ctx, 'discard')]];
  }
}

export function wizardPage(ctx: Context, step: SetupStep): Drawn {
  const [title, children] = body(ctx, step);
  const entrance = entranceOf(ctx.snapshot);
  const element = h(
    'div',
    { class: 'pn-wizard' },
    steps(stepLabels(entrance), stepIndex(step, entrance)),
    h('div', { class: 'pn-wizard-body' }, h('h1', { class: 'pn-title' }, title), ...children),
  );
  return { element };
}

export function quitPage(): Drawn {
  return {
    element: h(
      'div',
      { class: 'pn-wizard pn-quit' },
      h('div', { class: 'pn-wizard-body' }, h('h1', { class: 'pn-title' }, text.wizard.quitTitle), line(text.wizard.quitBody, 'pn-body')),
    ),
  };
}
