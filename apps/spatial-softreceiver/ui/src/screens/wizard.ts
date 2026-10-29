// The first-run wizard (phase `setup`): a step strip and one card per step.
// The supervisor decides the step; the card only explains it and offers the
// intent that moves it on. The first step chooses the entrance (ADR 0043);
// the strip then follows the entrance chosen.

import type { EntranceKind, SetupStep, Snapshot } from '../bindings';
import { facts, h } from '../dom';
import { text } from '../text';
import { brand } from './nav';
import { type Context, described, intentButton } from './common';
import {
  ENTRANCE_KINDS,
  captureOf,
  entranceBlocked,
  entranceGets,
  entranceKey,
  entranceName,
  entranceNeeds,
  entranceOf,
  stepIndex,
  stepLabels,
} from './entrance';
import { driverRows, platformRows } from './facts';

export { stepIndex } from './entrance';

function steps(labels: readonly string[], current: number): HTMLElement {
  const list = h('ol', { class: 'steps', 'aria-label': text.wizard.stepsLabel });
  labels.forEach((label, index) => {
    const state = index < current ? 'done' : index === current ? 'current' : 'upcoming';
    list.append(
      h(
        'li',
        { class: `step ${state}`, 'aria-current': state === 'current' ? 'step' : null },
        h('span', { class: 'step-number', 'aria-hidden': 'true' }, String(index + 1)),
        h('span', { class: 'step-label' }, label),
        state === 'done' ? h('span', { class: 'visually-hidden' }, ` (${text.wizard.stepDone})`) : null,
      ),
    );
  });
  return list;
}

function card(title: string, ...children: (Node | null)[]): HTMLElement {
  return h('section', { class: 'panel wizard-card' }, h('h1', { class: 'wizard-title' }, title), ...children);
}

const para = (sentence: string) => h('p', { class: 'body-text' }, sentence);

/**
 * One entrance to choose at the first step: its name, what it receives and
 * what it needs, and the button that chooses it (`choose_entrance`, which
 * also starts the checks). One the PC cannot run stays in view, locked,
 * beside the supervisor's reason.
 */
function entranceChoice(ctx: Context, kind: EntranceKind): HTMLElement {
  const e = text.entrance;
  const titleId = `${entranceKey(kind)}-title`;
  const blockedId = `${entranceKey(kind)}-blocked`;
  const blocked = entranceBlocked(ctx.snapshot, kind);
  const choose = intentButton(
    ctx,
    kind === 'driver' ? e.chooseDriver : e.chooseAppSound,
    { kind: 'choose_entrance', value: kind },
    // Neither entrance is the main action: the two weigh the same.
    { key: entranceKey(kind), variant: 'secondary' },
  );
  if (blocked !== null) choose.disabled = true;
  choose.setAttribute('aria-describedby', described(titleId, blocked !== null && blockedId) ?? titleId);
  return h(
    'article',
    { class: `entrance-choice${blocked === null ? '' : ' is-blocked'}`, 'aria-labelledby': titleId, 'data-entrance': kind },
    h(
      'h2',
      { id: titleId, class: 'entrance-choice-title' },
      entranceName(kind),
      blocked === null ? null : h('span', { class: 'tag tag-quiet' }, e.unavailable),
    ),
    h(
      'dl',
      { class: 'entrance-choice-facts' },
      h('div', null, h('dt', null, e.gets), h('dd', null, entranceGets(kind))),
      h('div', null, h('dt', null, e.needs), h('dd', null, ...entranceNeeds(kind, ctx.snapshot).map((need) => h('span', { class: 'entrance-need' }, need)))),
    ),
    blocked === null ? null : h('p', { id: blockedId, class: 'hint entrance-blocked' }, blocked),
    h('div', { class: 'actions' }, choose),
  );
}

function welcome(ctx: Context): HTMLElement {
  return card(
    text.wizard.welcomeTitle,
    ...text.wizard.welcomeBody.map(para),
    h('div', { class: 'entrance-choices' }, ...ENTRANCE_KINDS.map((kind) => entranceChoice(ctx, kind))),
  );
}

/**
 * The button that moves to the other entrance from a step this one cannot
 * pass; locked beside the reason when the PC cannot run that one either.
 */
