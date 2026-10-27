// The outputs to choose from, as a table (law 6): the overview's output
// list and the setup checklist's output step share it.

import { h } from '../../dom';
import { text } from '../../text';
import { type Build, act, badge, para, table } from './kit';

export function outputList(b: Build, keyPrefix: string): HTMLElement {
  const { outputs, chosen_output } = b.ctx.snapshot;
  if (outputs.length === 0) {
    return h(
      'div',
      { class: 'cn-empty' },
      para(text.wizard.noOutputs),
      h('div', { class: 'cn-actions' }, act(b, text.banner.recheck, { kind: 'recheck' }, { key: `${keyPrefix}-recheck` })),
    );
  }
  const saved = b.group;
  const rows = outputs.map((output, index) => {
    const current = chosen_output?.id === output.id;
    b.group = output.name;
    const control = current
      ? badge('good', text.status.currentOutput)
      : act(b, text.wizard.useOutput, { kind: 'choose_output', value: output.id }, { key: `${keyPrefix}-${index}` });
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
    { class: 'cn-output-list', 'data-region': 'output-list' },
    table({ label: text.wizard.outputsLabel, head: [text.wizard.outputsLabel, null], rows }),
  );
}
