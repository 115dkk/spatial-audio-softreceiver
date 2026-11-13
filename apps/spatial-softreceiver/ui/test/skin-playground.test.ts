// The dangerous playground skin (ADR 0047): the console's renderer under
// overblown copy. These tests hold its law: the same handles send the same
// intents as the console, a modal comes only after the intent has gone and
// never more than once per feature, the links are the two fixed ones, the
// tips are fifty true sentences shown one at a time, and the English page
// has no Hangul.

import { beforeEach, describe, expect, it } from 'vitest';
import type { Api } from '../src/api';
import type { Intent, Language, Snapshot } from '../src/bindings';
import { fixture } from '../src/fixtures';
import { createMockApi } from '../src/mock';
import type { Tab } from '../src/screens/common';
import { mount } from '../src/skins/playground';
import { en, ko } from '../src/skins/playground/catalog';
import { en as plainEn, ko as plainKo, text } from '../src/text';

const HANGUL = /[ᄀ-ᇿ㄰-㆏가-힯]/;

let root: HTMLElement;

beforeEach(() => {
  localStorage.clear();
});

const tick = () => new Promise((resolve) => setTimeout(resolve, 0));
const $ = <T extends Element = HTMLElement>(selector: string) => document.body.querySelector<T>(selector);
const click = (selector: string) => {
  const target = $(selector);
  if (target === null) throw new Error(`nothing at ${selector}`);
  target.click();
};

function mounted(start: Snapshot, tab?: Tab) {
  root = document.createElement('div');
  document.body.replaceChildren(root);
  const sent: Intent[] = [];
  const opened: string[] = [];
  const mock = createMockApi(start);
  const api: Api = {
    ...mock,
    dispatch: async (intent) => void sent.push(intent),
    shell: { ...mock.shell, openLink: async (link) => void opened.push(link) },
  };
  const app = mount(root, api, tab === undefined ? {} : { tab });
  app.draw(start);
  return { app, sent, opened };
}

function words(element: Element): string {
  const attributes = [...element.querySelectorAll('[aria-label], [title], [placeholder]')].flatMap((node) =>
    ['aria-label', 'title', 'placeholder'].map((name) => node.getAttribute(name) ?? ''),
  );
  return [element.textContent ?? '', ...attributes].join('\n');
}

