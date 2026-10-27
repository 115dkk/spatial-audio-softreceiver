// The overview (tab 'status'): the KPI strip with its 60-second traces, the
// processing flow as a map, the entrances as a table, the sound positions
// beside the output, and every other meter in one table below. Everything is
// built once a frame; the 30 Hz meters only change text nodes and attributes.

import type { AsioStatus, EndpointStatus, EndpointUpmixView, EngineStatus, Snapshot, UpmixerChoice } from '../../bindings';
import { type Child, h, liveText, setAttr, svg } from '../../dom';
import {
  chainErrorEntries,
  chainErrorTotal,
  codecName,
  count,
  describeCarrier,
  describeClock,
  describeFormat,
  describeOutputFormat,
  describeOutputMode,
  describeRate,
  describeUnsupported,
  describeVolume,
  oneDecimal,
} from '../../format';
import { ASIO_DEFAULTS } from '../../guard';
import { bypassReason } from '../../screens/upmix';
import { text } from '../../text';
import { listening } from './health';
import { type SelectionStore, history } from './history';
import {
  ASIO_LANE,
  type Build,
  type Tone,
  cls,
  entrance,
  laneMark,
  para,
  plain,
  row,
  section,
  table,
  toneGlyph,
} from './kit';
import { sceneMap } from './map';
import { outputList } from './outputs';
import type { Traces } from './trace';

/** Endpoints the screen shows: 0, 1 and 2; slot 3 is reserved in the contract. */
const SHOWN = 3;

export interface Page {
  element: HTMLElement;
  /** Fills the live parts from the engine (no sample is taken). */
  fill(engine: EngineStatus): void;
  /** Redraws the traces from the buffer. */
  traces?(now: number): void;
}

/** The entrances on the map and in the table: the three endpoints, and the ASIO input while it is on. */
export function lanesOf(snapshot: Snapshot): number[] {
  const lanes = [0, 1, 2];
  if ((snapshot.asio ?? ASIO_DEFAULTS).enabled) lanes.push(ASIO_LANE);
  return lanes;
}

// The KPI strip (law 3: a number with its unit and its basis) --------------

interface Kpi {
  key: string;
  label: string;
  unit: string;
  basis: 'window' | 'since';
  read(engine: EngineStatus): number | null;
  say(value: number): string;
  sub?(engine: EngineStatus): string;
}

function kpis(): Kpi[] {
  const m = text.meters;
  return [
    { key: 'p50', label: m.latencyP50, unit: m.ms, basis: 'window', read: (e) => (e.latency.samples === 0 ? null : e.latency.p50_ms), say: oneDecimal },
    { key: 'p99', label: m.latencyP99, unit: m.ms, basis: 'window', read: (e) => (e.latency.samples === 0 ? null : e.latency.p99_ms), say: oneDecimal },
    { key: 'ring', label: m.ringLevel, unit: m.ms, basis: 'window', read: (e) => e.ring_level_ms, say: oneDecimal },
    {
      key: 'clock',
      label: m.clock,
      unit: m.ppm,
      basis: 'window',
      read: (e) => e.clock_ppm,
      say: oneDecimal,
      sub: (e) => (e.clock === 'trim' ? m.trim : m.asrc),
    },
    { key: 'underruns', label: m.underruns, unit: m.times, basis: 'since', read: (e) => e.underruns, say: count },
    { key: 'restarts', label: m.outputRestarts, unit: m.times, basis: 'since', read: (e) => e.output_restarts, say: count },
    { key: 'reconnects', label: text.console.reconnects, unit: m.times, basis: 'since', read: (e) => e.output_reconnects, say: count },
  ];
}

/** The values the traces sample, by KPI key. */
export function traceValues(engine: EngineStatus): Record<string, number | null> {
  const values: Record<string, number | null> = {};
  for (const kpi of kpis()) values[kpi.key] = kpi.read(engine);
  return values;
}

const TRACE_W = 120;
const TRACE_H = 28;

