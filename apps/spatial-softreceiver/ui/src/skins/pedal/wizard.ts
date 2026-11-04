// The setup face and the quit face (ADR 0046, 이펙터의 법칙 3). Setup: a row
// of step LEDs with the steps' names printed (done lit green, the current
// one lit white, the ones ahead dark), the step's sentences printed in the
// face, and its actions as keys. The first step prints both entrances
// (ADR 0043), what each receives and needs, and the key that chooses it;
// the steps then follow the entrance chosen. The supervisor decides the
// step; the face only explains it and offers the intent that moves it on.
// The step's sentences stand in a printed legend box with the step's name
// on its tab, parted by hairlines, and its facts on spec plates (법칙 5).
// After quitting the face prints the closing sentences and every light of
// the box is dark. The room the sentences leave has the signal flow etched.

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
  switchTarget,
} from '../../screens/entrance';
import { driverRows, platformRows } from '../../screens/facts';
import { discardRow, outputRow } from './controls';
import { fine, flowRoom, group, hairline, intentKey, led, print, spec, sr } from './parts';

function steps(labels: readonly string[], current: number): HTMLElement {
  return h(
    'ol',
    { class: 'pd-steps', 'aria-label': text.wizard.stepsLabel },
    ...labels.map((label, index) => {
      const state = index < current ? 'done' : index === current ? 'current' : 'upcoming';
      return h(
        'li',
        { class: `pd-step is-${state}`, 'aria-current': state === 'current' ? 'step' : null },
        led(state === 'done' ? 'on' : 'white', state === 'upcoming' ? 'off' : 'on', 'md').element,
        print(label, 'pd-small'),
        state === 'done' ? sr(` (${text.wizard.stepDone})`) : null,
      );
    }),
  );
}

/** The step's sentences in a printed legend box, the step's name on its tab, a hairline between them. */
function legend(label: string, sentences: readonly string[]): HTMLElement {
  const children: Child[] = [];
  sentences.forEach((sentence, index) => {
    if (index > 0) children.push(hairline());
    children.push(h('p', { class: 'pd-body' }, sentence));
  });
  return group('pd-steptext', label, children, 'pd-steptext');
}

function platformFacts(ctx: Context): HTMLElement | null {
  return ctx.snapshot.platform === null ? null : spec(platformRows(ctx.snapshot.platform));
}

/** The key that chooses an entrance; a blocked welcome choice keeps only its reason. */
function entranceKeyFor(ctx: Context, kind: EntranceKind, label: string): Child[] {
  const blocked = entranceBlocked(ctx.snapshot, kind);
  const reasonId = `${entranceKey(kind)}-blocked`;
  if (blocked !== null) return [fine(blocked, reasonId)];
  return [intentKey(ctx, label, { kind: 'choose_entrance', value: kind }, { key: entranceKey(kind) })];
}

/** An entrance at the first step: its name, what it receives and needs, and its key. */
function entranceBlock(ctx: Context, kind: EntranceKind): HTMLElement {
  const e = text.entrance;
  const blocked = entranceBlocked(ctx.snapshot, kind) !== null;
  return h(
    'div',
    { class: `pd-group pd-entrance${blocked ? ' is-blocked' : ''}`, 'data-entrance': kind },
    h('h2', { class: 'pd-tab' }, entranceName(kind)),
    blocked ? print(e.unavailable, 'pd-tag') : null,
    spec([
      [e.gets, entranceGets(kind), true],
      [e.needs, h('span', null, ...entranceNeeds(kind, ctx.snapshot).map((need) => h('span', { class: 'pd-need' }, need))), true],
    ]),
    ...entranceKeyFor(ctx, kind, kind === 'driver' ? e.chooseDriver : e.chooseAppSound),
  );
}

function body(ctx: Context, step: SetupStep, label: string): [title: string, children: Child[]] {
  const w = text.wizard;
  const target = switchTarget(ctx.snapshot);
  switch (step) {
    case 'welcome':
      return [
        w.welcomeTitle,
        [
          legend(label, w.welcomeBody),
          h('div', { class: 'pd-entrances', role: 'group', 'aria-label': text.entrance.label }, ...ENTRANCE_KINDS.map((kind) => entranceBlock(ctx, kind))),
        ],
      ];
    case 'unsupported':
      return target === 'driver'
        ? [w.appUnsupportedTitle, [legend(label, [w.appUnsupportedBody]), platformFacts(ctx), ...entranceKeyFor(ctx, 'driver', w.switchToDriver)]]
        : [w.unsupportedTitle, [legend(label, [w.unsupportedBody]), platformFacts(ctx), ...(target === 'app_sound' ? entranceKeyFor(ctx, 'app_sound', w.switchToAppSound) : [])]];
    case 'not_test_signing':
      return [
        w.testSigningTitle,
        [
          legend(label, [w.testSigningBody]),
          platformFacts(ctx),
          intentKey(ctx, text.banner.recheck, { kind: 'recheck' }, { key: 'recheck' }),
          target === 'app_sound' ? h('p', { class: 'pd-body pd-other' }, w.testSigningOther) : null,
          ...(target === 'app_sound' ? entranceKeyFor(ctx, 'app_sound', w.switchToAppSound) : []),
        ],
      ];
    case 'driver': {
      const again = ctx.snapshot.error !== null || ctx.snapshot.driver?.kind === 'installed';
      return [
        w.driverTitle,
        [
          legend(label, [w.driverBody]),
          // The endpoints the driver adds, a ruled row each: the name Windows shows, what it is for.
          spec(
            text.endpoints.map((endpoint) => [endpoint.name, endpoint.purpose]),
            'pd-endpoints',
          ),
          ctx.snapshot.driver === null ? null : spec(driverRows(ctx.snapshot.driver)),
          intentKey(ctx, again ? w.driverReinstall : w.driverInstall, { kind: 'install_driver' }, {
            key: 'install',
            busyLabel: w.driverInstalling,
            describedBy: 'install-hint',
          }),
          fine(w.driverAdminHint, 'install-hint'),
        ],
      ];
    }
    case 'reboot_required':
      return [w.rebootTitle, [legend(label, [w.rebootBody]), intentKey(ctx, text.banner.recheck, { kind: 'recheck' }, { key: 'recheck' })]];
    case 'choose_output':
      return [w.outputTitle, [legend(label, [w.outputBody]), outputRow(ctx, 'output')]];
    case 'choose_discard':
      return [w.discardTitle, [legend(label, [w.discardBody]), discardRow(ctx, 'discard')]];
  }
}

export function wizardFace(ctx: Context, step: SetupStep): Drawn {
  const entrance = entranceOf(ctx.snapshot);
  const labels = stepLabels(entrance);
  const at = stepIndex(step, entrance);
  const [title, children] = body(ctx, step, labels[at] ?? text.wizard.stepsLabel);
  const element = h(
    'div',
    { class: 'pd-wizard' },
    steps(labels, at),
    h('div', { class: 'pd-rule', 'aria-hidden': 'true' }),
    h('div', { class: 'pd-wizard-body' }, h('h1', { class: 'pd-title' }, title), ...children),
    flowRoom(),
  );
  return { element };
}

export function quitFace(): Drawn {
  return {
    element: h(
      'div',
      { class: 'pd-wizard pd-quit' },
      h('div', { class: 'pd-wizard-body' }, h('h1', { class: 'pd-title' }, text.wizard.quitTitle), h('p', { class: 'pd-body' }, text.wizard.quitBody)),
      flowRoom(),
    ),
  };
}
