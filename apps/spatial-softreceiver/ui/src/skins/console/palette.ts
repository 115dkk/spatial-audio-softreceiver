// The command palette (law 5): every action the frame offers and the go-to
// commands, searchable by name, in a modal dialog with the combobox and
// listbox pattern. It lives outside the skin's root so a snapshot that
// redraws the window does not replace the search field while someone types
// in it (an IME composition would be lost); each frame only refreshes the
// list. Whether it is open and what was typed is view state of this skin.

import { h, svg } from '../../dom';
import { text } from '../../text';
import type { Command } from './kit';

const LIST_ID = 'cn-palette-list';
const optionId = (index: number) => `cn-cmd-${index}`;

/** Lower case, without spaces, so 출력장치 finds 출력 장치. */
const fold = (value: string) => value.toLocaleLowerCase().replace(/\s+/g, '');

export function matches(command: Command, query: string): boolean {
  const haystack = fold(`${command.label} ${command.group ?? ''}`);
  return query
    .split(/\s+/)
    .filter((token) => token.length > 0)
    .every((token) => haystack.includes(fold(token)));
}

export function searchGlyph(): SVGSVGElement {
  return svg(
    'svg',
    { class: 'cn-icon', viewBox: '0 0 16 16', width: 16, height: 16, 'aria-hidden': 'true', focusable: 'false' },
    svg('circle', { cx: 7, cy: 7, r: 4.5, fill: 'none', stroke: 'currentColor', 'stroke-width': 1.6 }),
    svg('path', { d: 'M10.4 10.4L14 14', fill: 'none', stroke: 'currentColor', 'stroke-width': 1.6, 'stroke-linecap': 'round' }),
  );
}

export class Palette {
  private layer: HTMLElement | null = null;
  private input: HTMLInputElement | null = null;
  private list: HTMLUListElement | null = null;
  private commands: Command[] = [];
  private shown: Command[] = [];
  private activeId: string | null = null;
  private returnTo: HTMLElement | null = null;
  private returnKey: string | null = null;

  constructor(private readonly root: HTMLElement) {}

  get isOpen(): boolean {
    return this.layer !== null;
  }

  /** The frame's commands; the list follows them while it is open. */
  setCommands(commands: Command[]): void {
    this.commands = commands;
    if (this.isOpen) this.refresh();
  }

  open(): void {
    if (this.layer !== null) {
      this.input?.focus();
      return;
    }
    const active = document.activeElement;
    this.returnTo = active instanceof HTMLElement && active !== document.body ? active : null;
    this.returnKey = this.returnTo?.dataset.key ?? null;
    this.activeId = null;

    const input = h('input', {
      type: 'text',
      class: 'cn-palette-input',
      role: 'combobox',
      'aria-expanded': 'true',
      'aria-controls': LIST_ID,
      'aria-autocomplete': 'list',
      'aria-label': text.console.paletteInput,
      placeholder: text.console.paletteInput,
      autocomplete: 'off',
      spellcheck: 'false',
    });
    input.addEventListener('input', () => {
      this.activeId = null;
      this.refresh();
    });
    input.addEventListener('keydown', (event) => this.onKey(event));
    const list = h('ul', { id: LIST_ID, class: 'cn-palette-list', role: 'listbox', 'aria-label': text.console.paletteTitle });
    const dialog = h(
      'div',
      { class: 'cn-palette', role: 'dialog', 'aria-modal': 'true', 'aria-labelledby': 'cn-palette-title' },
      h('h2', { id: 'cn-palette-title', class: 'cn-visually-hidden' }, text.console.paletteTitle),
      h('div', { class: 'cn-palette-search' }, searchGlyph(), input, h('kbd', { class: 'cn-kbd', 'aria-hidden': 'true' }, 'Esc')),
      list,
      h('p', { class: 'cn-palette-foot' }, text.console.paletteHint),
    );
    const layer = h('div', { class: 'cn-palette-layer' }, dialog);
    layer.addEventListener('mousedown', (event) => {
      // A press on the backdrop closes; one inside the dialog does not.
      if (event.target === layer) {
        event.preventDefault();
        this.close();
      }
    });
    this.layer = layer;
    this.input = input;
    this.list = list;
    document.body.append(layer);
    this.root.setAttribute('inert', '');
    this.refresh();
    input.focus();
  }

