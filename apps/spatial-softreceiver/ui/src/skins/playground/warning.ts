// The warning before another skin changes to the playground (ADR 0047). The
// shared controller loads this only when that choice is made, so the other
// skins carry none of the playground until then. It comes in the playground's
// own colours over whichever skin is on screen, and it comes every time:
// a warning has no "don't show again".

import './warning.css';
import type { Language } from '../../bindings';
import { h } from '../../dom';
import { en, ko } from './catalog';

/** Asks whether to go on to the playground; closing it any other way means no. */
export function warn(language: Language, host: HTMLElement): Promise<boolean> {
  const t = (language === 'en' ? en : ko).own.warning;
  return new Promise((resolve) => {
    const no = h('button', { type: 'button', class: 'pgw-button', 'data-key': 'skin-warning-no', autofocus: true }, t.no);
    const yes = h('button', { type: 'button', class: 'pgw-button is-go', 'data-key': 'skin-warning-yes' }, t.yes);
    const dialog = h(
      'dialog',
      { class: 'pgw', 'aria-labelledby': 'pgw-title', 'aria-describedby': 'pgw-body' },
      h('div', { class: 'pgw-stripe', 'aria-hidden': 'true' }),
      h('h2', { id: 'pgw-title' }, t.title),
      h('p', { id: 'pgw-body' }, t.body),
      h('div', { class: 'pgw-actions' }, no, yes),
    );
    let answered = false;
    const answer = (value: boolean) => {
      if (answered) return;
      answered = true;
      dialog.remove();
      resolve(value);
    };
    no.addEventListener('click', () => answer(false));
    yes.addEventListener('click', () => answer(true));
    // Escape closes a modal dialog by itself.
    dialog.addEventListener('close', () => answer(false));
    host.append(dialog);
    dialog.showModal();
    no.focus();
  });
}
