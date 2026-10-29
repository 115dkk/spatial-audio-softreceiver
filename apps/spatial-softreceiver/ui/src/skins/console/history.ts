// The status history (law 8, ADR 0041 addendum): the last 90 minutes the
// supervisor folded into `Snapshot.health`, shown like a public status page.
// A summary of the minute in progress, one row of 90 bars per part with its
// uptime, a readout of the minute under the pointer or the keyboard, and the
// incidents the supervisor grouped. Every state is the supervisor's; the
// screen only counts minutes for the uptime share. Minutes nobody watched
// stay "no record" and are never filled in.

import type { HealthHistory, HealthIncident, HealthMinute, HealthPart, MinuteState } from '../../bindings';
import { h, liveText } from '../../dom';
import { count, oneDecimal } from '../../format';
import { appMode } from '../../screens/entrance';
import { text } from '../../text';
import { type Build, type Tone, badge, cls, section, table, toneGlyph } from './kit';

export const PARTS: readonly HealthPart[] = ['driver', 'engine', 'output', 'plugins'];
/** Bars per row: one per minute of the window. */
export const SLOTS = 90;

/**
 * A part by its name. With the app-sound entrance (`apps`) the driver is not
 * in use (ADR 0043), and its row says so rather than read as idle or down.
 */
export function partName(part: HealthPart, apps = false): string {
  const c = text.console;
  switch (part) {
    case 'driver':
      return apps ? c.partDriverUnused : c.partDriver;
    case 'engine':
      return c.engine;
    case 'output':
      return c.device;
    case 'plugins':
      return c.partPlugins;
  }
}

export function stateWord(state: MinuteState): string {
  const c = text.console;
  switch (state) {
    case 'no_record':
      return c.minuteNoRecord;
    case 'idle':
      return c.minuteIdle;
    case 'good':
      return c.minuteGood;
    case 'degraded':
      return c.minuteDegraded;
    case 'down':
      return c.minuteDown;
  }
}

export function stateTone(state: MinuteState): Tone {
  return state === 'good' ? 'good' : state === 'degraded' ? 'warn' : state === 'down' ? 'crit' : 'off';
}

const SEVERITY: Record<MinuteState, number> = { no_record: 0, idle: 1, good: 2, degraded: 3, down: 4 };

/** Minutes up (good or degraded) over minutes in use (good, degraded or down), as a percentage. */
export function uptime(minutes: readonly HealthMinute[], part: HealthPart): number | null {
  let up = 0;
  let used = 0;
  for (const minute of minutes) {
    const state = minute[part];
    if (state === 'good' || state === 'degraded') up += 1;
    if (state === 'good' || state === 'degraded' || state === 'down') used += 1;
  }
  return used === 0 ? null : (up / used) * 100;
}

const pad2 = (value: number) => String(value).padStart(2, '0');

/** A minute `ago` minutes before now as HH:MM on the local clock, with how long ago. */
export function at(ago: number, now: number): string {
  const time = new Date(now - ago * 60_000);
  return `${pad2(time.getHours())}:${pad2(time.getMinutes())} (${ago === 0 ? text.console.checkNow : text.console.minutesAgo(ago)})`;
}

/** The overall line, from the minute in progress: every part in use good, or the worst one. */
function summary(minutes: readonly HealthMinute[], apps: boolean): { tone: Tone; sentence: string } {
  const c = text.console;
  const last = minutes.at(-1);
  if (last === undefined) return { tone: 'off', sentence: c.historyEmpty };
  const used = PARTS.filter((part) => SEVERITY[last[part]] >= SEVERITY.good);
  if (used.length === 0) return { tone: 'off', sentence: c.historyIdle };
  const worst = used.reduce((a, b) => (SEVERITY[last[b]] > SEVERITY[last[a]] ? b : a));
  const state = last[worst];
  if (state === 'good') return { tone: 'good', sentence: c.historyGood };
  return { tone: stateTone(state), sentence: c.partState(partName(worst, apps), stateWord(state)) };
}

export interface HistorySelection {
  part: HealthPart;
  /** Minutes before the minute in progress. */
  ago: number;
}

/** Where the highlighted minute lives between frames: the skin's view state. */
export interface SelectionStore {
  get(): HistorySelection | null;
  set(selection: HistorySelection): void;
}

function incidentRows(
  minutes: readonly HealthMinute[],
  incidents: readonly HealthIncident[],
  now: number,
  apps: boolean,
): HTMLTableRowElement[] {
  const c = text.console;
  const agoOf = (index: number) => minutes.length - 1 - index;
  return [...incidents]
    .sort((a, b) => b.last - a.last || b.first - a.first)
    .map((incident) => {
      const counts = [
        incident.underruns > 0 ? text.status.asioUnderruns(count(incident.underruns)) : null,
        incident.output_recoveries > 0 ? c.recoveries(count(incident.output_recoveries)) : null,
      ].filter((part): part is string => part !== null);
      return h(
        'tr',
        { class: incident.open ? 'is-open' : null },
        h('th', { scope: 'row' }, badge(stateTone(incident.worst), c.partState(partName(incident.part, apps), stateWord(incident.worst)))),
        h('td', null, incident.open ? badge(stateTone(incident.worst), c.incidentOpen) : badge('good', c.incidentClosed)),
        h('td', { class: 'cn-num' }, at(agoOf(incident.first), now)),
        h('td', { class: 'cn-num' }, incident.open ? c.checkNow : at(agoOf(incident.last), now)),
        h('td', { class: 'cn-num' }, c.durationMinutes(incident.last - incident.first + 1)),
        h('td', null, counts.length === 0 ? '-' : counts.join(', ')),
      );
    });
}

