// What every skin does the same way (ADR 0041): keeps the last snapshot and
// the view state, sends one intent at a time and holds the controls until the
// snapshot that follows lands, keeps a refused intent's error until the state
// moves on, and puts keyboard focus back on the same control after a redraw.
// A skin gives only the drawing (`Renderer`); the behaviour behind its
// controls is this, so no skin can drift from another (ADR 0001).

import { type Api, toUserError } from '../api';
import type { EngineStatus, Intent, Snapshot, UserError } from '../bindings';
import { setLanguage, text } from '../text';
import type { Band } from '../screens/banner';
import type { Context, Drawn, Live, ViewState } from '../screens/common';
import type { App, AppOptions } from './types';

export interface Frame {
  /** What every control needs: the snapshot, the view state and `send`. */
  readonly ctx: Context;
  /** The error bands: a refused intent's first, then the snapshot's unless closed. */
  readonly errors: Band[];
  /** While running, the engine as the meters last said; otherwise the snapshot's. */
  readonly engine: EngineStatus | null;
  /**
   * The intents the drawing offers so far (`ctx.offer`). An error band drawn
   * after the body leaves out an action the body already has.
   */
  readonly offered: ReadonlySet<Intent['kind']>;
  /** Closes a band; a kept error comes back once its sentence changes. */
  dismiss(error: UserError): void;
  /**
   * The band's "choose output": in the wizard, focus on the first output in
   * `[data-region="output-list"]`; otherwise the status page with the
   * output list open (`view.outputsOpen`).
   */
  chooseOutput(): void;
}

export interface Renderer {
  /**
   * Draws the whole window for one frame. The returned element replaces the
   * root's children; `live` gets the 30 Hz meters until the next frame.
   * Controls keep focus across frames through a stable `data-key`.
   */
  render(frame: Frame): Drawn;
}

/**
 * The parts of a snapshot whose change ends a refused intent's band. The
 * health history moves on by itself every minute (ADR 0041), so it is left
 * out.
 */
function stateKey(snapshot: Snapshot): string {
  const { health: _, ...rest } = snapshot;
  return JSON.stringify({
    ...rest,
    engine: snapshot.engine === null ? null : { fault: snapshot.engine.fault },
  });
}

/** How long the controls wait for the snapshot that follows an intent. */
const SETTLE_MS = 1000;

const sameError = (a: UserError | null, b: UserError | null) =>
  a !== null && b !== null && a.message === b.message && a.next === b.next;

export function mountWith(root: HTMLElement, api: Api, options: AppOptions, renderer: Renderer): App {
  const view: ViewState = {
    tab: options.tab ?? 'status',
    outputsOpen: false,
    profileTab: null,
    dismissed: null,
    gainSending: null,
    autostart: null,
    autostartWanted: null,
    update: null,
  };
  let snapshot: Snapshot | null = null;
  let lastKey = '';
  let transient: UserError | null = null;
  let pending: Intent['kind'] | null = null;
  let sending: Intent | null = null;
  let live: Live | null = null;
  let meters: EngineStatus | null = null;
  let focusKey: string | null = null;

  function setView(patch: Partial<ViewState>) {
    Object.assign(view, patch);
    render();
  }

  function report(error: UserError) {
    transient = error;
    render();
  }

  /** Resolves at the next drawn snapshot, or after `ms` without one. */
  let nextDraw: (() => void)[] = [];
  function snapshotLanded(ms: number): Promise<void> {
    return new Promise((resolve) => {
      const timer = setTimeout(resolve, ms);
      nextDraw.push(() => {
        clearTimeout(timer);
        resolve();
      });
    });
  }

  async function send(intent: Intent, after?: (ok: boolean) => void): Promise<void> {
    if (pending !== null) return;
    pending = intent.kind;
    sending = intent;
    transient = null;
    render();
    let ok = true;
    try {
      await api.dispatch(intent);
    } catch (reason) {
      ok = false;
      transient = toUserError(reason);
    }
    after?.(ok);
    // The shell replies first and publishes the snapshot right after
    // (apps/spatial-softreceiver/src/control.rs); the controls stay locked
    // until it lands so they do not flash back to the old state.
    render();
    await snapshotLanded(SETTLE_MS);
    pending = null;
    sending = null;
    render();
  }

  function chooseOutput() {
    if (snapshot?.phase.kind === 'setup') {
      root.querySelector<HTMLElement>('[data-region="output-list"] button')?.focus();
      return;
    }
    setView({ tab: 'status', outputsOpen: true });
  }

  function dismiss(error: UserError) {
    if (sameError(error, transient)) transient = null;
    if (sameError(error, snapshot?.error ?? null)) view.dismissed = error.message;
    render();
  }

  function render() {
    if (snapshot === null) return;
    setLanguage(snapshot.language);
    document.documentElement.lang = snapshot.language;
    document.title = text.product;
    const active = document.activeElement;
    const activeKey = active instanceof HTMLElement && root.contains(active) ? (active.dataset.key ?? null) : null;
    focusKey = activeKey ?? focusKey;

    const offered = new Set<Intent['kind']>();
    const ctx: Context = {
      snapshot,
      view,
      busy: pending !== null,
      pending,
      sending,
      shell: api.shell,
      send: (intent, after) => void send(intent, after),
      setView,
      report,
      offer: (kind) => offered.add(kind),
    };

    const errors: Band[] = [];
    const kept = snapshot.error;
    if (transient !== null && !sameError(transient, kept)) errors.push({ error: transient, kept: false });
    if (kept !== null && !(kept.next === 'none' && view.dismissed === kept.message)) {
      errors.push({ error: kept, kept: true });
    }

    const engine = snapshot.phase.kind === 'running' ? (meters ?? snapshot.engine) : snapshot.engine;
    const drawn = renderer.render({ ctx, errors, engine, offered, dismiss, chooseOutput });
    live = drawn.live ?? null;
    root.replaceChildren(drawn.element);
    root.dataset.ready = 'true';

    if (focusKey !== null) {
      const target = root.querySelector<HTMLElement>(`[data-key="${CSS.escape(focusKey)}"]`);
      if (target !== null && !(target as HTMLButtonElement).disabled) {
        // A redraw must not scroll the page back to the focused control.
        target.focus({ preventScroll: true });
        focusKey = null;
      } else if (target === null) {
        focusKey = null;
      }
    }
  }

  function draw(next: Snapshot) {
    const key = stateKey(next);
    if (key !== lastKey) {
      // A refused intent's band lasts until the state moves on.
      if (lastKey !== '') transient = null;
      lastKey = key;
    }
    if (view.dismissed !== null && next.error?.message !== view.dismissed) view.dismissed = null;
    if (next.phase.kind !== 'ready') view.outputsOpen = false;
    meters = null;
    snapshot = next;
    render();
    const waiting = nextDraw;
    nextDraw = [];
    for (const resolve of waiting) resolve();
  }

  function updateMeters(engine: EngineStatus) {
    if (snapshot?.phase.kind !== 'running') return;
    meters = engine;
    live?.update(engine);
  }

  if (api.shell.native) {
    api.shell.updateState().then(
      (update) => setView({ update }),
      () => setView({ update: null }),
    );
    void api.shell.onUpdate((update) => setView({ update }));
    api.shell.autostartEnabled().then(
      (enabled) => setView({ autostart: enabled }),
      () => setView({ autostart: null }),
    );
  }

  return { draw, updateMeters, report, send: (intent) => send(intent) };
}
