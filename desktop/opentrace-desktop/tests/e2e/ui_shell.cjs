// Desktop interface shell (shell.js) in a real browser (Playwright, Chromium): one-row application bar, workspace tabs,
// command registry, command palette, "Customize interface" editor (hide / reorder / apply), persistence and migration
// of the stored layout, critical elements, emergency reset, presets, themes and the independent plot background
// (DATA3 not black on a dark field), plot / panel manager / report manager windows, genotypes unchanged by UI changes.
//   OT_URL=http://127.0.0.1:8765 [OT_SHOTS=dir] [OT_FSA_DIR=dir] node ui_shell.cjs
// Exit code 1 when a check fails. Screenshots go to OT_SHOTS (default: no screenshots).
const { chromium } = require('playwright');
const fs = require('fs'), path = require('path');
const dir = process.env.OT_FSA_DIR || '', out = process.env.OT_SHOTS || '', base = process.env.OT_URL || 'http://127.0.0.1:8765';
const R = p => fs.readFileSync(p, 'utf8');
const results = []; const shot = async (p, name) => { if (out) await p.screenshot({ path: path.join(out, String(name).replace(/^\//, '')) }); }; const ok = (name, cond, info) => { results.push({ name, ok: !!cond, info: info === undefined ? '' : info }); console.log((cond ? 'PASS ' : 'FAIL ') + name + (info !== undefined ? ' ' + JSON.stringify(info) : '')); };

// Optional local data set (OT_FSA_DIR): kits/<PanGlobal plus kit>/... and 16-10-2026_PGplus/*.fsa (ladders, controls, six samples)
const pgplusSetup = async ({ panels, bins, stutter, am, std, fsas }) => {
    const parse = async body => { const r = await fetch('/api/import-gm-kit', { method: 'POST', headers: { 'X-Session-Token': window.OT_TOKEN, 'X-User-Ticket': Sec.ticket(), 'Content-Type': 'application/json' }, body: JSON.stringify(body) }); const j = await r.json(); if (!r.ok) throw Error(JSON.stringify(j)); return j; };
    const db = KitStore.load(); KitStore.addPanels(db, (await parse({ panels })).panels);
    KitStore.addBinSet(db, 'PanGlobal plus-1.1-HW', (await parse({ bins })).bins); KitStore.setStutter(db, 'PanGlobal plus-1.1-HW', (await parse({ stutter })).stutter); KitStore.save(db);
    const m = await Methods.post('/api/analysis-method', { action: 'import_xml', xml: am }); Methods.put(m.method);
    const s = SizeStd.fromXml(std); SizeStd.put(s);
    const files = fsas.map(h => new File([Uint8Array.from(atob(h.b64), c => c.charCodeAt(0))], h.name));
    const OT = window.OT; Object.assign(OT.state.project, { name: 'UI smoke', runFolders: [], samples: [], dirty: false });
    if (window.ProjectOptions) ProjectOptions.get().ars = 'continue';
    OT.addSampleFiles(files, '16-10-2026_PGplus', m.method.name, 'PanGlobal plus-1.1-HW\u0001PanGlobal plus-1.1-HW', s.name);
    // informational dialogs during the analysis (not normalized samples ...) are confirmed with their default button
    window.__dlgs = []; const iv = setInterval(() => { const d = document.querySelector('.dlg'); const b = d && d.querySelector('.foot button'); if (b) { window.__dlgs.push(d.textContent.slice(0, 160) + ' => ' + b.textContent); b.click(); } }, 300);
    await new Promise(r => setTimeout(r, 300)); await OT.analyze('pending'); clearInterval(iv);
    const S = OT.state.project.samples; return { n: S.length, calls: S.reduce((a, x) => a + ((x.report && x.report.calls) || []).filter(c => c.label).length, 0),
      dlgs: window.__dlgs, st: S.map(x => x.status).join(','), digest: S.map(x => x.file + ':' + ((x.report && x.report.calls) || []).filter(c => c.label).map(c => c.marker + '=' + c.label).join(',')).join('\n') };
  };
function pgplusArgs(dir) {
  const R = p => fs.readFileSync(p, 'utf8');
  const K = path.join(dir, 'kits', '!_PanGlobal plus-1.1_HW(GA21053-GA21067)2024.01.12'), K2 = path.join(K, 'PanGlobal plus-1.1_HW(GA21053-GA21067)2024.01.12');
  const names = fs.readdirSync(path.join(dir, '16-10-2026_PGplus')).filter(f => f.endsWith('.fsa')).sort().filter(f => /^(333-[1-6]-|Ladder-|PC-|NC-)/.test(f));
  const fsas = names.map(f => ({ name: f, b64: fs.readFileSync(path.join(dir, '16-10-2026_PGplus', f)).toString('base64') }));
  return { panels: R(path.join(K2, 'PanGlobal plus-1.1_Panels-HW.txt')), bins: R(path.join(K2, 'PanGlobal plus-1.1_bins-HW.txt')), stutter: R(path.join(K2, 'PanGlobal Plus-1.1_stutter.txt')), am: R(path.join(K, 'PGPlusv1_1_BaseW101.xml')), std: R(path.join(K, 'ILS-500.xml')), fsas };
}
(async () => {
  const browser = await chromium.launch(); const ctx = await browser.newContext({ viewport: { width: 1600, height: 950 } });
  const errs = []; const page = await ctx.newPage(); page.on('pageerror', e => errs.push('main: ' + e.message));
  await page.goto(base + '/idx/'); await page.waitForFunction(() => window.OT && window.OT.state && window.Shell && window.KitStore && window.Methods && window.SizeStd);
  ok('web default chrome is classic', await page.evaluate(() => document.documentElement.dataset.chrome === 'classic' && getComputedStyle(document.getElementById('menubar')).display !== 'none'));
  await shot(page, '/00_classic_web_default.png');
  await page.evaluate(() => Shell.set('chrome', 'industrial')); await page.evaluate(() => Shell.set('theme', 'graphite'));
  // data: the built-in example project (synthetic, no laboratory data); OT_FSA_DIR may point to a local kit + FSA folder
  // (PanGlobal plus layout used during development; never part of the repository)
  const r = dir ? await page.evaluate(pgplusSetup, pgplusArgs(dir)) : await page.evaluate(async () => {
    await window.OT.loadExample(); const S = window.OT.state.project.samples;
    return { n: S.length, calls: S.reduce((a, x) => a + ((x.report && x.report.calls) || []).filter(c => c.label).length, 0),
      digest: S.map(x => x.file + ':' + ((x.report && x.report.calls) || []).filter(c => c.label).map(c => c.marker + '=' + c.label).join(',')).join('\n') }; });
  const nExpected = r.n;
  ok('project with genotypes loaded', r.n > 0 && r.calls > 0, { n: r.n, calls: r.calls });
  await page.waitForTimeout(500);
  // geometry: one application bar row, right group pinned right, tabs row, no gap under the status bar
  const geo = await page.evaluate(() => { const q = s => document.querySelector(s)?.getBoundingClientRect(); const a = q('#shAppbar'), rg = q('#shRight'), m = q('#menubar'), ws = q('#wsBar'), sb = q('.statusbar');
    return { appH: a && a.height, rightRight: rg && Math.round(rg.right), vw: innerWidth, menuTop: m && m.top, rightTop: rg && rg.top, wsTop: ws && ws.top, appBottom: a && a.bottom, sbBottom: sb && Math.round(sb.bottom), vh: innerHeight,
      tops: [...document.querySelectorAll('#menubar>.mtop')].map(t => t.textContent), tabs: [...document.querySelectorAll('#wsTabs .wstab')].map(t => t.textContent),
      right: [...document.querySelectorAll('#shRight>*')].map(e => e.className) } });
  ok('one application bar row (menus + right group)', geo.appH <= 32 && Math.abs(geo.menuTop - geo.rightTop) < 2, geo.appH);
  ok('right group pinned to the right edge', Math.abs(geo.rightRight - geo.vw) <= 1, [geo.rightRight, geo.vw]);
  ok('workspace tabs directly under the application bar', Math.abs(geo.wsTop - geo.appBottom) <= 1);
  ok('status bar without bottom gap', Math.abs(geo.sbBottom - geo.vh) <= 1, [geo.sbBottom, geo.vh]);
  ok('menu sections', geo.tops.join('|') === 'Файл|Правка|Анализ|Вид|Графики|Инструменты|Администрирование|Справка' || geo.tops.length >= 8, geo.tops);
  ok('workspace tabs', geo.tabs.join('|') === 'Проекты|Графики|Генотипы|Стандарты|Журнал', geo.tabs);
  await shot(page, '/01_main_graphite.png');
  for (const th of ['light', 'dark', 'contrast', 'system']) { await page.evaluate(t => Shell.set('theme', t), th); await page.waitForTimeout(200); await shot(page, `/02_main_${th}.png`); }
  await page.evaluate(() => Shell.set('theme', 'graphite'));
  // workspace tabs
  for (const [i, id] of [[1, 'plots'], [2, 'genotypes'], [3, 'standards'], [4, 'journal'], [0, 'projects']]) {
    await page.click(`#wsTabs .wstab:nth-child(${i + 1})`); await page.waitForTimeout(250);
    const st = await page.evaluate(() => ({ on: document.querySelector('#wsTabs .wstab.on')?.dataset.cmd, hub: [...document.querySelectorAll('[id^=page-hub-]')].filter(p => !p.hidden).map(p => p.id), btns: [...document.querySelectorAll('[id^=page-hub-]:not([hidden]) .hubbtn')].length }));
    ok('workspace tab ' + id, st.on === id, st); if (id !== 'projects' && id !== 'genotypes') await shot(page, `/03_hub_${id}.png`);
  }
  // registry
  const reg = await page.evaluate(() => { const c = Shell.commands(); return { n: c.length, menu: c.filter(x => x.area.startsWith('main.menu')).length, noRun: c.filter(x => x.kind === 'action' && !x.hasRun && !x.disabled && !x.critical && x.area.startsWith('main.menu')).map(x => x.id), dyn: c.filter(x => x.dynamic).map(x => x.id), areas: Shell.areas().length } });
  ok('registry: main window commands', reg.n > 100, reg); ok('registry: every main menu action has a handler', reg.noRun.length === 0, reg.noRun);
  // palette
  await page.keyboard.press('Control+Shift+P'); await page.waitForSelector('#shPalette'); await page.keyboard.type('панел'); await page.waitForTimeout(150);
  const pal = await page.evaluate(() => [...document.querySelectorAll('.sh-palrow')].map(r => r.textContent).slice(0, 8));
  ok('palette finds commands', pal.length > 0, pal); await shot(page, '/04_palette.png'); await page.keyboard.press('Escape');
  ok('palette closes on Esc', await page.evaluate(() => !document.getElementById('shPalette')));
  // palette runs a command (Find dialog)
  await page.keyboard.press('Control+Shift+P'); await page.keyboard.type('Найти'); await page.waitForTimeout(150); await page.keyboard.press('Enter'); await page.waitForTimeout(300);
  ok('palette runs a command', await page.evaluate(() => !!document.querySelector('.dlg')), await page.evaluate(() => document.querySelector('.dlg .title')?.textContent));
  await page.keyboard.press('Escape'); await page.waitForTimeout(150);
  // editor
  await page.evaluate(() => Shell.openEditor()); await page.waitForSelector('.sh-editor'); await page.waitForTimeout(200);
  await page.evaluate(() => { const n = [...document.querySelectorAll('.sh-ednode')].find(n => /Панель инструментов/.test(n.textContent)); n && n.click(); });
  await page.waitForTimeout(150); await shot(page, '/05_editor.png');
  // hide "help" and "find" toolbar buttons, move "analyze" one up, apply
  await page.evaluate(() => { const rows = [...document.querySelectorAll('.sh-edgrid tr')]; for (const r of rows) { const t = r.children[1]?.textContent || ''; if (/^Найти|^Справка|Help/.test(t)) { const cb = r.querySelector('input'); cb.checked = false; cb.dispatchEvent(new Event('change')) } } });
  await page.evaluate(() => { const r = [...document.querySelectorAll('.sh-edgrid tr')].find(r => /^Анализ/.test(r.children[1]?.textContent || '')); r && r.querySelector('button[title="Выше"]').click() });
  await page.evaluate(() => { const b = [...document.querySelectorAll('.dlg .foot button')].find(b => b.textContent === 'Применить'); b.click() }); await page.waitForTimeout(300);
  const tb1 = await page.evaluate(() => [...document.querySelectorAll('#toolbar>[data-cmd]')].filter(e => !e.hidden).map(e => e.dataset.cmd));
  ok('hidden toolbar buttons are not shown', !tb1.includes('tbFind') && !tb1.includes('help'), tb1);
  ok('reordered toolbar button moved', tb1.indexOf('tbAnalyze') >= 0 && tb1.indexOf('tbAnalyze') < tb1.indexOf('tbManager'), tb1);
  // plot window (before the reload tests: the project lives in this page)
  await page.evaluate(() => { const S = window.OT.state; S.selected = new Set(S.project.samples.slice(0, 3).map(s => s.id)); });
  const [pw] = await Promise.all([ctx.waitForEvent('page'), page.evaluate(() => window.OT.displayPlots())]);
  pw.on('pageerror', e => errs.push('plot: ' + e.message)); await pw.waitForLoadState('load'); await pw.waitForFunction(() => document.querySelectorAll('#plotarea canvas').length > 0, null, { timeout: 30000 }); await pw.waitForTimeout(1200);
  const px = async () => pw.evaluate(() => { const cv = document.querySelector('#plotarea canvas'); const g = cv.getContext('2d'); const d = g.getImageData(3, Math.floor(cv.height / 2), 1, 1).data; return [d[0], d[1], d[2]]; });
  const p1 = await px(); ok('plot: light field with the graphite theme (auto)', p1[0] > 240 && p1[1] > 240, p1);
  await shot(pw, '/06_plot_graphite_light_field.png');
  await page.evaluate(() => Shell.set('plot_surface', 'dark')); await pw.waitForTimeout(800);
  const p2 = await px(); ok('plot: dark field after the common setting changed in another window', p2[0] < 40, p2);
  await shot(pw, '/07_plot_dark_field.png');
  const blk = await pw.evaluate(() => { let black = 0; for (const cv of document.querySelectorAll('#plotarea canvas')) { const g = cv.getContext('2d'); const d = g.getImageData(0, 0, cv.width, cv.height).data; for (let i = cv.width * 4 * Math.round(24 * devicePixelRatio); i < d.length; i += 4) if (d[i] < 8 && d[i + 1] < 8 && d[i + 2] < 8 && d[i + 3] > 200) black++; } return black; });
  ok('plot: no pure-black drawing on the dark field', blk < 50, blk);
  await page.evaluate(() => Shell.set('theme', 'dark')); await page.evaluate(() => Shell.set('plot_surface', 'auto')); await pw.waitForTimeout(600);
  ok('plot: auto field with the dark theme is dark', (await px())[0] < 40); await shot(pw, '/08_plot_dark_theme.png');
  await page.evaluate(() => Shell.set('theme', 'graphite')); await pw.waitForTimeout(600);
  ok('plot: back to light field', (await px())[0] > 240);
  await pw.evaluate(() => { const v = [...document.querySelectorAll('#menubar>.mtop')].find(t => t.dataset.cmd === 'view'); v.dispatchEvent(new MouseEvent('mousedown', { bubbles: true })); v.click(); });
  await pw.waitForTimeout(200); await shot(pw, '/09_plot_view_menu.png'); await pw.keyboard.press('Escape');
  const preg = await pw.evaluate(() => { const c = Shell.commands(); return { n: c.length, dyes: c.filter(x => /plot\.toolbar1\/dye\./.test(x.id)).length, tb2: c.filter(x => x.area === 'plot.toolbar2').length, eb: c.filter(x => x.area === 'plot.editbar').length, pal: c.some(x => x.id === 'plot.menu.view/shell.palette') } });
  ok('plot registry: dyes, toolbars, edit bar, shell items', preg.dyes >= 5 && preg.tb2 >= 10 && preg.eb >= 4 && preg.pal, preg);
  await pw.evaluate(() => { const L = Shell.layout; window.__keepHidden = JSON.parse(JSON.stringify(L.hidden)); L.hidden['plot.toolbar2'] = ['bins']; Shell.set('hidden', L.hidden); });
  await pw.waitForTimeout(300);
  ok('plot toolbar button hidden', await pw.evaluate(() => document.querySelector('#toolbar2 [data-cmd="bins"]').hidden));
  await pw.evaluate(() => Shell.set('hidden', window.__keepHidden)); await pw.waitForTimeout(200);
  await page.evaluate(() => Shell.applyPreset('analysis')); await pw.waitForTimeout(800);
  ok('preset Analysis reaches the plot window (panes)', await pw.evaluate(() => document.querySelectorAll('#plotarea .pane').length >= 3), await pw.evaluate(() => document.querySelectorAll('#plotarea .pane').length));
  await shot(pw, '/10_plot_preset_analysis.png');
  await page.evaluate(() => Shell.applyPreset('standard')); await pw.waitForTimeout(500);
  for (const th of ['light', 'contrast']) { await page.evaluate(t => Shell.set('theme', t), th); await pw.waitForTimeout(500); await shot(pw, `/11_plot_${th}.png`); }
  await page.evaluate(() => Shell.set('theme', 'graphite'));
  const dig2 = await page.evaluate(() => window.OT.state.project.samples.map(x => x.file + ':' + ((x.report && x.report.calls) || []).filter(c => c.label).map(c => c.marker + '=' + c.label).join(',')).join('\n'));
  ok('theme/layout changes do not change the genotypes', dig2 === r.digest);
  await pw.close();
  for (const [name, p] of [['panels', '/idx/panels.html'], ['report', '/idx/report.html']]) {
    const [w] = await Promise.all([ctx.waitForEvent('page'), page.evaluate(p => window.open(p, '_blank'), p)]);
    w.on('pageerror', e => errs.push(name + ': ' + e.message)); await w.waitForLoadState('load'); await w.waitForTimeout(1500);
    const st = await w.evaluate(() => ({ chrome: document.documentElement.dataset.chrome, app: !!document.getElementById('shAppbar'), n: Shell.commands().length, tops: [...document.querySelectorAll('#menubar>.mtop')].length }));
    ok(name + ' window: shell mounted', st.chrome === 'industrial' && st.app && st.n > 10 && st.tops > 3, st);
    await shot(w, `/12_${name}.png`); await w.close();
  }
  // hide a menu item and a whole section
  await page.evaluate(() => { const L = Shell.layout; L.hidden['main.menu.tools'] = ['mixture']; L.hidden['main.toolbar'] = ['tbFind', 'help']; L.hidden['main.menu'] = ['admin']; localStorage.setItem('opentrace.uiLayout.v1', JSON.stringify(L)); });
  // persistence: reload
  await page.reload(); await page.waitForFunction(() => window.OT && window.Shell); await page.waitForTimeout(500);
  const after = await page.evaluate(() => ({ chrome: document.documentElement.dataset.chrome, tb: [...document.querySelectorAll('#toolbar>[data-cmd]')].filter(e => !e.hidden).map(e => e.dataset.cmd), tops: [...document.querySelectorAll('#menubar>.mtop')].map(t => t.dataset.cmd) }));
  ok('layout persisted after reload', after.chrome === 'industrial' && !after.tb.includes('tbFind'), after.tb.length);
  ok('hidden menu section not shown', !after.tops.includes('admin'), after.tops);
  // hidden command still reachable through the palette
  const viaPal = await page.evaluate(() => Shell.commands().some(c => c.id === 'main.menu.admin/securityManager' || c.id.startsWith('main.menu.admin')));
  ok('hidden section commands stay in the registry', viaPal);
  // hidden item: Tools menu
  await page.click('#menubar>.mtop[data-cmd="tools"]'); await page.waitForTimeout(150);
  const tools = await page.evaluate(() => [...document.querySelectorAll('.menu.show .mi')].map(m => m.textContent));
  ok('hidden menu item not in the menu', !tools.some(t => /^Анализ смесей|^Mixture/.test(t)), tools.slice(0, 6)); await page.keyboard.press('Escape');
  // critical elements cannot be hidden
  await page.evaluate(() => { const L = Shell.layout; L.hidden['main.status'] = ['statusText', 'stopBtn']; L.hidden['main.right'] = ['status', 'quick']; localStorage.setItem('opentrace.uiLayout.v1', JSON.stringify(L)); });
  await page.reload(); await page.waitForFunction(() => window.OT && window.Shell); await page.waitForTimeout(400);
  const crit = await page.evaluate(() => ({ st: !document.getElementById('statusText').hidden, status: !!document.querySelector('#shRight .sh-status'), quick: !!document.querySelector('#shRight .sh-ico-btn') }));
  ok('critical elements stay visible', crit.st && crit.status && crit.quick, crit);
  // corrupt layout -> factory + message
  await page.evaluate(() => localStorage.setItem('opentrace.uiLayout.v1', '{"schema_version":99}')); await page.reload(); await page.waitForFunction(() => window.OT && window.Shell); await page.waitForTimeout(700);
  ok('corrupt layout falls back to factory with a message', await page.evaluate(() => !!document.querySelector('.dlg') && Shell.layout.chrome === 'classic'), await page.evaluate(() => document.querySelector('.dlg')?.textContent.slice(0, 120)));
  await page.keyboard.press('Escape');
  // emergency reset key
  await page.evaluate(() => { Shell.set('chrome', 'industrial'); Shell.set('theme', 'dark'); });
  await page.keyboard.press('Control+Shift+F12'); await page.waitForTimeout(200);
  await page.evaluate(() => { const b = [...document.querySelectorAll('.dlg .foot button')].find(b => /Да|Yes/.test(b.textContent)); b && b.click() }); await page.waitForTimeout(200);
  ok('Ctrl+Shift+F12 restores the factory layout', await page.evaluate(() => Shell.layout.theme === Shell.FACTORY.theme && Shell.layout.chrome === Shell.FACTORY.chrome && !Object.keys(Shell.layout.hidden).length));
  // presets
  await page.evaluate(() => { Shell.set('chrome', 'industrial'); Shell.set('theme', 'graphite'); Shell.applyPreset('qc'); }); await page.waitForTimeout(300);
  ok('preset QC: low quality to top', await page.evaluate(() => window.OT.state.lowQualityTop === true));
  await page.evaluate(() => Shell.applyPreset('standard'));
  // re-analysis not triggered by UI changes: compare digest with the digest before (the project was reloaded from memory? no - reload cleared it)
  ok('no page errors', errs.length === 0, errs);
  await browser.close();
  const failed = results.filter(x => !x.ok);
  console.log(failed.length ? `${failed.length} of ${results.length} checks FAILED` : `all ${results.length} checks passed`);
  process.exit(failed.length ? 1 : 0);
})().catch(e => { console.error(e); process.exit(2); });
