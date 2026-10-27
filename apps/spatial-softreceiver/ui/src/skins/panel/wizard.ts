// The first-run wizard as a step screen in the display (ADR 0041, 전면 패널의
// 법칙 5): the four steps along the top with the current one marked, the
// step's sentences and facts, and its action rows. The supervisor decides
// the step; the screen only explains it and offers the intent that moves it
// on. After quitting the display says what the standard closing page says.

import type { SetupStep } from '../../bindings';
import { type Child, h } from '../../dom';
import { text } from '../../text';
import type { Context, Drawn } from '../../screens/common';
import { driverRows, platformRows } from '../../screens/facts';
import { stepIndex } from '../../screens/wizard';
import { facts, hint, intentRow, line, outputRows } from './ui';

function steps(current: number): HTMLElement {
  return h(
    'ol',
    { class: 'pn-steps', 'aria-label': text.wizard.stepsLabel },
    ...text.wizard.steps.map((label, index) => {
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

function body(ctx: Context, step: SetupStep): [title: string, children: Child[]] {
  const w = text.wizard;
  switch (step) {
    case 'welcome':
      return [w.welcomeTitle, [...paragraphs(...w.welcomeBody), intentRow(ctx, w.begin, { kind: 'begin_setup' }, { key: 'begin' })]];
    case 'unsupported':
      return [w.unsupportedTitle, [...paragraphs(w.unsupportedBody), platformFacts(ctx)]];
    case 'not_test_signing':
      return [
        w.testSigningTitle,
        [...paragraphs(w.testSigningBody), platformFacts(ctx), intentRow(ctx, text.banner.recheck, { kind: 'recheck' }, { key: 'recheck' })],
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
  }
}

export function wizardPage(ctx: Context, step: SetupStep): Drawn {
  const [title, children] = body(ctx, step);
  const element = h(
    'div',
    { class: 'pn-wizard' },
    steps(stepIndex(step)),
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
