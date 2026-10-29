// The health line (law 1): the engine, the driver and the output device in
// one sentence and one status colour, with the next action when there is a
// problem. With the app-sound entrance (ADR 0043) no driver is in use, so
// none is reported; its lanes with an app stand where the endpoints would. Derived from the snapshot, the meters and the error bands alone;
// the supervisor decides, this only names what it says (ADR 0001).

import type { EngineStatus, Intent, Snapshot } from '../../bindings';
import { describeOutputMode, describeUnsupported } from '../../format';
import { ASIO_DEFAULTS } from '../../guard';
import { bannerAction } from '../../screens/banner';
import type { Tab } from '../../screens/common';
import { CAPTURE_LANES, appLabel, appMode, captureStateWord, laneName, laneView } from '../../screens/entrance';
import { text } from '../../text';
import type { Frame } from '../controller';
import { type Tone, basename, entrance } from './kit';

export type HealthAction =
  | { kind: 'intent'; intent: Intent; label: string }
  | { kind: 'choose_output'; label: string }
  | { kind: 'go'; tab: Tab; section: string | null; label: string };

export interface Health {
  tone: Tone | 'busy';
  title: string;
  detail: string | null;
  action: HealthAction | null;
}

/** What the output plays: the kind, with its BRIR set, correction or layout. */
export function listening(snapshot: Snapshot): string {
  const { profile } = snapshot;
  const kind = text.profile.kindNames[profile.kind];
  if (profile.kind === 'headphones') {
    return profile.headphones.brir === null ? kind : `${kind} (BRIR ${basename(profile.headphones.brir)})`;
  }
  const { correction, layout } = profile.speakers;
  if (correction !== null) return `${kind} (${correction.title})`;
  return layout === null ? kind : `${kind} ${layout}`;
}

function device(snapshot: Snapshot): string {
  return snapshot.chosen_output?.name ?? text.status.noOutput;
}

interface Issue {
  title: string;
  detail: string | null;
  go: { tab: Tab; section: string | null } | null;
}

/**
 * What needs attention now, most serious first: the ASIO input, an entrance
 * whose sound is not played, an upmixer that stopped or stands aside, an
 * output that fell back. Counters since the engine started are not issues:
 * the KPI strip shows them with their basis.
 */
export function issues(snapshot: Snapshot, engine: EngineStatus | null): Issue[] {
  const found: Issue[] = [];
  const running = snapshot.phase.kind === 'running';
  const asio = engine?.asio ?? null;
  if (running && (snapshot.asio ?? ASIO_DEFAULTS).enabled && asio !== null) {
    if (asio.fault_reason !== null) {
      found.push({ title: `${text.asio}: ${text.status.asioFailed}`, detail: asio.fault_reason, go: { tab: 'settings', section: 'cn-sec-asio' } });
    } else if (asio.clock_fault) {
      found.push({ title: `${text.asio}: ${text.console.clockStandIn}`, detail: text.status.asioClockFault, go: null });
    }
  }
  // The app-sound entrance takes sound on its lanes with an app (ADR 0043);
  // the driver entrance on all three endpoints.
  const apps = appMode(snapshot);
  const lanes = apps ? CAPTURE_LANES.filter((lane) => laneView(snapshot, lane) !== null) : [0, 1, 2];
  if (running && engine !== null) {
    for (const lane of lanes) {
      const endpoint = engine.endpoints[lane];
      if (endpoint !== undefined && endpoint.unsupported !== null) {
        found.push({
          title: text.console.issueUnsupported(apps ? laneName(lane) : entrance(lane).name),
          detail: describeUnsupported(endpoint.unsupported),
          go: null,
        });
      }
    }
  }
  // A lane whose app ended waits, silent, for it to play again.
  if (running && apps) {
    for (const lane of CAPTURE_LANES) {
      const view = laneView(snapshot, lane);
      if (view !== null && (view.state === 'waiting' || view.state === 'not_running')) {
        found.push({ title: `${laneName(lane)}: ${captureStateWord(view.state)}`, detail: appLabel(view), go: null });
      }
    }
  }
  for (const view of snapshot.upmix.endpoints) {
    const name = apps && view.endpoint < 3 ? laneName(view.endpoint) : entrance(view.endpoint).name;
    if (view.state.kind === 'fallback') {
      found.push({ title: `${name}: ${text.upmix.stateFallback}`, detail: null, go: { tab: 'upmix', section: null } });
    } else if (view.state.kind === 'failed') {
      found.push({ title: `${name}: ${view.state.value}`, detail: null, go: { tab: 'upmix', section: null } });
    }
  }
  if (running && engine !== null && (engine.output_fell_back || engine.output_ks_fallback)) {
    found.push({ title: `${text.meters.outputMode}: ${describeOutputMode(engine)}`, detail: null, go: null });
  }
  return found;
}