function kpiStrip(b: Build, running: boolean, traces: Traces): Page {
  const c = text.console;
  const tiles: { fill(engine: EngineStatus): void; trace(now: number): void }[] = [];
  const list = h('ul', { class: 'cn-kpis' });
  for (const kpi of kpis()) {
    const value = liveText('-');
    const sub = liveText('');
    const line = svg('polyline', { class: 'cn-trace-line', points: '', display: 'none' });
    const tile = h(
      'li',
      { class: running ? 'cn-kpi' : 'cn-kpi is-off', 'data-kpi': kpi.key },
      h('span', { class: 'cn-kpi-label' }, kpi.label, kpi.sub && running ? h('span', { class: 'cn-kpi-sub' }, ' · ', sub.node) : null),
      h('span', { class: 'cn-kpi-value' }, h('span', { class: 'cn-num' }, value.node), h('span', { class: 'cn-kpi-unit' }, kpi.unit)),
      h('span', { class: 'cn-kpi-basis' }, running ? (kpi.basis === 'window' ? c.basisWindow : c.basisSinceOn) : c.basisOff),
      running
        ? h(
            'span',
            { class: 'cn-kpi-trace' },
            svg(
              'svg',
              { class: 'cn-trace', viewBox: `0 0 ${TRACE_W} ${TRACE_H}`, preserveAspectRatio: 'none', 'aria-hidden': 'true', focusable: 'false' },
              line,
            ),
            h('span', { class: 'cn-trace-label' }, c.traceLabel),
          )
        : null,
    );
    list.append(tile);
    tiles.push({
      fill(engine) {
        const read = kpi.read(engine);
        value.set(read === null ? '-' : kpi.say(read));
        if (kpi.sub) sub.set(kpi.sub(engine));
      },
      trace(now) {
        const points = traces.points(kpi.key, now, TRACE_W, TRACE_H);
        if (points === null) {
          setAttr(line, 'display', 'none');
        } else {
          setAttr(line, 'points', points);
          line.removeAttribute('display');
        }
      },
    });
  }
  const element = section(b, 'kpi', c.kpiTitle, [
    list,
    running ? h('p', { class: 'cn-hint' }, c.traceNote) : null,
  ]);
  return {
    element,
    fill: (engine) => tiles.forEach((tile) => tile.fill(engine)),
    traces: (now) => tiles.forEach((tile) => tile.trace(now)),
  };
}

// The processing flow (law 2) -------------------------------------------------

export function choiceName(choice: UpmixerChoice): string {
  return choice === 'first_party' ? text.upmix.firstParty : choice === 'plugin' ? text.upmix.plugin : text.upmix.off;
}

/** What an entrance's upmixer is doing, in words, and how it stands. */
export function upmixState(
  snapshot: Snapshot,
  view: EndpointUpmixView,
  withLatency = false,
): { word: string; tone: Tone } {
  const u = text.upmix;
  switch (view.state.kind) {
    case 'stopped': {
      if (view.effective === 'plugin' && view.plugin === null) return { word: u.noPlugin, tone: 'off' };
      return { word: snapshot.phase.kind === 'running' ? u.stateOff : u.stateAfterEnable, tone: 'off' };
    }
    case 'starting':
      return { word: u.stateStarting, tone: 'off' };
    case 'running':
      if (view.bypass !== null) return { word: bypassReason(view.bypass), tone: 'off' };
      return {
        word:
          !withLatency || view.latency_ms === null
            ? u.stateRunning
            : `${u.stateRunning} · ${u.latency(oneDecimal(view.latency_ms))}`,
        tone: 'good',
      };
    case 'fallback':
      return { word: u.stateFallback, tone: 'warn' };
    case 'failed':
      return { word: view.state.value, tone: 'crit' };
  }
}

/** The formats coming in now, by name: PCM, direct, the codecs. */
function formatNames(engine: EngineStatus): string {
  const names = new Set<string>();
  for (const endpoint of engine.endpoints.slice(0, SHOWN)) {
    const format = endpoint.format;
    if (format === null) continue;
    names.add(
      format.kind === 'pcm'
        ? format.value.channel_mask === 0
          ? text.format.direct
          : text.format.pcm
        : codecName(format.value.codec),
    );
  }
  return names.size === 0 ? text.status.noInput : [...names].join(', ');
}

