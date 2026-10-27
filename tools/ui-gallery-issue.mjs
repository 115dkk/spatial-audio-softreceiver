#!/usr/bin/env node
// Checks a rendered screen gallery against its scene table and writes the
// review issue's body (.github/workflows/ui-gallery.yml).
//
//   node tools/ui-gallery-issue.mjs --shots DIR --check
//   node tools/ui-gallery-issue.mjs --list-skins
//   node tools/ui-gallery-issue.mjs --shots DIR --skin SKIN --base URL --tree URL \
//        --run URL --run-number N --commit SHA --source REF > body.md
//
// The scene table is apps/spatial-softreceiver/ui/e2e/gallery.json; the
// gallery spec takes <skin>/<name>.<language>.<scheme>.png for every skin
// and scene (ADR 0041). The check fails when a picture is missing, empty or
// not in the table, when a fixture in fixtures/index.json is in no scene, or
// when the table itself is inconsistent. A review issue holds one skin: three
// skins do not fit GitHub's body limit together. `--list-skins` prints each
// skin and its name, a tab apart, for the workflow to loop over. Node
// built-ins only, so `npm ci` is not needed to run it.

import { readdirSync, readFileSync, statSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const ui = join(root, 'apps', 'spatial-softreceiver', 'ui');
const TABS = new Set(['status', 'profile', 'upmix', 'settings']);
const SCHEMES = new Set(['light', 'dark']);
/** The app speaks exactly these; the gallery shows every screen in both. */
const LANGUAGES = ['ko', 'en'];
/** GitHub refuses an issue body over 65536 characters. */
const BODY_LIMIT = 65_000;

function fail(message) {
  console.error(`ui-gallery-issue: ${message}`);
  process.exit(1);
}

function args(argv) {
  const out = { check: false, listSkins: false };
  for (let i = 0; i < argv.length; i += 1) {
    const key = argv[i];
    if (key === '--check') {
      out.check = true;
      continue;
    }
    if (key === '--list-skins') {
      out.listSkins = true;
      continue;
    }
    if (!key.startsWith('--') || i + 1 >= argv.length) fail(`unexpected argument ${key}`);
    out[key.slice(2)] = argv[i + 1];
    i += 1;
  }
  if (out.shots === undefined && !out.listSkins) fail('--shots DIR is required');
  return out;
}

const readJson = (path) => JSON.parse(readFileSync(path, 'utf8'));

/** The table with defaults applied; throws on an inconsistent table. */
function scenes(table, fixtures) {
  const problems = [];
  const languages = [...table.languages].sort();
  if (languages.join() !== [...LANGUAGES].sort().join()) {
    problems.push(`languages must be exactly ${LANGUAGES.join(', ')}, not ${table.languages.join(', ')}`);
  }
  for (const language of LANGUAGES) {
    if (typeof table.languageNames?.[language] !== 'string') problems.push(`languageNames has no ${language}`);
  }
  for (const scheme of SCHEMES) {
    if (typeof table.schemeNames?.[scheme] !== 'string') problems.push(`schemeNames has no ${scheme}`);
  }
  const skins = Array.isArray(table.skins) ? table.skins : [];
  if (skins.length === 0) problems.push('skins lists no skin');
  if (new Set(skins).size !== skins.length) problems.push('skins lists a skin twice');
  for (const skin of skins) {
    if (!/^[a-z]+$/.test(skin)) problems.push(`skin ${skin} is not a plain folder name`);
    if (typeof table.skinNames?.[skin] !== 'string') problems.push(`skinNames has no ${skin}`);
  }
  const groups = new Set(table.groups.map((group) => group.id));
  const names = new Set();
  const used = new Set();
  const list = table.scenarios.map((scene) => {
    const full = {
      ...scene,
      size: scene.size ?? table.defaults.size,
      schemes: scene.schemes ?? table.defaults.schemes,
    };
    if (names.has(full.name)) problems.push(`scene ${full.name} is listed twice`);
    names.add(full.name);
    if (!/^[a-z0-9][a-z0-9x-]*$/.test(full.name)) problems.push(`scene name ${full.name} is not a plain file name`);
    if (!groups.has(full.group)) problems.push(`scene ${full.name} has no group ${full.group}`);
    if (!fixtures.includes(full.fixture)) problems.push(`scene ${full.name} uses unknown fixture ${full.fixture}`);
    if (full.tab !== null && !TABS.has(full.tab)) problems.push(`scene ${full.name} has unknown tab ${full.tab}`);
    if (full.schemes.length === 0) problems.push(`scene ${full.name} has no palette`);
    if (new Set(full.schemes).size !== full.schemes.length) problems.push(`scene ${full.name} lists a palette twice`);
    for (const scheme of full.schemes) if (!SCHEMES.has(scheme)) problems.push(`scene ${full.name}: scheme ${scheme}`);
    for (const skin of Object.keys(full.skinSteps ?? {})) {
      if (!skins.includes(skin)) problems.push(`scene ${full.name} has steps for unknown skin ${skin}`);
    }
    used.add(full.fixture);
    return full;
  });
  for (const fixture of fixtures) if (!used.has(fixture)) problems.push(`fixture ${fixture} is in no scene`);
  for (const group of table.groups) {
    if (!list.some((scene) => scene.group === group.id)) problems.push(`group ${group.id} has no scene`);
  }
  if (problems.length > 0) fail(`gallery.json:\n  ${problems.join('\n  ')}`);
  return list;
}

const file = (scene, language, scheme) => `${scene.name}.${language}.${scheme}.png`;

/** Text from outside (a branch name) shown as code: escaped, so it cannot close the element or mention anyone. */
const code = (value) =>
  `<code>${String(value).replace(/[&<>"'`@]/g, (character) => `&#${character.codePointAt(0)};`)}</code>`;

/** The PNGs under DIR as `<skin>/<file>` (and `<file>` for any loose at the top). */
function pictures(dir) {
  const found = [];
  let entries;
  try {
    entries = readdirSync(dir, { withFileTypes: true });
  } catch (error) {
    fail(`cannot read ${dir}: ${error.message}`);
  }
  for (const entry of entries) {
    if (entry.isFile() && entry.name.endsWith('.png')) found.push(entry.name);
    if (!entry.isDirectory()) continue;
    for (const name of readdirSync(join(dir, entry.name))) {
      if (name.endsWith('.png')) found.push(`${entry.name}/${name}`);
    }
  }
  return found;
}

/** Fails unless DIR holds exactly the table's pictures for every skin, none empty. */
function checkShots(dir, table, list) {
  const expected = new Set();
  for (const skin of table.skins) {
    for (const scene of list) {
      for (const language of table.languages) {
        for (const scheme of scene.schemes) expected.add(`${skin}/${file(scene, language, scheme)}`);
      }
    }
  }
  const present = pictures(dir);
  const problems = [];
  for (const name of expected) {
    if (!present.includes(name)) problems.push(`missing ${name}`);
    else if (statSync(join(dir, name)).size === 0) problems.push(`empty ${name}`);
  }
  for (const name of present) if (!expected.has(name)) problems.push(`not in gallery.json: ${name}`);
  if (problems.length > 0) fail(`${dir}:\n  ${problems.join('\n  ')}`);
  return expected.size;
}

function body(options, table, list) {
  const need = ['skin', 'base', 'tree', 'run', 'run-number', 'commit', 'source'];
  for (const key of need) if (options[key] === undefined) fail(`--${key} is required for the issue body`);
  const skin = options.skin;
  if (!table.skins.includes(skin)) fail(`--skin ${skin} is not in gallery.json`);
  const base = `${options.base.replace(/\/$/, '')}/${skin}`;
  const tree = `${options.tree.replace(/\/$/, '')}/${skin}`;
  const short = options.commit.slice(0, 7);
  const count = list.reduce((sum, scene) => sum + scene.schemes.length * table.languages.length, 0);
  const others = table.skins.filter((other) => other !== skin).map((other) => table.skinNames[other]);
  const lines = [
    `<!-- ui-gallery ${skin} -->`,
    `**화면 갤러리, ${table.skinNames[skin]} 스킨** · 실행 [#${options['run-number']}](${options.run}) · 커밋 ${code(short)} (${code(options.source)})`,
    '',
    `앱의 모든 화면 상태를 ${table.skinNames[skin]} 스킨으로, 한국어와 영어, 밝은 팔레트와 어두운 팔레트에서 찍었습니다. 앱을 띄우지 않고, 실제 화면 코드를 Rust가 만든 시험 자료와 모의 감독자 위에 올려 찍은 것입니다. 장면을 펼쳐 보고, 사진을 누르면 원래 크기로 열립니다. 다른 스킨(${others.join(', ')})은 제목 끝에 그 스킨 이름이 붙은 이슈에 있습니다.`,
    '',
    `장면 ${list.length}개, 사진 ${count}장 · [원본 PNG](${tree}) · 실행의 \`ui-gallery\` 아티팩트에도 같은 사진이 있습니다.`,
    '',
    '장면 표는 `apps/spatial-softreceiver/ui/e2e/gallery.json`입니다. 새 화면 상태가 생기면 그 표에 장면을 더하고, Rust 쪽 상태라면 `crates/ssr-types/tests/ui_contract.rs`에 시험 자료를 더합니다.',
  ];
  for (const group of table.groups) {
    const scenesInGroup = list.filter((scene) => scene.group === group.id);
    lines.push('', `### ${group.title}`, '');
    for (const scene of scenesInGroup) {
      const [width, height] = scene.size;
      const where = scene.tab === null ? scene.fixture : `${scene.fixture}, ${scene.tab}`;
      lines.push(
        `<details><summary><b>${scene.title}</b> · <code>${scene.name}</code> · ${width}×${height} · ${where}</summary>`,
        '',
        `| | ${table.languages.map((language) => table.languageNames[language]).join(' | ')} |`,
        `|:-:|${table.languages.map(() => ':-:').join('|')}|`,
      );
      for (const scheme of scene.schemes) {
        const cells = table.languages.map((language) => {
          const name = file(scene, language, scheme);
          return `![${scene.name} ${language} ${scheme}](${base}/${name})`;
        });
        lines.push(`| ${table.schemeNames[scheme]} | ${cells.join(' | ')} |`);
      }
      lines.push('', '</details>');
    }
  }
  const text = `${lines.join('\n')}\n`;
  if (text.length > BODY_LIMIT) fail(`the body is ${text.length} characters; GitHub takes at most 65536`);
  return text;
}

const options = args(process.argv.slice(2));
const table = readJson(join(ui, 'e2e', 'gallery.json'));
const fixtures = readJson(join(ui, 'fixtures', 'index.json'));
const list = scenes(table, fixtures);
if (options.listSkins) {
  for (const skin of table.skins) console.log(`${skin}\t${table.skinNames[skin]}`);
} else {
  const count = checkShots(options.shots, table, list);
  if (options.check) {
    console.log(`ui-gallery-issue: ${table.skins.length} skins, ${list.length} scenes, ${count} pictures, all present`);
  } else {
    process.stdout.write(body(options, table, list));
  }
}