const pageName = (tab: Tab): string => (tab === 'status' ? text.console.overview : text.nav[tab]);

function issueHealth(issue: Issue, rest: number, fallback: string | null): Health {
  const more = rest > 0 ? text.console.moreIssues(rest) : null;
  const detail = [issue.detail ?? fallback, more].filter((part): part is string => part !== null).join(' ');
  return {
    tone: 'warn',
    title: issue.title,
    detail: detail.length > 0 ? detail : null,
    action:
      issue.go === null ? null : { kind: 'go', tab: issue.go.tab, section: issue.go.section, label: text.console.show(pageName(issue.go.tab)) },
  };
}

/** The band's action as the health line offers it; a dismiss stays with the band. */
function errorAction(frame: Frame): HealthAction | null {
  const first = frame.errors[0];
  if (first === undefined) return null;
  const action = bannerAction(frame.ctx.snapshot.phase, first.error.next);
  switch (action.kind) {
    case 'intent':
      return { kind: 'intent', intent: action.intent, label: action.label };
    case 'choose_output':
      return { kind: 'choose_output', label: action.label };
    case 'dismiss':
    case 'nothing':
      return null;
  }
}

function setupHealth(frame: Frame): Health {
  const { snapshot } = frame.ctx;
  const failed = frame.errors.length > 0;
  const w = text.wizard;
  const c = text.console;
  if (snapshot.phase.kind !== 'setup') throw new Error('setup health outside setup');
  switch (snapshot.phase.value) {
    case 'welcome':
      return { tone: 'off', title: c.setupWelcome, detail: c.setupWelcomeDetail, action: null };
    case 'unsupported':
      return { tone: 'crit', title: appMode(snapshot) ? w.appUnsupportedTitle : w.unsupportedTitle, detail: null, action: null };
    case 'not_test_signing':
      return { tone: 'crit', title: w.testSigningTitle, detail: null, action: null };
    case 'driver':
      return {
        tone: failed ? 'crit' : 'warn',
        title: snapshot.driver?.kind === 'installed' ? c.setupDriverProblem : c.setupDriver,
        detail: null,
        action: null,
      };
    case 'reboot_required':
      return { tone: 'warn', title: w.rebootTitle, detail: null, action: null };
    case 'choose_output':
      return { tone: failed ? 'crit' : 'warn', title: w.outputTitle, detail: null, action: null };
    case 'choose_discard':
      return { tone: failed ? 'crit' : 'warn', title: w.discardTitle, detail: null, action: null };
  }
}

export function health(frame: Frame): Health {
  const { ctx, engine, errors } = frame;
  const { snapshot, pending } = ctx;
  const c = text.console;
  const phase = snapshot.phase;
  const plan = c.healthPlan(listening(snapshot), device(snapshot));

  switch (phase.kind) {
    case 'quit':
      return { tone: 'off', title: text.wizard.quitTitle, detail: text.wizard.quitBody, action: null };
    case 'setup':
      return setupHealth(frame);
    case 'faulted':
      return {
        tone: 'crit',
        title: c.healthFaulted,
        detail: null,
        action: errorAction(frame) ?? { kind: 'intent', intent: { kind: 'recheck' }, label: text.banner.recheck },
      };
    case 'ready': {
      if (pending === 'enable') return { tone: 'busy', title: c.healthTurningOn, detail: plan, action: null };
      if (errors.length > 0) {
        const tone = errors[0]!.error.next === 'none' ? 'warn' : 'crit';
        return { tone, title: `${c.healthOff} ${c.checkBelow}`, detail: plan, action: errorAction(frame) };
      }
      if (snapshot.chosen_output === null) {
        return { tone: 'warn', title: c.healthNoOutput, detail: null, action: { kind: 'choose_output', label: text.banner.chooseOutput } };
      }
      const found = issues(snapshot, engine);
      if (found.length > 0) return issueHealth(found[0]!, found.length - 1, plan);
      return { tone: 'off', title: c.healthOff, detail: plan, action: null };
    }
    case 'running': {
      if (pending === 'disable') return { tone: 'busy', title: c.healthTurningOff, detail: null, action: null };
      const playing = c.healthPlaying(listening(snapshot), device(snapshot));
      if (engine?.fault) return { tone: 'warn', title: c.healthReconnecting, detail: engine.fault.value, action: null };
      const kept = errors.find((band) => band.kept);
      if (kept !== undefined) {
        return { tone: 'warn', title: `${c.healthOn} ${c.checkBelow}`, detail: playing, action: errorAction(frame) };
      }
      const found = issues(snapshot, engine);
      if (found.length > 0) return issueHealth(found[0]!, found.length - 1, null);
      return { tone: 'good', title: c.healthGood, detail: playing, action: null };
    }
  }
}
