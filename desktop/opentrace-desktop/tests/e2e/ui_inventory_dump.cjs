// Dumps the command registry of the shell (Shell.commands() / Shell.areas()) of the four OpenTrace windows on the
// built-in example project, for docs/desktop/UI_COMMAND_INVENTORY.md (see ui_inventory_md.py).
//   OT_URL=http://127.0.0.1:8765 node ui_inventory_dump.cjs registry.json
const { chromium } = require('playwright');
const fs = require('fs');
(async () => {
  const base = process.env.OT_URL || 'http://127.0.0.1:8765', out = process.argv[2] || 'registry.json';
  const browser = await chromium.launch(); const ctx = await browser.newContext({ viewport: { width: 1600, height: 950 } });
  const errs = []; const page = await ctx.newPage(); page.on('pageerror', e => errs.push('main: ' + e.message));
  await page.goto(base + '/idx/'); await page.waitForFunction(() => window.OT && window.OT.state && window.Shell);
  await page.evaluate(() => Shell.set('chrome', 'industrial'));
  await page.evaluate(() => window.OT.loadExample()); await page.waitForTimeout(500);
  const res = {}; const dump = async (w, name) => { res[name] = await w.evaluate(() => ({ commands: Shell.commands(), areas: Shell.areas() })); };
  await dump(page, 'main');
  await page.evaluate(() => { const S = window.OT.state; S.selected = new Set(S.project.samples.slice(0, 2).map(s => s.id)); });
  const [pw] = await Promise.all([ctx.waitForEvent('page'), page.evaluate(() => window.OT.displayPlots())]);
  pw.on('pageerror', e => errs.push('plot: ' + e.message)); await pw.waitForLoadState('load');
  await pw.waitForFunction(() => document.querySelectorAll('#plotarea canvas').length > 0, null, { timeout: 30000 }); await pw.waitForTimeout(1000);
  await dump(pw, 'plot');
  for (const [name, p] of [['panels', '/idx/panels.html'], ['report', '/idx/report.html']]) {
    const [w] = await Promise.all([ctx.waitForEvent('page'), page.evaluate(p => window.open(p, '_blank'), p)]);
    w.on('pageerror', e => errs.push(name + ': ' + e.message)); await w.waitForLoadState('load'); await w.waitForTimeout(1200); await dump(w, name);
  }
  fs.writeFileSync(out, JSON.stringify({ errs, res }, null, 1));
  for (const [k, v] of Object.entries(res)) console.log(k, v.commands.length, 'IDs in', v.areas.length, 'areas');
  console.log('errors', errs); await browser.close(); process.exit(errs.length ? 1 : 0);
})().catch(e => { console.error(e); process.exit(2); });