/** Whether an entrance has sound now: scene objects in its lane, or a DAW that plays. */
function sounding(engine: EngineStatus, lane: number): boolean {
  if (lane === ASIO_LANE) {
    const asio = engine.asio ?? null;
    return asio !== null && asio.fault_reason === null && asio.client_pid !== 0 && !asio.clock_fault;
  }
  return engine.scene.some((object) => object.lane === lane);
}

function asioFormat(asio: AsioStatus): string {
  const s = text.status;
  const hz = asio.sample_rate;
  return [
    `${asio.layout} (${text.format.channels(asio.channels)})`,
    hz > 0 ? describeRate(hz) : '-',
    s.asioBuffer(count(asio.buffer_frames), hz > 0 ? oneDecimal((asio.buffer_frames / hz) * 1000) : '-'),
  ].join(' · ');
}

function flow(b: Build, running: boolean): Page {
  const c = text.console;
  const { snapshot } = b.ctx;
  const lanes = lanesOf(snapshot);
  const inputs = lanes.map((lane) => {
    const line = liveText(text.status.engineOff);
    const stub = h('span', { class: `cn-stub cn-lane-${lane}`, 'aria-hidden': 'true' });
    const element = h(
      'li',
      { class: `cn-node cn-node-in`, 'data-lane': lane },
      h('span', { class: 'cn-node-name' }, laneMark(lane), h('span', { class: 'cn-ellipsis' }, entrance(lane).name)),
      h('span', { class: 'cn-node-line cn-ellipsis' }, line.node),
      stub,
    );
    return { lane, line, stub, element };
  });

  const formats = liveText(running ? text.status.noInput : text.status.engineOff);
  const upmixLines: Child[] = snapshot.upmix.endpoints
    .filter((view) => view.effective !== 'off' && view.endpoint < SHOWN)
    .map((view) => {
      const state = upmixState(snapshot, view);
      return h(
        'span',
        { class: 'cn-node-line cn-node-upmix' },
        laneMark(view.endpoint),
        h('span', { class: 'cn-ellipsis' }, `${choiceName(view.effective)} · ${state.word}`),
      );
    });
  const directions = liveText(c.directions((running ? snapshot.engine?.directions : null)?.length ?? snapshot.profile.directions.length));
  const mode = liveText(
    snapshot.output_path === 'kernel_streaming' ? text.meters.kernelStreaming : text.settings.outputPathWasapi,
  );
  const outputFormat = liveText('');
  const formatLine = h('span', { class: 'cn-node-line cn-num', hidden: true }, outputFormat.node);
  const links = Array.from({ length: 3 }, () => h('span', { class: 'cn-link', 'aria-hidden': 'true' }));
  const bus = h('div', { class: 'cn-bus', 'aria-hidden': 'true' }, links[0]!);

  const stage = (title: string, ...lines: Child[]) =>
    h('div', { class: 'cn-node cn-node-stage' }, h('span', { class: 'cn-node-title' }, title), ...lines);

  const element = section(b, 'flow', c.flowTitle, [
    h(
      'div',
      { class: 'cn-flow-scroll' },
      h(
        'div',
        { class: 'cn-flow' },
        h('span', { class: 'cn-flow-caption' }, text.status.entrances),
        h('ul', { class: 'cn-flow-in', 'aria-label': text.status.entrances }, ...inputs.map((input) => input.element)),
        bus,
        stage(
          c.decode,
          h('span', { class: 'cn-node-line' }, formats.node),
          ...(upmixLines.length > 0 ? upmixLines : [h('span', { class: 'cn-node-line cn-muted' }, text.upmix.off)]),
        ),
        h('div', { class: 'cn-link-cell', 'aria-hidden': 'true' }, links[1]!),
        stage(
          c.render,
          h('span', { class: 'cn-node-line' }, listening(snapshot)),
          h('span', { class: 'cn-node-line cn-muted' }, directions.node),
        ),
        h('div', { class: 'cn-link-cell', 'aria-hidden': 'true' }, links[2]!),
        stage(
          text.status.output,
          h('span', { class: 'cn-node-line cn-node-strong' }, snapshot.chosen_output?.name ?? text.status.noOutput),
          h('span', { class: 'cn-node-line' }, mode.node),
          formatLine,
        ),
      ),
    ),
  ]);

  return {
    element,
    fill(engine) {
      let any = false;
      for (const input of inputs) {
        const on = sounding(engine, input.lane);
        any ||= on;
        input.stub.classList.toggle('is-flowing', on);
        if (input.lane === ASIO_LANE) {
          const asio = engine.asio ?? null;
          input.line.set(
            asio === null
              ? text.console.waiting
              : asio.fault_reason !== null
                ? text.status.asioFailed
                : asio.client_exe || '-',
          );
        } else {
          input.line.set(describeFormat(engine.endpoints[input.lane]?.format ?? null));
        }
      }
      for (const link of links) link.classList.toggle('is-flowing', any);
      bus.classList.toggle('is-flowing', any);
      formats.set(formatNames(engine));
      directions.set(text.console.directions(engine.directions.length));
      mode.set(describeOutputMode(engine));
      formatLine.hidden = false;
      outputFormat.set(describeOutputFormat(engine));
    },
  };
}