  /** Closes and puts the focus back where it was. */
  close(): void {
    if (this.layer === null) return;
    this.layer.remove();
    this.layer = null;
    this.input = null;
    this.list = null;
    this.root.removeAttribute('inert');
    const byKey =
      this.returnKey === null ? null : this.root.querySelector<HTMLElement>(`[data-key="${CSS.escape(this.returnKey)}"]`);
    const target =
      this.returnTo?.isConnected === true
        ? this.returnTo
        : (byKey ?? this.root.querySelector<HTMLElement>('[data-key="cn-palette"]'));
    target?.focus({ preventScroll: true });
  }

  private activeIndex(): number {
    if (this.shown.length === 0) return -1;
    const at = this.shown.findIndex((command) => command.id === this.activeId);
    return at < 0 ? 0 : at;
  }

  private refresh(): void {
    if (this.list === null || this.input === null) return;
    const query = this.input.value;
    this.shown = this.commands.filter((command) => matches(command, query));
    const at = this.activeIndex();
    this.activeId = this.shown[at]?.id ?? null;
    if (this.shown.length === 0) {
      this.list.replaceChildren(h('li', { class: 'cn-palette-empty', role: 'presentation' }, text.console.paletteEmpty));
    } else {
      this.list.replaceChildren(
        ...this.shown.map((command, index) => {
          const option = h(
            'li',
            {
              id: optionId(index),
              class: 'cn-palette-option',
              role: 'option',
              'aria-selected': index === at ? 'true' : 'false',
              'aria-disabled': command.disabled ? 'true' : null,
              'data-command': command.id,
            },
            h('span', { class: 'cn-palette-label' }, command.label),
            command.group === null ? null : h('span', { class: 'cn-palette-group' }, command.group),
            command.disabled && command.reason !== null
              ? h('span', { class: 'cn-palette-reason' }, command.reason)
              : command.action === null
                ? null
                : h('span', { class: 'cn-palette-action' }, command.action),
          );
          option.addEventListener('mousedown', (event) => event.preventDefault());
          option.addEventListener('click', () => this.run(index));
          return option;
        }),
      );
    }
    this.mark(at);
  }

  /** Moves the selection without rebuilding the list. */
  private mark(at: number): void {
    if (this.list === null || this.input === null) return;
    this.list.querySelectorAll('[role="option"]').forEach((option, index) => {
      option.setAttribute('aria-selected', index === at ? 'true' : 'false');
    });
    if (at < 0) this.input.removeAttribute('aria-activedescendant');
    else this.input.setAttribute('aria-activedescendant', optionId(at));
    this.list.querySelector<HTMLElement>(`#${optionId(at)}`)?.scrollIntoView?.({ block: 'nearest' });
  }

  private move(step: number): void {
    if (this.shown.length === 0) return;
    const at = (this.activeIndex() + step + this.shown.length) % this.shown.length;
    this.activeId = this.shown[at]?.id ?? null;
    this.mark(at);
  }

  private run(index: number): void {
    const command = this.shown[index];
    if (command === undefined || command.disabled) return;
    this.close();
    command.run();
  }

  private onKey(event: KeyboardEvent): void {
    // An IME still composing owns its Enter and arrows.
    if (event.isComposing) return;
    switch (event.key) {
      case 'ArrowDown':
        event.preventDefault();
        this.move(1);
        break;
      case 'ArrowUp':
        event.preventDefault();
        this.move(-1);
        break;
      case 'Enter':
        event.preventDefault();
        this.run(this.activeIndex());
        break;
      case 'Escape':
        event.preventDefault();
        event.stopPropagation();
        this.close();
        break;
      case 'Tab':
        // The field is the dialog's only stop; Tab stays in it.
        event.preventDefault();
        break;
    }
  }
}
