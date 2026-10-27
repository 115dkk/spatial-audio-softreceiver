import './base.css';
import { connect, inTauri, toUserError } from './api';
import type { Skin, Snapshot, UserError } from './bindings';
import { h } from './dom';
import { type App, type AppOptions, isSkin, loadSkin, skinOf } from './skins';
import { text } from './text';
import type { Tab } from './screens/common';

const root = document.getElementById('app');
if (root === null) throw new Error('#app is missing from index.html');

const params = new URLSearchParams(location.search);

function tabParam(): Tab | undefined {
  const tab = params.get('tab');
  return tab === 'status' || tab === 'profile' || tab === 'upmix' || tab === 'settings' ? tab : undefined;
}

/**
 * The page holds one skin's code and stylesheet, so another skin is drawn by
 * loading the page again (ADR 0041). In the shell the next load reads the
 * skin from the supervisor; in a browser over the mock it rides in `?skin=`.
 */
let reloading = false;
function reloadFor(skin: Skin): void {
  if (reloading) return;
  reloading = true;
  if (inTauri()) {
    location.reload();
    return;
  }
  const url = new URL(location.href);
  url.searchParams.set('skin', skin);
  location.replace(url);
}

async function start(host: HTMLElement) {
  if (params.has('gallery')) {
    const { gallery } = await import('./gallery');
    await gallery(host, params, tabParam());
    return;
  }
  const api = await connect(params.get('fixture') ?? undefined);
  const tab = tabParam();
  const options: AppOptions = tab === undefined ? {} : { tab };

  // Snapshots and problems that come before the skin is drawn wait here; a
  // snapshot event is newer than the reply to `snapshot()`.
  let app: App | null = null;
  let mounted: Skin | null = null;
  let waiting: Snapshot | null = null;
  const problems: UserError[] = [];
  await api.onSnapshot(
    (snapshot) => {
      // The shell's stand-in snapshot after quitting carries no saved
      // choice; the closing screen stays in the skin it is in.
      const skin = skinOf(snapshot);
      if (mounted !== null && skin !== mounted && snapshot.phase.kind !== 'quit') {
        reloadFor(skin);
      } else if (app === null) {
        waiting = snapshot;
      } else {
        app.draw(snapshot);
      }
    },
    (error) => (app === null ? problems.push(error) : app.report(error)),
  );
  const first = waiting ?? (await api.snapshot());

  // Over the mock a page asked for by `?skin=` tells the mock first, so the
  // snapshots it sends agree with the skin drawn.
  const asked = inTauri() ? null : params.get('skin');
  const skin = isSkin(asked) ? asked : skinOf(first);
  if (skin !== skinOf(first)) await api.dispatch({ kind: 'set_skin', value: skin });

  const module = await loadSkin(skin);
  mounted = skin;
  document.documentElement.dataset.skin = skin;
  const drawn = module.mount(host, api, options);
  app = drawn;
  drawn.draw(waiting ?? first);
  waiting = null;
  for (const problem of problems.splice(0)) drawn.report(problem);
  await api.onMeters(drawn.updateMeters);
}

start(root).catch((reason: unknown) => {
  // The gallery's pictures must fail on this (as a page error), not show it.
  if (params.has('gallery')) {
    setTimeout(() => {
      throw reason;
    }, 0);
  }
  // Nothing to draw without a snapshot: say why and offer a reload.
  const error = toUserError(reason);
  root.replaceChildren(
    h(
      'div',
      { class: 'fatal', role: 'alert' },
      h('p', null, error.message),
      h('button', { type: 'button', onClick: () => location.reload() }, text.banner.retry),
    ),
  );
});
