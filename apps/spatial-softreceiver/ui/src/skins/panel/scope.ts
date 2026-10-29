// What the front panel's parts get for one frame (ADR 0041): the controller's
// frame, the gain the encoder shows now and the readouts that follow it while
// it turns, and the display's OSD. Nothing here outlives a reload except as
// the encoder's drag and the OSD's timer, which are only seen, not kept.

import type { Frame } from '../controller';
import type { Context } from '../../screens/common';
import { oneDecimal } from '../../format';

export interface GainReadouts {
  /** The gain in flight (`view.gainSending`) or else the snapshot's: what the encoder returns to. */
  readonly settled: number;
  /** The gain shown now: the encoder's while it turns, else `settled`. */
  value(): number;
  /** A readout that follows the encoder while it turns, without a redraw. */
  follow(set: (db: number) => void): void;
}

/**
 * Which lane's app list stands open under its row (ADR 0043): seen only,
 * like the encoder's turn, and gone on reload.
 */
export interface PickerHold {
  readonly open: number | null;
  /** Opens a lane's list (null closes it) and redraws. */
  set(lane: number | null): void;
}

export interface Scope {
  readonly ctx: Context;
  readonly frame: Frame;
  readonly gain: GainReadouts;
  readonly picker: PickerHold;
  /** Shows a short line on the display for a moment. */
  osd(line: string): void;
}

/** U+2212, the minus a display shows. */
const MINUS = '−';

/** One decimal with its sign, as a display shows it: -3.5 as "−3.5", 2 as "+2.0", 0 as "0.0". */
export function signed(value: number): string {
  const text = oneDecimal(value);
  if (text.startsWith('-')) return `${MINUS}${text.slice(1)}`;
  return Number(text) > 0 ? `+${text}` : text;
}

/** The lanes the display colours: 0 game, 1 music, 2 PASS. */
export const LANES = 3;
