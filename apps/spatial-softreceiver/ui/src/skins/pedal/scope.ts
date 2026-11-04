// What the pedal's faces get for one frame (ADR 0046): the controller's
// frame, the gain the LEVEL encoder shows now and the readouts that follow it
// while it turns, which lane's app row stands open, and what the snapshot
// says each light of the box should do. Nothing here outlives a reload; the
// encoder's turn and the open row are only seen, never kept.

import type { EngineStatus, Intent, Snapshot } from '../../bindings';
import { oneDecimal } from '../../format';
import { text } from '../../text';
import type { Context } from '../../screens/common';
import type { Frame } from '../controller';
import type { KnobHold } from './parts';

export interface GainReadouts {
  /** The gain in flight (`view.gainSending`) or else the snapshot's: what the encoder returns to. */
  readonly settled: number;
  /** The gain shown now: the encoder's while it turns, else `settled`. */
  value(): number;
  /** A readout that follows the encoder while it turns, without a redraw. */
  follow(set: (db: number) => void): void;
  /** The encoder's turn, kept across frames. */
  readonly hold: KnobHold;
  /** Every step of a turn: the readouts follow. */
  turned(db: number): void;
}

/** Which lane's app row stands open (ADR 0043): seen only, gone on reload. */
export interface PickerHold {
  readonly open: number | null;
  set(lane: number | null): void;
}

export interface Scope {
  readonly ctx: Context;
  readonly frame: Frame;
  readonly gain: GainReadouts;
  readonly picker: PickerHold;
  /** The ASIO attenuation knob's turn, kept across frames. */
  readonly asioHold: KnobHold;
}

/** U+2212, the minus a readout prints. */
const MINUS = '−';

/** One decimal with its sign: -3.5 as "−3.5", 2 as "+2.0", 0 as "0.0". */
export function signed(value: number): string {
  const words = oneDecimal(value);
  if (words.startsWith('-')) return `${MINUS}${words.slice(1)}`;
  return Number(words) > 0 ? `+${words}` : words;
}

/** The lanes with their own colour: 0 game, 1 music, 2 PASS; 3 is the ASIO input. */
export const LANES = 3;
export const ASIO_LANE = 3;

/** The profile, the gain and the faces can change once the wizard is done. */
export function profileOpen(snapshot: Snapshot): boolean {
  const phase = snapshot.phase.kind;
  return phase === 'ready' || phase === 'running' || phase === 'faulted';
}

export type Lamp = 'off' | 'on' | 'wait' | 'fault';

/** The power lamp: green running, amber turning on or reconnecting, red stopped. */
export function powerLamp(snapshot: Snapshot, pending: Intent['kind'] | null, engine: EngineStatus | null): Lamp {
  if (pending === 'enable') return 'wait';
  switch (snapshot.phase.kind) {
    case 'running':
      return (engine ?? snapshot.engine)?.fault ? 'wait' : 'on';
    case 'faulted':
      return 'fault';
    default:
      return 'off';
  }
}

/** The power state in words, for the lamp's screen-reader line. */
export function powerWord(lamp: Lamp, snapshot: Snapshot): string {
  switch (lamp) {
    case 'wait':
      return snapshot.phase.kind === 'running' ? text.status.reconnecting : text.status.powerTurningOn;
    case 'on':
      return text.status.on;
    case 'fault':
      return text.status.stopped;
    case 'off':
      return text.status.off;
  }
}

/**
 * The upmix lamp: the output kind in use applies the upmixers (its switch is
 * on) and some input's upmixer is on, a VST one with its plugin chosen. This
 * is what the upmix face sets up; whether the sound playing now goes around
 * it is the face's to say.
 */
export function upmixApplied(snapshot: Snapshot): boolean {
  const { upmix, profile } = snapshot;
  if (!upmix[profile.kind]) return false;
  return upmix.endpoints.some((view) => view.effective === 'first_party' || (view.effective === 'plugin' && view.plugin !== null));
}

/** The BRIR lamp: the headphones are the output in use and render through a BRIR set. */
export function brirInUse(snapshot: Snapshot): boolean {
  const { profile } = snapshot;
  return profile.kind === 'headphones' && (profile.headphones.brir !== null || profile.from_file);
}

export type Signal = 'sound' | 'idle' | 'none';

/**
 * Each input's state for its jack's LED: sound now (an object of its lane in
 * the last mixed block), a format but silent, or nothing. The ASIO input is
 * lit while a DAW plays into it and dim while its sound waits on a stand-in
 * clock. With the app-sound entrance (ADR 0043) only its lanes with an app
 * can light; PASS, which only the driver feeds, stays dark.
 */
export function signals(engine: EngineStatus | null, lit: readonly number[] = [0, 1, 2]): Signal[] {
  const states: Signal[] = ['none', 'none', 'none', 'none'];
  if (engine === null) return states;
  for (let lane = 0; lane < LANES; lane += 1) {
    if (lit.includes(lane) && engine.endpoints[lane]?.format) states[lane] = 'idle';
  }
  for (const object of engine.scene) {
    if (object.lane < LANES && lit.includes(object.lane)) states[object.lane] = 'sound';
  }
  const asio = engine.asio ?? null;
  if (asio !== null && asio.fault_reason === null) states[ASIO_LANE] = asio.clock_fault ? 'idle' : 'sound';
  return states;
}

/** The loudest object of a lane in the last mixed block, or -Infinity. */
export function loudest(engine: EngineStatus, lane: number): number {
  let level = -Infinity;
  for (const object of engine.scene) {
    if (object.lane === lane && object.level_dbfs > level) level = object.level_dbfs;
  }
  return level;
}
