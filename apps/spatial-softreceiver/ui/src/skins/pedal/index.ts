// The pedal skin (ADR 0046, 이펙터의 법칙): the window is one effects pedal
// of today. Every function has its own face on the same box (status,
// profile, upmix, settings, setup, quit), and everything on it is a part of
// the box: jacks, LEDs, knobs, switches, keys and print. What the parts do
// is the controller every skin shares (src/skins/controller.ts); this file
// only builds the box from a frame and keeps what is only seen (the
// encoder's turn, the open app row, where the face was scrolled, the scale).

import './pedal.css';
import type { Api } from '../../api';
import { text } from '../../text';
import type { Drawn, Live } from '../../screens/common';
import { CAPTURE_LANES, appMode, laneView } from '../../screens/entrance';
import { type Frame, mountWith } from '../controller';
import type { App, AppOptions } from '../types';
import { band, notes } from './bands';
import { type ScrollHold, enclosure, footswitch, indicators, jackStrip } from './frame';
import type { KnobHold } from './parts';
import { profileFace } from './profile';
import { type Scope, profileOpen } from './scope';
import { settingsFace } from './settings';
import { statusFace } from './status';
import { upmixFace } from './upmix';
import { quitFace, wizardFace } from './wizard';

/** The layout every part is drawn for; a larger window scales the parts up to half again. */
const DESIGN: [number, number] = [1256, 776];
const MOST = 1.5;

/** Parts grow with the window, from the 1280 × 800 layout up to half again. */
function fit(root: HTMLElement): void {
  if (typeof window === 'undefined') return;
  const apply = () => {
    const width = window.innerWidth - 24;
    const height = window.innerHeight - 24;
    const k = Math.min(MOST, Math.max(1, Math.min(width / DESIGN[0], height / DESIGN[1])));
    root.style.setProperty('--pd-k', k.toFixed(3));
  };
  apply();
  window.addEventListener('resize', apply);
}

export function mount(root: HTMLElement, api: Api, options: AppOptions = {}): App {
  fit(root);
  // Seen only, never kept: the knobs' turns, the open app row, the face's scroll.
  const gainHold: KnobHold = { drag: null, wheel: null };
  const asioHold: KnobHold = { drag: null, wheel: null };
  let picking: number | null = null;
  let scrolled: ScrollHold | null = null;
  let watching: ResizeObserver | null = null;

  function render(frame: Frame): Drawn {
    const { ctx } = frame;
    const { snapshot, view } = ctx;
    const phase = snapshot.phase;
    const able = profileOpen(snapshot);
    const running = phase.kind === 'running';
    const engineNow = running ? frame.engine : null;

    // An app row has nothing to offer once the lanes cannot change.
    if (phase.kind !== 'ready' && phase.kind !== 'faulted') picking = null;
    const readouts: ((db: number) => void)[] = [];
    const settled = ctx.pending === 'set_gain' && view.gainSending !== null ? view.gainSending : snapshot.profile.gain_db;
    const scope: Scope = {
      ctx,
      frame,
      gain: {
        settled,
        value: () => gainHold.drag ?? settled,
        follow: (set) => readouts.push(set),
        hold: gainHold,
        turned: (db) => {
          for (const set of readouts) set(db);
        },
      },
      picker: {
        open: picking,
        set(lane) {
          picking = lane;
          ctx.setView({});
        },
      },
      asioHold,
    };

    // The box's parts that offer an intent first, then the face, then the
    // bands, which leave out what the box or the face already offers.
    const power = footswitch(ctx);
    let face: Drawn;
    if (phase.kind === 'setup') face = wizardFace(ctx, phase.value);
    else if (phase.kind === 'quit') face = quitFace();
    else {
      switch (view.tab) {
        case 'status':
          face = statusFace(scope, engineNow);
          break;
        case 'profile':
          face = profileFace(scope, engineNow);
          break;
        case 'upmix':
          face = upmixFace(scope);
          break;
        case 'settings':
          face = settingsFace(scope);
          break;
      }
    }
    const kept: HTMLElement[] = [];
    const refused: HTMLElement[] = [];
    frame.errors.forEach((entry, index) => (entry.kept ? kept : refused).push(band(ctx, frame, entry, index)));

    // The app-sound entrance's jacks light for its lanes with an app (ADR 0043).
    const lit = appMode(snapshot) ? CAPTURE_LANES.filter((lane) => laneView(snapshot, lane) !== null) : [0, 1, 2];
    const strip = jackStrip(ctx, lit);
    strip.update(engineNow);
    const lamps = indicators(ctx, frame.engine);

    const element = enclosure({
      ctx,
      strip,
      power,
      lamps,
      face: face.element,
      faceName: able ? view.tab : phase.kind,
      tabbed: able,
      top: [kept.length === 0 ? null : groupOf(kept), notes(snapshot.notes)],
      refused,
      scroll: {
        held: scrolled,
        keep: (hold) => {
          scrolled = hold;
        },
      },
      watch: (observer) => {
        watching?.disconnect();
        watching = observer;
      },
    });

    if (!running) return { element };
    const live: Live = {
      update(engine) {
        strip.update(engine);
        lamps.update(engine);
        face.live?.update(engine);
      },
    };
    return { element, live };
  }

  return mountWith(root, api, options, { render });
}

/** The snapshot's error bands as one labelled group. */
function groupOf(bands: HTMLElement[]): HTMLElement {
  const group = document.createElement('div');
  group.setAttribute('role', 'group');
  group.setAttribute('aria-label', text.banner.errorLabel);
  group.append(...bands);
  return group;
}