describe('the playground skin', () => {
  it('lays its words over the catalog and keeps the facts in them', () => {
    mounted(fixture('ready'));
    expect(text.console.overview).toBe(ko.over.screen.console.overview);
    expect(text.console.overview).not.toBe(plainKo.console.overview);
    // A sentence it does not replace is the catalog's own.
    expect(text.settings.autostart).toBe(plainKo.settings.autostart);
    // The health line still names what will play and where.
    const device = fixture('ready').chosen_output!.name;
    expect($('[data-region="health"]')!.textContent).toContain(device);
  });

  it('sends what the console sends, then congratulates and asks once', async () => {
    const { sent } = mounted(fixture('ready'));
    expect($('.pg-modal')).toBeNull();
    click('[data-key="power"]');
    await tick();
    expect(sent).toEqual([{ kind: 'enable' }]);
    // The intent had gone before anything was asked.
    expect($('.pg-toast')!.textContent).toContain(ko.own.features.enable);
    expect($('.pg-modal')!.getAttribute('data-kind')).toBe('yesno');
    click('.pg-modal-x');
    expect($('.pg-modal')).toBeNull();
  });

  it('begs after a PRO page, opens only the sponsors link, and does not ask again', async () => {
    const { opened, sent } = mounted(fixture('running'), 'status');
    click('[data-key="nav-upmix"]');
    const modal = $('.pg-modal')!;
    expect(modal.getAttribute('data-kind')).toBe('beg');
    expect(modal.textContent).toContain(ko.own.features.nav_upmix);
    click('.pg-modal .pg-button.is-hype');
    await tick();
    expect(opened).toEqual(['sponsors']);
    expect(sent).toEqual([]);
    expect($('.pg-modal')).toBeNull();
    click('[data-key="nav-status"]');
    click('.pg-modal-x');
    click('[data-key="nav-upmix"]');
    expect($('.pg-modal')).toBeNull();
    // Asked once per viewer: kept for the next page load.
    expect(JSON.parse(localStorage.getItem('ssr.playground.asked')!)).toContain('nav_upmix');
  });

  it('takes turns asking for a star and a rating, and a rating opens the repository', async () => {
    const { opened } = mounted(fixture('ready'), 'status');
    click('[data-key="nav-profile"]');
    expect($('.pg-modal')!.getAttribute('data-kind')).toBe('yesno');
    click('.pg-modal .pg-button:not(.is-hype)');
    expect(opened).toEqual([]);
    click('[data-key="nav-settings"]');
    expect($('.pg-modal')!.getAttribute('data-kind')).toBe('stars');
    const stars = [...document.body.querySelectorAll<HTMLElement>('.pg-star')];
    expect(stars).toHaveLength(5);
    stars[3]!.click();
    await tick();
    expect(opened).toEqual(['repo']);
    expect($('.pg-toasts')!.textContent).toContain(ko.own.toast.ratedTitle(4));
  });

  it('shows one of fifty tips and another one when the tip is closed', () => {
    for (const catalog of [ko, en]) {
      expect(catalog.own.tips).toHaveLength(50);
      expect(new Set(catalog.own.tips).size).toBe(50);
    }
    mounted(fixture('running'));
    const shown = () => $('.pg-tip-text')!.textContent!;
    const first = shown();
    expect(ko.own.tips).toContain(first);
    for (let round = 0; round < 20; round += 1) {
      const before = shown();
      click('[data-key="pg-tip-next"]');
      expect(ko.own.tips).toContain(shown());
      expect(shown()).not.toBe(before);
    }
  });

  it('holds on twice to a viewer who leaves, and sends nothing when they stay', async () => {
    const { sent } = mounted(fixture('ready'), 'settings');
    const leave = () => {
      const select = $<HTMLSelectElement>('[data-key="skin"]')!;
      select.value = 'console';
      select.dispatchEvent(new Event('change', { bubbles: true }));
    };
    const kind = () => $('.pg-modal')?.getAttribute('data-kind') ?? null;

    leave();
    await tick();
    expect(kind()).toBe('stay');
    expect($('.pg-modal')!.textContent).toContain(ko.own.stay.lead(plainKo.skins.console));
    // Staying, by the button or by closing, sends nothing.
    click('.pg-modal .is-hype');
    await tick();
    expect(kind()).toBeNull();
    expect(sent).toEqual([]);
    expect($('.pg-toasts')!.textContent).toContain(ko.own.stay.toastTitle);

    await new Promise((resolve) => setTimeout(resolve, 1100));
    leave();
    await tick();
    click('.pg-modal .is-leave');
    expect(kind()).toBe('offer');
    expect(sent).toEqual([]);
    click('.pg-modal .is-leave');
    await tick();
    expect(kind()).toBeNull();
    expect(sent).toEqual([{ kind: 'set_skin', value: 'console' }]);
  });

  it('prints no apology for itself', () => {
    mounted(fixture('running'));
    expect($('.pg-footnote')).toBeNull();
    for (const own of [ko.own, en.own]) expect('footnote' in own).toBe(false);
  });

  it('has the console handles the gallery and the other tests hold', () => {
    mounted(fixture('ready'), 'status');
    for (const key of ['power', 'outputs-toggle', 'nav-status', 'nav-profile', 'nav-upmix', 'nav-settings', 'cn-palette']) {
      expect($(`[data-key="${key}"]`), key).not.toBeNull();
    }
    expect($('[data-region="health"]')).not.toBeNull();
  });

  it.each<[Language, string]>([
    ['en', 'ready'],
    ['en', 'running'],
    ['en', 'faulted'],
    ['en', 'setup-welcome'],
  ])('has no Hangul on the %s page of %s', async (language, name) => {
    const start = fixture(name, language);
    mounted(start);
    expect(text.console.overview).toBe(en.over.screen.console.overview);
    expect(text.settings.autostart).toBe(plainEn.settings.autostart);
    if ($('[data-key="power"]') !== null) {
      click('[data-key="power"]');
      await tick();
    }
    const hangul = words(document.body)
      .split('\n')
      .filter((line) => HANGUL.test(line));
    expect(hangul).toEqual([]);
  });
});