function switchEntrance(ctx: Context, kind: EntranceKind): HTMLElement {
  const blocked = entranceBlocked(ctx.snapshot, kind);
  const button = intentButton(
    ctx,
    kind === 'driver' ? text.wizard.switchToDriver : text.wizard.switchToAppSound,
    { kind: 'choose_entrance', value: kind },
    { key: entranceKey(kind), variant: 'secondary' },
  );
  if (blocked === null) return button;
  button.disabled = true;
  button.setAttribute('aria-describedby', `${entranceKey(kind)}-blocked`);
  return h('div', { class: 'entrance-switch' }, button, h('p', { id: `${entranceKey(kind)}-blocked`, class: 'hint' }, blocked));
}

function platformFacts(snapshot: Snapshot): HTMLElement | null {
  return snapshot.platform === null ? null : facts(platformRows(snapshot.platform));
}

function unsupported(ctx: Context): HTMLElement {
  if (entranceOf(ctx.snapshot) === 'app_sound') {
    return card(
      text.wizard.appUnsupportedTitle,
      para(text.wizard.appUnsupportedBody),
      platformFacts(ctx.snapshot),
      h('div', { class: 'actions' }, switchEntrance(ctx, 'driver')),
    );
  }
  return card(
    text.wizard.unsupportedTitle,
    para(text.wizard.unsupportedBody),
    platformFacts(ctx.snapshot),
    h('div', { class: 'actions' }, switchEntrance(ctx, 'app_sound')),
  );
}

function notTestSigning(ctx: Context): HTMLElement {
  return card(
    text.wizard.testSigningTitle,
    para(text.wizard.testSigningBody),
    platformFacts(ctx.snapshot),
    h(
      'div',
      { class: 'actions' },
      intentButton(ctx, text.banner.recheck, { kind: 'recheck' }, { key: 'recheck', variant: 'primary' }),
    ),
    h('p', { class: 'body-text wizard-other' }, text.wizard.testSigningOther),
    h('div', { class: 'actions' }, switchEntrance(ctx, 'app_sound')),
  );
}

function endpointList(): HTMLElement {
  return h(
    'ul',
    { class: 'endpoint-names' },
    ...text.endpoints.map((endpoint) =>
      h('li', null, h('span', { class: 'endpoint-name' }, endpoint.name), h('span', { class: 'muted' }, endpoint.purpose)),
    ),
  );
}

function driver(ctx: Context): HTMLElement {
  const again = ctx.snapshot.error !== null || ctx.snapshot.driver?.kind === 'installed';
  return card(
    text.wizard.driverTitle,
    para(text.wizard.driverBody),
    endpointList(),
    ctx.snapshot.driver === null ? null : facts(driverRows(ctx.snapshot.driver)),
    h(
      'div',
      { class: 'actions' },
      intentButton(
        ctx,
        again ? text.wizard.driverReinstall : text.wizard.driverInstall,
        { kind: 'install_driver' },
        { key: 'install', variant: 'primary', pendingLabel: text.wizard.driverInstalling },
      ),
      h('p', { class: 'hint' }, text.wizard.driverAdminHint),
    ),
  );
}

function rebootRequired(ctx: Context): HTMLElement {
  return card(
    text.wizard.rebootTitle,
    para(text.wizard.rebootBody),
    h(
      'div',
      { class: 'actions' },
      intentButton(ctx, text.banner.recheck, { kind: 'recheck' }, { key: 'recheck', variant: 'primary' }),
    ),
  );
}

/** The outputs to choose from; shared with the status screen's list. */
export function outputList(ctx: Context, keyPrefix: string): HTMLElement {
  const { outputs, chosen_output } = ctx.snapshot;
  if (outputs.length === 0) {
    return h(
      'div',
      { class: 'empty' },
      para(text.wizard.noOutputs),
      h('div', { class: 'actions' }, intentButton(ctx, text.banner.recheck, { kind: 'recheck' }, { key: `${keyPrefix}-recheck` })),
    );
  }
  const list = h('ul', { class: 'output-list', 'data-region': 'output-list', 'aria-label': text.wizard.outputsLabel });
  outputs.forEach((output, index) => {
    const current = chosen_output?.id === output.id;
    list.append(
      h(
        'li',
        { class: `output-row${current ? ' current' : ''}` },
        h('span', { class: 'output-name' }, output.name),
        current
          ? h('span', { class: 'tag' }, text.status.currentOutput)
          : intentButton(
              ctx,
              text.wizard.useOutput,
              { kind: 'choose_output', value: output.id },
              { key: `${keyPrefix}-${index}`, variant: 'secondary' },
            ),
      ),
    );
  });
  return list;
}

