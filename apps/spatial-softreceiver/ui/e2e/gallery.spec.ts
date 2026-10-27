// The screen gallery's pictures: every scene of gallery.json in every skin
// (ADR 0041), every language and each of its palettes, one test and one
// picture apiece, as <skin>/<name>.<language>.<scheme>.png in
// $SSR_UI_SHOTS_DIR (else ui/shots/).
// tools/ui-gallery-issue.mjs checks the folder against the same table.
//
// Each picture is a fresh browser context with the language's locale, the
// scene's window size and the palette. Before and after the picture the page
// must have no console error or warning and no page error, the skin's fonts in
// place, the scene's language on <html>, no horizontal overflow, and Hangul
// only on the Korean pages.

import { existsSync, mkdirSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { expect, type Page, test } from '@playwright/test';
import { SKINS } from '../src/skins/names';

type Language = 'ko' | 'en';
type Scheme = 'light' | 'dark';
type Tab = 'status' | 'profile' | 'upmix' | 'settings';
type Skin = (typeof SKINS)[number];

interface SceneEntry {
  name: string;
  group: string;
  fixture: string;
  tab: Tab | null;
  size?: [number, number];
  schemes?: Scheme[];
  skinSteps?: Partial<Record<string, unknown[]>>;
}

interface Table {
  skins: Skin[];
  skinNames: Record<string, string>;
  languages: Language[];
  defaults: { size: [number, number]; schemes: Scheme[] };
  groups: { id: string }[];
  scenarios: SceneEntry[];
}

const readJson = <T>(path: string | URL): T =>
  JSON.parse(readFileSync(typeof path === 'string' ? new URL(path, import.meta.url) : path, 'utf8')) as T;

const table = readJson<Table>('./gallery.json');
const fixtureNames = readJson<string[]>('../fixtures/index.json');
const fixturePath = (name: string, language: Language) =>
  new URL(language === 'ko' ? `../fixtures/${name}.json` : `../fixtures/${language}/${name}.json`, import.meta.url);

const scenes = table.scenarios.map((scene) => ({
  ...scene,
  size: scene.size ?? table.defaults.size,
  schemes: scene.schemes ?? table.defaults.schemes,
}));

const LOCALES: Record<Language, string> = { ko: 'ko-KR', en: 'en-US' };
const HANGUL = /[가-힣ᄀ-ᇿ㄰-㆏]/u;
const READY = 'html[data-gallery-ready="true"]';
/** The faces each skin must have in place before its picture (ADR 0041). */
const FONTS: Record<Skin, string[]> = {
  standard: ['400 14px "IBM Plex Sans KR"'],
  panel: ['400 15px "Galmuri14"', '400 14px "IBM Plex Sans KR"'],
  console: ['500 14px "Pretendard"'],
};

test.describe.configure({ mode: 'parallel' });

test('the scene table covers every fixture, both languages and every main tab', () => {
  // The gallery shows every skin the screen can load, each once.
  expect(table.skins, 'the gallery skins').toEqual([...SKINS]);
  expect(Object.keys(table.skinNames).sort(), 'skinNames').toEqual([...SKINS].sort());
  const strange = scenes.flatMap((scene) =>
    Object.keys(scene.skinSteps ?? {})
      .filter((skin) => !SKINS.includes(skin as Skin))
      .map((skin) => `${scene.name}: ${skin}`),
  );
  expect(strange, 'skinSteps for a skin that does not exist').toEqual([]);

  const names = scenes.map((scene) => scene.name);
  expect(names.filter((name, index) => names.indexOf(name) !== index), 'scene names listed twice').toEqual([]);

  const used = new Set(scenes.map((scene) => scene.fixture));
  expect(fixtureNames.filter((name) => !used.has(name)), 'fixtures in no scene').toEqual([]);

  // The app speaks exactly Korean and English; every scene is shown in both,
  // in at least one palette, none twice.
  expect([...table.languages].sort(), 'the gallery languages').toEqual(['en', 'ko']);
  const palettes = scenes.filter((scene) => scene.schemes.length === 0 || new Set(scene.schemes).size !== scene.schemes.length);
  expect(palettes.map((scene) => scene.name), 'scenes without a palette or with one twice').toEqual([]);
  expect(table.languages.filter((language) => !(language in LOCALES)), 'languages without a locale').toEqual([]);
  const missing = scenes.flatMap((scene) =>
    table.languages.filter((language) => !existsSync(fixturePath(scene.fixture, language))).map(
      (language) => `${scene.name}: ${language} ${scene.fixture}`,
    ),
  );
  expect(missing, 'scenes whose fixture is missing in a language').toEqual([]);

  // The main screens (ready, running, faulted) are shot on a tab, the wizard
  // and the quit screen without one, and every tab is shot at least once.
  const tabs = new Set<Tab>();
  const misplaced: string[] = [];
  for (const scene of scenes) {
    const phase = readJson<{ phase: { kind: string } }>(fixturePath(scene.fixture, 'ko')).phase.kind;
    const main = phase === 'ready' || phase === 'running' || phase === 'faulted';
    if (main && scene.tab !== null) tabs.add(scene.tab);
    if (main !== (scene.tab !== null)) misplaced.push(`${scene.name} (${phase}, tab ${scene.tab})`);
  }
  expect(misplaced, 'scenes whose tab does not fit their phase').toEqual([]);
  expect([...tabs].sort()).toEqual(['profile', 'settings', 'status', 'upmix']);
});

/** The checks every picture must pass, before and after it is taken. */
async function check(page: Page, language: Language, skin: Skin, problems: string[]) {
  expect(problems, 'console errors, warnings and page errors').toEqual([]);
  for (const font of FONTS[skin]) {
    const loaded = await page.evaluate((face) => document.fonts.check(face, '공간'), font);
    expect(loaded, `${font} is loaded`).toBe(true);
  }
  const facts = await page.evaluate(() => ({
    lang: document.documentElement.lang,
    scrollWidth: document.documentElement.scrollWidth,
    clientWidth: document.documentElement.clientWidth,
    text: document.body.innerText,
  }));
  expect(facts.lang, '<html lang>').toBe(language);
  expect(facts.scrollWidth, 'no horizontal overflow').toBeLessThanOrEqual(facts.clientWidth);
  if (language === 'en') {
    const hangul = facts.text.split('\n').filter((line) => HANGUL.test(line));
    expect(hangul, 'Hangul on an English page').toEqual([]);
  } else {
    expect(HANGUL.test(facts.text), 'Hangul on a Korean page').toBe(true);
  }
}

for (const skin of table.skins) {
  for (const scene of scenes) {
    for (const language of table.languages) {
      for (const scheme of scene.schemes) {
        const file = `${scene.name}.${language}.${scheme}.png`;
        test.describe(`${skin}/${file}`, () => {
          const [width, height] = scene.size;
          test.use({ locale: LOCALES[language], viewport: { width, height }, colorScheme: scheme, deviceScaleFactor: 1 });

          test('draws and is photographed', async ({ page }, info) => {
            const problems: string[] = [];
            page.on('console', (message) => {
              if (message.type() === 'error' || message.type() === 'warning') {
                problems.push(`console ${message.type()}: ${message.text()}`);
              }
            });
            page.on('pageerror', (error) => problems.push(`page error: ${error.message}`));
            // A scene that cannot be drawn throws; fail on that at once.
            const broken = new Promise<never>((_, reject) => {
              page.on('pageerror', (error) => reject(error));
            });
            await page.emulateMedia({ colorScheme: scheme });
            await page.goto(`/?gallery=1&scene=${encodeURIComponent(scene.name)}&lang=${language}&skin=${skin}`);
            await Promise.race([page.locator(READY).waitFor({ state: 'attached' }), broken]);

            await check(page, language, skin, problems);
            const dir = join(String(info.config.metadata.shotsDir), skin);
            mkdirSync(dir, { recursive: true });
            // A turning spinner would make every picture of its scene differ;
            // 'disabled' cancels an infinite animation to its first frame for
            // the shot (and finishes finite ones) so it is the same each run.
            await page.screenshot({ path: join(dir, file), fullPage: true, animations: 'disabled' });
            await check(page, language, skin, problems);
          });
        });
      }
    }
  }
}