// The entrances table (law 6) ------------------------------------------------

interface EntranceRow {
  element: HTMLTableRowElement;
  fill(engine: EngineStatus | null): void;
}

function stateCell(): { cell: HTMLElement; set(tone: Tone, word: string, reason: string | null): void } {
  const word = liveText('');
  const reason = liveText('');
  const holder = h('span', { class: 'cn-badge tone-off' }, toneGlyph('off'), h('span', null, word.node));
  const reasonLine = h('span', { class: 'cn-cell-sub', hidden: true }, reason.node);
  let shownTone: Tone = 'off';
  return {
    cell: h('td', { class: 'cn-cell-state' }, holder, reasonLine),
    set(tone, label, why) {
      if (tone !== shownTone) {
        shownTone = tone;
        setAttr(holder, 'class', `cn-badge tone-${tone}`);
        holder.firstChild?.replaceWith(toneGlyph(tone));
      }
      word.set(label);
      reasonLine.hidden = why === null;
      if (why !== null) reason.set(why);
    },
  };
}

function nameCell(lane: number): HTMLElement {
  const { name, purpose } = entrance(lane);
  return h(
    'th',
    { scope: 'row', class: 'cn-cell-name' },
    h('span', { class: 'cn-entrance' }, laneMark(lane), h('span', null, name)),
    h('span', { class: 'cn-cell-sub' }, purpose),
  );
}