export function history(b: Build, health: HealthHistory | undefined, store: SelectionStore): HTMLElement {
  const c = text.console;
  const apps = appMode(b.ctx.snapshot);
  const minutes = health?.minutes.slice(-SLOTS) ?? [];
  const shift = (health?.minutes.length ?? 0) - minutes.length;
  const incidents = (health?.incidents ?? [])
    .map((incident) => ({ ...incident, first: Math.max(0, incident.first - shift), last: incident.last - shift }))
    .filter((incident) => incident.last >= 0);
  const now = Date.now();
  const padding = SLOTS - minutes.length;
  const minuteAt = (slot: number): HealthMinute | null => (slot >= padding ? (minutes[slot - padding] ?? null) : null);

  const readout = liveText(c.readoutHint);
  const readoutId = 'cn-history-readout';
  const rows = new Map<HealthPart, { element: HTMLElement; bars: HTMLElement[] }>();
  let shown: HistorySelection | null = null;

  function say(selection: HistorySelection): string {
    const slot = SLOTS - 1 - selection.ago;
    const minute = minuteAt(slot);
    const state = minute?.[selection.part] ?? 'no_record';
    const line = c.readoutState(at(selection.ago, now), partName(selection.part, apps), stateWord(state));
    return minute === null ? line : `${line} ${c.readoutCounts(count(minute.underruns), count(minute.output_recoveries))}`;
  }

  function select(selection: HistorySelection) {
    if (shown !== null) {
      rows.get(shown.part)?.bars[SLOTS - 1 - shown.ago]?.classList.remove('is-selected');
      rows.get(shown.part)?.element.classList.remove('is-active');
    }
    shown = selection;
    store.set(selection);
    const row = rows.get(selection.part);
    row?.bars[SLOTS - 1 - selection.ago]?.classList.add('is-selected');
    row?.element.classList.add('is-active');
    readout.set(say(selection));
  }

  const list = h('div', { class: 'cn-history-rows' });
  for (const part of PARTS) {
    const current = minutes.at(-1)?.[part] ?? 'no_record';
    const share = uptime(minutes, part);
    const shareText = share === null ? '-' : `${oneDecimal(share)}%`;
    const degraded = minutes.filter((minute) => minute[part] === 'degraded').length;
    const down = minutes.filter((minute) => minute[part] === 'down').length;
    const bars = Array.from({ length: SLOTS }, (_, slot) => {
      const state = minuteAt(slot)?.[part] ?? 'no_record';
      const bar = h('span', { class: `cn-bar is-${state}`, 'data-slot': slot });
      bar.addEventListener('mouseenter', () => select({ part, ago: SLOTS - 1 - slot }));
      bar.addEventListener('click', () => {
        select({ part, ago: SLOTS - 1 - slot });
        element.focus({ preventScroll: true });
      });
      return bar;
    });
    const element = h(
      'div',
      {
        class: 'cn-history-row',
        role: 'group',
        tabindex: 0,
        'data-key': `history-${part}`,
        'data-part': part,
        'aria-label': c.rowLabel(partName(part, apps), stateWord(current), shareText, degraded, down),
        'aria-describedby': readoutId,
      },
      h(
        'div',
        { class: 'cn-history-head' },
        h('span', { class: 'cn-history-name' }, partName(part, apps)),
        badge(stateTone(current), stateWord(current)),
        h('span', { class: 'cn-history-uptime' }, c.uptime, ' ', h('span', { class: 'cn-num' }, shareText)),
      ),
      h('div', { class: 'cn-bars', 'aria-hidden': 'true' }, ...bars),
    );
    element.addEventListener('focus', () => {
      if (shown?.part !== part) select({ part, ago: shown?.ago ?? 0 });
    });
    element.addEventListener('keydown', (event) => {
      const ago = shown?.part === part ? shown.ago : 0;
      const next =
        event.key === 'ArrowLeft' ? Math.min(SLOTS - 1, ago + 1)
        : event.key === 'ArrowRight' ? Math.max(0, ago - 1)
        : event.key === 'Home' ? SLOTS - 1
        : event.key === 'End' ? 0
        : null;
      if (next === null) return;
      event.preventDefault();
      select({ part, ago: next });
    });
    rows.set(part, { element, bars });
    list.append(element);
  }

  const kept = store.get();
  if (kept !== null) select(kept);

  const top = summary(minutes, apps);
  const incidentTable =
    incidents.length === 0
      ? h('p', { class: 'cn-muted' }, c.incidentsNone)
      : table({
          labelledBy: 'cn-incidents-title',
          head: [c.incidentPart, text.status.stateLabel, c.colStart, c.colEnd, c.colLength, c.colCounts],
          rows: incidentRows(minutes, incidents, now, apps),
          className: 'cn-incident-table',
        });
  return section(
    b,
    'history',
    c.historyTitle,
    [
      h(
        'div',
        { class: `cn-history-summary tone-${top.tone}` },
        h('span', { class: 'cn-history-summary-mark' }, toneGlyph(top.tone)),
        h('p', null, top.sentence),
      ),
      list,
      h(
        'div',
        { class: 'cn-history-scale', 'aria-hidden': 'true' },
        h('span', null, c.minutesAgo(SLOTS)),
        h('span', null, c.checkNow),
      ),
      h('p', { id: readoutId, class: cls('cn-history-readout'), 'aria-live': 'polite' }, readout.node),
      h('h3', { id: 'cn-incidents-title', class: 'cn-sub-title' }, c.incidentsTitle),
      incidentTable,
    ],
    { aside: h('span', { class: 'cn-hint' }, c.historyBasis), className: 'cn-history' },
  );
}