/**
 * The devices that can take the apps' original sound (ADR 0043), drawn like
 * the output list: the one in use tagged, a button on each other. Shared
 * with the settings screen. With none, the wizard's band carries the
 * supervisor's sentence and this offers the recheck; elsewhere the sentence
 * stands here. `locked` names why the buttons are locked (a running engine).
 */
export function discardList(ctx: Context, keyPrefix: string, locked: string | null = null): HTMLElement {
  const { discards, discard } = captureOf(ctx.snapshot);
  if (discards.length === 0) {
    const inWizard = ctx.snapshot.phase.kind === 'setup';
    return h(
      'div',
      { class: 'empty' },
      inWizard ? null : h('p', { class: 'hint' }, text.entrance.noDiscards),
      inWizard
        ? h('div', { class: 'actions' }, intentButton(ctx, text.banner.recheck, { kind: 'recheck' }, { key: `${keyPrefix}-recheck` }))
        : null,
    );
  }
  const list = h('ul', { class: 'output-list', 'data-region': 'discard-list', 'aria-label': text.wizard.discardsLabel });
  discards.forEach((output, index) => {
    const current = discard?.id === output.id;
    let control: HTMLElement;
    if (current) {
      control = h('span', { class: 'tag' }, text.status.currentOutput);
    } else {
      const choose = intentButton(
        ctx,
        text.wizard.useOutput,
        { kind: 'choose_discard', value: output.id },
        { key: `${keyPrefix}-${index}`, variant: 'secondary' },
      );
      if (locked !== null) {
        choose.disabled = true;
        choose.setAttribute('aria-describedby', `${keyPrefix}-locked`);
      }
      control = choose;
    }
    list.append(h('li', { class: `output-row${current ? ' current' : ''}` }, h('span', { class: 'output-name' }, output.name), control));
  });
  if (locked === null) return list;
  return h('div', null, list, h('p', { id: `${keyPrefix}-locked`, class: 'hint discard-locked' }, locked));
}

function chooseOutput(ctx: Context): HTMLElement {
  return card(text.wizard.outputTitle, para(text.wizard.outputBody), outputList(ctx, 'output'));
}

function chooseDiscard(ctx: Context): HTMLElement {
  return card(text.wizard.discardTitle, para(text.wizard.discardBody), discardList(ctx, 'discard'));
}

function body(ctx: Context, step: SetupStep): HTMLElement {
  switch (step) {
    case 'welcome':
      return welcome(ctx);
    case 'unsupported':
      return unsupported(ctx);
    case 'not_test_signing':
      return notTestSigning(ctx);
    case 'driver':
      return driver(ctx);
    case 'reboot_required':
      return rebootRequired(ctx);
    case 'choose_output':
      return chooseOutput(ctx);
    case 'choose_discard':
      return chooseDiscard(ctx);
  }
}

export interface WizardParts {
  head: HTMLElement;
  body: HTMLElement;
}

/** The wizard's head (brand and steps) and the current step's card. */
export function wizard(ctx: Context, step: SetupStep): WizardParts {
  const entrance = entranceOf(ctx.snapshot);
  const head = h(
    'header',
    { class: 'wizard-head' },
    h('div', { class: 'wizard-brand' }, brand(), h('span', { class: 'muted' }, text.wizard.heading)),
    steps(stepLabels(entrance), stepIndex(step, entrance)),
  );
  return { head, body: body(ctx, step) };
}

/** The screen after `quit`. */
export function quit(): WizardParts {
  return {
    head: h('header', { class: 'wizard-head' }, h('div', { class: 'wizard-brand' }, brand())),
    body: card(text.wizard.quitTitle, para(text.wizard.quitBody)),
  };
}