function endpointRow(snapshot: Snapshot, lane: number): EntranceRow {
  const s = text.status;
  const state = stateCell();
  const format = liveText('-');
  const carrier = liveText('');
  const carrierLine = h('span', { class: 'cn-cell-sub', hidden: true }, carrier.node);
  const volume = liveText('-');
  const epoch = liveText('-');
  const total = liveText('-');
  const kinds = h('ul', { class: 'cn-error-kinds' });
  const summary = h('summary', { tabindex: -1 }, h('span', { class: 'cn-num' }, total.node));
  const errors = h('details', { class: 'cn-errors' }, summary, kinds);
  const view = snapshot.upmix.endpoints.find((candidate) => candidate.endpoint === lane);
  const upmix: Child[] =
    view === undefined || view.effective === 'off'
      ? [h('span', { class: 'cn-muted' }, text.upmix.off)]
      : (() => {
          const st = upmixState(snapshot, view);
          return [
            h('span', null, view.plugin === null ? choiceName(view.effective) : `${choiceName(view.effective)} · ${view.plugin.name}`),
            h('span', { class: cls('cn-cell-sub', st.tone !== 'off' && st.tone !== 'good' && `tone-text-${st.tone}`) }, st.word),
          ];
        })();
  const element = h(
    'tr',
    { 'data-lane': lane },
    nameCell(lane),
    state.cell,
    h('td', { class: 'cn-cell-format' }, h('span', null, format.node), carrierLine),
    h('td', { class: 'cn-num cn-cell-num' }, volume.node),
    h('td', { class: 'cn-cell-upmix' }, ...upmix),
    h('td', { class: 'cn-cell-errors' }, errors),
    h('td', { class: 'cn-num cn-cell-num cn-muted', title: s.epochHint }, epoch.node),
  );
  let errorKey = '';
  return {
    element,
    fill(engine) {
      const endpoint: EndpointStatus | null = engine?.endpoints[lane] ?? null;
      if (endpoint === null) {
        state.set('off', s.engineOff, null);
        format.set('-');
        carrierLine.hidden = true;
        volume.set('-');
        epoch.set('-');
        total.set('-');
        errors.classList.remove('has-errors');
        return;
      }
      if (endpoint.unsupported !== null) state.set('warn', text.console.notPlayed, describeUnsupported(endpoint.unsupported));
      else if (endpoint.format === null) state.set('off', s.noInput, null);
      else state.set('good', text.console.receiving, null);
      format.set(endpoint.format === null ? '-' : describeFormat(endpoint.format));
      const container = describeCarrier(endpoint.format);
      carrierLine.hidden = container === null;
      if (container !== null) carrier.set(container);
      volume.set(describeVolume(endpoint.volume_db, endpoint.muted));
      epoch.set(count(endpoint.epoch));
      const entries = chainErrorEntries(endpoint.chain_errors);
      const sum = chainErrorTotal(endpoint.chain_errors);
      total.set(sum === 0 ? s.chainErrorsNone : count(sum));
      errors.classList.toggle('has-errors', sum > 0);
      // Only a count with kinds behind it opens; "none" is not a control.
      setAttr(summary, 'tabindex', sum > 0 ? '0' : '-1');
      const key = entries.map((entry) => `${entry.kind}:${entry.count}`).join(',');
      if (key !== errorKey) {
        errorKey = key;
        kinds.replaceChildren(
          ...entries.map((entry) => h('li', null, h('span', null, entry.label), h('span', { class: 'cn-num' }, count(entry.count)))),
        );
      }
      if (sum === 0) errors.open = false;
    },
  };
}

function asioRow(): EntranceRow {
  const s = text.status;
  const state = stateCell();
  const format = liveText('-');
  const underruns = liveText('-');
  const element = h(
    'tr',
    { 'data-lane': ASIO_LANE },
    nameCell(ASIO_LANE),
    state.cell,
    h('td', { class: 'cn-cell-format' }, format.node),
    h('td', { class: 'cn-num cn-cell-num cn-muted' }, '-'),
    h('td', { class: 'cn-muted' }, '-'),
    h('td', { class: 'cn-cell-errors cn-num' }, underruns.node),
    h('td', { class: 'cn-num cn-cell-num cn-muted' }, '-'),
  );
  return {
    element,
    fill(engine) {
      if (engine === null) {
        state.set('off', s.engineOff, null);
        format.set('-');
        underruns.set('-');
        return;
      }
      const asio = engine.asio ?? null;
      if (asio === null) {
        state.set('off', text.console.waiting, s.asioWaiting);
        format.set('-');
        underruns.set('-');
        return;
      }
      if (asio.fault_reason !== null) {
        state.set('crit', s.asioFailed, asio.fault_reason);
        format.set('-');
        underruns.set('-');
        return;
      }
      if (asio.clock_fault) state.set('warn', text.console.clockStandIn, s.asioClockFault);
      else state.set('good', text.console.connected, asio.client_exe || null);
      format.set(asioFormat(asio));
      underruns.set(s.asioUnderruns(count(asio.underruns)));
    },
  };
}

function entrances(b: Build): Page {
  const s = text.status;
  const { snapshot } = b.ctx;
  const rows: EntranceRow[] = lanesOf(snapshot).map((lane) => (lane === ASIO_LANE ? asioRow() : endpointRow(snapshot, lane)));
  const element = section(b, 'entrances', s.entrances, [
    table({
      labelledBy: 'cn-sec-entrances-title',
      head: [s.entrances, s.stateLabel, text.upmix.pluginFormat, s.volume, text.upmix.choiceLabel, s.chainErrors, s.epoch],
      rows: rows.map((entry) => entry.element),
      className: 'cn-entrance-table',
    }),
  ]);
  for (const entry of rows) entry.fill(null);
  return { element, fill: (engine) => rows.forEach((entry) => entry.fill(engine)) };
}

// The output beside the map --------------------------------------------------

function outputSummary(b: Build): HTMLElement {
  const { ctx } = b;
  const { snapshot, view } = ctx;
  const s = text.status;
  const p = text.profile;
  const { profile } = snapshot;
  const ready = snapshot.phase.kind === 'ready';
  const rows: HTMLElement[] = [
    row(p.outputTitle, [h('span', { class: 'cn-value' }, p.kindNames[profile.kind])]),
    row(text.console.profileName, [h('span', { class: 'cn-value' }, profile.name)]),
  ];
  if (profile.kind === 'headphones') {
    rows.push(row(p.brir, [h('span', { class: profile.headphones.brir === null ? 'cn-muted' : 'cn-path' }, profile.headphones.brir ?? p.none)]));
  } else if (profile.speakers.correction !== null) {
    rows.push(row(p.correctionTitle, [h('span', { class: 'cn-value' }, profile.speakers.correction.title)]));
  } else {
    rows.push(row(p.layout, [h('span', { class: 'cn-value' }, profile.speakers.layout ?? p.layoutFromFile)]));
  }
  rows.push(row(text.console.gain, [h('span', { class: 'cn-value cn-num' }, `${oneDecimal(profile.gain_db)} ${p.gainUnit}`)]));
  rows.push(
    row(text.console.device, [
      h(
        'span',
        { class: cls('cn-value', snapshot.chosen_output === null && 'cn-muted') },
        snapshot.chosen_output?.name ?? s.noOutput,
      ),
      ready
        ? plain(
            b,
            view.outputsOpen ? s.closeOutputs : snapshot.chosen_output === null ? s.choose : s.change,
            () => ctx.setView({ outputsOpen: !view.outputsOpen }),
            {
              key: 'outputs-toggle',
              variant: 'quiet',
              attrs: { 'aria-expanded': view.outputsOpen ? 'true' : 'false', 'aria-controls': 'cn-output-list' },
            },
          )
        : null,
    ]),
  );
  rows.push(
    row(text.settings.outputPath, [
      h(
        'span',
        { class: 'cn-value' },
        snapshot.output_path === 'kernel_streaming' ? text.meters.kernelStreaming : text.settings.outputPathWasapi,
      ),
    ]),
  );
  const list = ready && view.outputsOpen ? h('div', { id: 'cn-output-list', class: 'cn-output-pick' }, outputList(b, 'status-output')) : null;
  return section(
    b,
    'output',
    s.output,
    [
      h('div', { class: 'cn-rows' }, ...rows),
      list,
      snapshot.phase.kind === 'running' && snapshot.roles_taken ? h('p', { class: 'cn-hint' }, s.rolesTaken) : null,
      h(
        'div',
        { class: 'cn-actions' },
        plain(b, text.console.show(text.nav.profile), () => ctx.setView({ tab: 'profile' }), { key: 'output-profile', variant: 'quiet' }),
      ),
    ],
    { className: 'cn-output-summary' },
  );
}

// Every other meter (law 3: grouped by basis) ---------------------------------

type MetricRow = [label: string, unit: string, read: (engine: EngineStatus) => string];

function metricGroups(): [basis: string, rows: MetricRow[]][] {
  const m = text.meters;
  const c = text.console;
  return [
    [
      c.basisWindow,
      [
        [m.latencyP50, m.ms, (e) => (e.latency.samples === 0 ? '-' : oneDecimal(e.latency.p50_ms))],
        [m.latencyP99, m.ms, (e) => (e.latency.samples === 0 ? '-' : oneDecimal(e.latency.p99_ms))],
        [m.ringLevel, m.ms, (e) => oneDecimal(e.ring_level_ms)],
        [m.clock, '', describeClock],
      ],
    ],
    [
      c.basisSinceOn,
      [
        [m.underruns, m.times, (e) => count(e.underruns)],
        [m.outputRestarts, m.times, (e) => count(e.output_restarts)],
        [c.reconnects, m.times, (e) => count(e.output_reconnects)],
        [c.reconnectAttempts, m.times, (e) => count(e.output_reconnect_attempts)],
        [c.lateResyncs, m.times, (e) => count(e.output_late_resyncs)],
        [m.decodeRefused, m.times, (e) => count(e.decode_refused)],
        [m.skippedRecords, m.times, (e) => count(e.skipped_records)],
        [c.unattributedErrors, m.times, (e) => count(chainErrorTotal(e.unattributed_chain_errors))],
        [c.droppedFrames, c.frames, (e) => count(e.dropped_frames)],
        [m.lostBytes, m.bytes, (e) => count(e.lost_bytes)],
      ],
    ],
    [
      text.status.output,
      [
        [m.outputMode, '', describeOutputMode],
        [m.output, '', describeOutputFormat],
        [
          c.ksFallback,
          '',
          (e) =>
            e.output_ks_fallback_reason === null
              ? '-'
              : `${e.output_ks_fallback_reason} (0x${(e.output_ks_fallback_code >>> 0).toString(16).padStart(8, '0')})`,
        ],
      ],
    ],
  ];
}

function metrics(b: Build, running: boolean): Page {
  const c = text.console;
  const { snapshot } = b.ctx;
  if (!running) {
    return { element: section(b, 'metrics', text.status.meters, [para(c.metricsOff, 'cn-muted')]), fill: () => undefined };
  }
  const setters: ((engine: EngineStatus) => void)[] = [];
  const rows: HTMLTableRowElement[] = [];
  for (const [basis, group] of metricGroups()) {
    rows.push(h('tr', { class: 'cn-group-row' }, h('th', { scope: 'colgroup', colspan: 3 }, basis)));
    for (const [label, unit, read] of group) {
      const value = liveText('-');
      rows.push(
        h('tr', null, h('th', { scope: 'row' }, label), h('td', { class: 'cn-num cn-cell-num' }, value.node), h('td', { class: 'cn-muted' }, unit)),
      );
      setters.push((engine) => value.set(read(engine)));
    }
  }
  rows.push(
    h(
      'tr',
      null,
      h('th', { scope: 'row' }, c.roles),
      h('td', { class: 'cn-cell-num' }, snapshot.roles_taken ? c.rolesYes : c.rolesNo),
      h('td', null),
    ),
  );
  const element = section(b, 'metrics', text.status.meters, [
    table({ labelledBy: 'cn-sec-metrics-title', head: [c.metric, c.value, c.unit], rows, className: 'cn-metric-table' }),
  ]);
  return { element, fill: (engine) => setters.forEach((set) => set(engine)) };
}

// The page --------------------------------------------------------------------

export function overview(b: Build, engineNow: EngineStatus | null, traces: Traces, selection: SelectionStore): Page {
  const { snapshot } = b.ctx;
  const running = snapshot.phase.kind === 'running';
  const engine = running ? engineNow : null;
  const strip = kpiStrip(b, running && engine !== null, traces);
  const past = history(b, snapshot.health, selection);
  const flowMap = flow(b, running && engine !== null);
  const table = entrances(b);
  const map = sceneMap(engine?.directions ?? snapshot.profile.directions, engine !== null, lanesOf(snapshot));
  const mapSection = section(b, 'map', text.profile.mapTitle, [map.element], { className: 'cn-map-section' });
  const summary = outputSummary(b);
  const meters = metrics(b, engine !== null);
  const element = h(
    'div',
    { class: 'cn-page cn-overview' },
    strip.element,
    past,
    flowMap.element,
    table.element,
    h('div', { class: 'cn-split' }, mapSection, summary),
    meters.element,
  );
  const fill = (next: EngineStatus) => {
    strip.fill(next);
    flowMap.fill(next);
    table.fill(next);
    map.update(next);
    meters.fill(next);
  };
  if (engine !== null) fill(engine);
  return { element, fill, traces: strip.traces ?? (() => undefined) };
}
