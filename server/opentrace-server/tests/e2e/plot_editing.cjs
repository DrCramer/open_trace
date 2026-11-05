// Label editing in the Samples Plot window (Playwright, Chromium), on the built-in example project.
// Covers the ID-X ways to clean an electropherogram: select a peak by clicking it or its label, the peak popup
// (right-click: Delete Label(s), Rename Allele Label >, Peak Raw Data), the Delete key, Ctrl+Z, the "Edit labels"
// bar, single-click editing (a click on a label deletes it) and Shift+drag range selection.
//   OT_URL=http://127.0.0.1:8765 [OT_SHOTS=dir] node plot_editing.cjs   (needs the npm package playwright + Chromium)
// Works against `opentrace serve` and against OpenTrace Server (log in first when the server asks for it:
// OT_USER / OT_PASSWORD).
const { chromium } = require('playwright');
(async () => {
  const base = process.env.OT_URL || 'http://127.0.0.1:8765';
  const shots = process.env.OT_SHOTS || '';
  const browser = await chromium.launch();
  const ctx = await browser.newContext({ viewport: { width: 1500, height: 950 } });
  const page = await ctx.newPage();
  const errors = [];
  const watch = p => p.on('pageerror', e => errors.push(e.message));
  watch(page);
  await page.goto(base + '/idx/');
  await page.waitForFunction(() => window.OT && window.OT.state && window.Sec);
  if (process.env.OT_PASSWORD) {
    await page.evaluate(async ([n, pw]) => {
      const r = await fetch('/api/security', { method: 'POST', headers: { 'X-Session-Token': window.OT_TOKEN, 'Content-Type': 'application/json' }, body: JSON.stringify({ action: 'login', name: n, password: pw }) });
      const j = await r.json(); localStorage.setItem('opentrace.ticket', j.ticket || '');
    }, [process.env.OT_USER || 'Casework Analyst', process.env.OT_PASSWORD]);
    await page.reload();
    await page.waitForFunction(() => window.OT && window.OT.state && window.Sec && window.Sec.user);
  }
  await page.evaluate(() => window.OT.loadExample());
  const popupP = ctx.waitForEvent('page');
  await page.evaluate(() => {
    const s = window.OT.state.project.samples.find(x => x.type === 'sample');
    window.OT.state.selected.clear(); window.OT.state.selected.add(s.id); window.OT.displayPlots();
  });
  const plot = await popupP;
  watch(plot);
  await plot.waitForFunction(() => [...document.querySelectorAll('#plotarea canvas')].some(c => c._geom && c._geom.labelBoxes.length));
  const out = {};
  // Reason for Change (only when the audit asks for it) and confirmations: answer them.
  const answer = async () => {
    for (let i = 0; i < 20; i++) {
      await plot.waitForTimeout(150);
      const d = await plot.evaluate(() => { const d = document.querySelector('.dlg'); if (!d) return null; const r = d.querySelector('#rfcReason'); if (r) { r.value = 'e2e'; } return d.textContent.slice(0, 120); });
      if (!d) return;
      await plot.evaluate(() => document.querySelector('.dlg .foot .btn').click());
    }
  };
  const calls = () => plot.evaluate(() => { const p = window.opener.OT.plotPayload(); return p.samples.reduce((n, s) => n + ((s.report && s.report.calls) || []).length, 0); });
  // Centre of the first allele label of the first pane, in page coordinates.
  const labelPoint = (i = 0) => plot.evaluate(i => {
    const cv = [...document.querySelectorAll('#plotarea canvas')].find(c => c._geom && c._geom.labelBoxes.some(b => b.call));
    const b = cv._geom.labelBoxes.filter(b => b.call)[i]; const r = cv.getBoundingClientRect();
    cv.scrollIntoView({ block: 'center' });
    const r2 = cv.getBoundingClientRect();
    return { x: r2.left + (b.x0 + b.x1) / 2, y: r2.top + (b.y0 + b.y1) / 2, label: String(b.call.label) };
  }, i);
  out.editbar = await plot.evaluate(() => !!document.querySelector('#editbar .eb'));
  out.calls_start = await calls();

  // 0. Sizing table (ID-X SamplePlotTableModel): every sized peak of the shown dyes, row <-> peak selection, Ctrl+V / Ctrl+G filters.
  await plot.evaluate(() => { const b = [...document.querySelectorAll('#toolbar2 .tb')].find(x => x.title === T('sizingTable')); b.click(); });
  out.sizing_rows = await plot.evaluate(() => document.querySelectorAll('#gtGrid table.sizing tbody tr').length);
  out.sized_peaks = await plot.evaluate(() => { const p = window.opener.OT.plotPayload(), v = window.__otView; let n = 0;
    for (const s of p.samples) for (const c of (s.analysis.channels || [])) if (v.dyes.has(c.tag)) n += (c.peaks || []).filter(k => k.size_bp != null).length; return n; });
  out.sizing_call_cells = await plot.evaluate(() => [...document.querySelectorAll('#gtGrid table.sizing tbody tr')].filter(tr => tr.lastElementChild.textContent).length);
  await plot.evaluate(() => { const tr = [...document.querySelectorAll('#gtGrid table.sizing tbody tr')].find(tr => tr.lastElementChild.textContent);
    tr.dispatchEvent(new MouseEvent('mousedown', { bubbles: true })); });
  out.row_selects_peak = await plot.evaluate(() => window.__otView.selPeaks.size);
  out.row_marked = await plot.evaluate(() => document.querySelectorAll('#gtGrid table.sizing tbody tr.sel').length);
  await plot.keyboard.press('Control+g');
  out.rows_selected_filter = await plot.evaluate(() => document.querySelectorAll('#gtGrid table.sizing tbody tr').length);
  await plot.keyboard.press('Control+v');
  out.rows_call_filter = await plot.evaluate(() => document.querySelectorAll('#gtGrid table.sizing tbody tr').length);
  await plot.keyboard.press('Control+f');
  out.rows_all_again = await plot.evaluate(() => document.querySelectorAll('#gtGrid table.sizing tbody tr').length);
  if (shots) await plot.screenshot({ path: shots + '/plot_sizing_table.png' });
  await plot.evaluate(() => { const b = [...document.querySelectorAll('#toolbar2 .tb')].find(x => x.title === T('genotypesTable')); b.click(); window.__otView.selPeaks.clear(); });
  await plot.waitForFunction(() => [...document.querySelectorAll('#plotarea canvas')].some(c => c._geom && c._geom.labelBoxes.length));

  // 1. Click on a label selects its peak; the bar enables Delete.
  let p = await labelPoint(0);
  await plot.mouse.click(p.x, p.y);
  out.selected_after_label_click = await plot.evaluate(() => window.__otView.selPeaks.size);
  out.delete_enabled = await plot.evaluate(() => !document.getElementById('eb_delete').disabled);
  out.hint = await plot.evaluate(() => document.getElementById('ebHint').textContent);
  // arrows after a click on a label: next / previous allele label of the pane
  const selKey = () => plot.evaluate(() => [...window.__otView.selPeaks.keys()].join());
  const k0 = await selKey();
  await plot.keyboard.press('ArrowRight');
  const k1 = await selKey();
  await plot.keyboard.press('ArrowLeft');
  const k2 = await selKey(); out.arrows_dbg = [k0, k1, k2];
  out.arrows_label_mode = k1 !== k0 && k2 === k0;

  // 2. Right-click: the ID-X peak popup.
  await plot.mouse.click(p.x, p.y, { button: 'right' });
  await plot.waitForTimeout(200);
  out.popup = await plot.evaluate(() => [...document.querySelectorAll('.menu.ctx.show > .mi')].map(m => m.childNodes[0].textContent + (m.classList.contains('dis') ? ' (off)' : '')));
  await plot.hover('.menu.ctx.show .mi.sub');
  await plot.waitForTimeout(200);
  out.rename_submenu = await plot.evaluate(() => [...document.querySelectorAll('.menu.ctx.show.sub-open')].flatMap(m => [...m.querySelectorAll('.mi')].map(x => x.textContent)));
  if (shots) await plot.screenshot({ path: shots + '/plot_popup.png' });
  await plot.keyboard.press('Escape');
  await plot.click('#xy'); // closes the menu

  // 3. Delete key removes the label, Ctrl+Z brings it back.
  await plot.mouse.click(p.x, p.y);
  if (!(await plot.evaluate(() => window.__otView.selPeaks.size))) await plot.mouse.click(p.x, p.y);
  await plot.keyboard.press('Delete');
  await answer();
  out.calls_after_delete = await calls();
  await plot.keyboard.press('Control+z');
  await answer();
  out.calls_after_undo = await calls();

  // 4. Single-click editing: a click on a label deletes it.
  await plot.click('#eb_clean');
  out.clean_on = await plot.evaluate(() => document.body.classList.contains('cleanmode'));
  p = await labelPoint(0);
  await plot.mouse.click(p.x, p.y);
  await answer();
  out.calls_after_click_delete = await calls();
  if (shots) await plot.screenshot({ path: shots + '/plot_clean_mode.png' });
  await plot.click('#eb_clean');
  await plot.click('#eb_undo');
  await answer();
  out.calls_after_second_undo = await calls();
  out.clean_off = await plot.evaluate(() => !document.body.classList.contains('cleanmode'));

  // 5. Shift+drag over the whole first pane selects every peak; Delete removes all their labels with one reason.
  const pane = await plot.evaluate(() => { const cv = [...document.querySelectorAll('#plotarea canvas')].find(c => c._geom && c._geom.labelBoxes.some(b => b.call)); const r = cv.getBoundingClientRect(), G = cv._geom;
    return { x0: r.left + G.L + 2, x1: r.left + G.W - G.R - 2, y: r.top + (G.top + G.bottom) / 2, labels: G.labelBoxes.filter(b => b.call).length }; });
  await plot.click('#xy');
  await plot.keyboard.down('Shift'); await plot.mouse.move(pane.x0, pane.y); await plot.mouse.down(); await plot.mouse.move(pane.x1, pane.y, { steps: 8 }); await plot.mouse.up(); await plot.keyboard.up('Shift');
  out.range_selected = await plot.evaluate(() => window.__otView.selPeaks.size);
  const before = await calls();
  await plot.keyboard.press('Delete');
  await answer();
  out.range_deleted = before - (await calls());
  out.pane_labels = pane.labels;
  out.edits_in_project = await plot.evaluate(() => window.opener.OT.state.project.samples.reduce((n, s) => n + (s.edits || []).filter(e => !e.undone).length, 0));
  if (shots) await plot.screenshot({ path: shots + '/plot_after_range_delete.png' });
  // 5a. Multiple Dye Plot (ID-X GenotypeMultipleDyePane): combined, then one pane per dye
  await plot.evaluate(() => { window.__otView.selMarker = window.opener.OT.plotPayload().kit.markers[0].name; window.__otView.selSample = window.opener.OT.plotPayload().samples[0].id; });
  await plot.click('#tbMultiDye');
  await plot.waitForSelector('.mdpdlg canvas');
  const mdp1 = await plot.evaluate(() => document.querySelectorAll('.mdpdlg canvas').length);
  await plot.click('#mdp_separate');
  await plot.waitForTimeout(200);
  out.multiple_dye = [mdp1, await plot.evaluate(() => document.querySelectorAll('.mdpdlg canvas').length)];
  await plot.evaluate(() => document.querySelector('.mdpdlg').closest('.dlg').querySelector('.foot .btn').click());
  // 6. ID-X plot window keys: Ctrl+N / Ctrl+Y X axis, Ctrl+Shift+M / D Y axis, Ctrl+Shift+N / H labels, Ctrl+A / W / Q tables
  const v = () => plot.evaluate(() => { const v = window.__otView; return [v.xAxis, v.yAxis, v.labels, v.table].join(','); });
  const keys = {};
  for (const k of ['Control+n', 'Control+y', 'Control+Shift+m', 'Control+Shift+d', 'Control+Shift+n', 'Control+Shift+h', 'Control+a', 'Control+q', 'Control+w']) {
    await plot.keyboard.press(k); keys[k] = await v();
  }
  out.keys = keys;
  out.keys_ok = keys['Control+n'].startsWith('dp,') && keys['Control+y'].startsWith('bp,') && keys['Control+Shift+m'].split(',')[1] === 'max' &&
    keys['Control+Shift+d'].split(',')[1] === 'individual' && keys['Control+Shift+n'].split(',')[2] === 'none' && keys['Control+Shift+h'].split(',')[2] === 'h' &&
    keys['Control+a'].endsWith(',sizing') && keys['Control+q'].endsWith(',none') && keys['Control+w'].endsWith(',genotypes');
  out.page_errors = errors;
  console.log(JSON.stringify(out, null, 1));
  const ok = out.multiple_dye[0] === 1 && out.multiple_dye[1] > 1 && out.keys_ok && out.arrows_label_mode && out.sizing_rows === out.sized_peaks && out.sizing_rows > out.calls_start && out.sizing_call_cells === out.calls_start &&
    out.row_selects_peak === 1 && out.row_marked === 1 && out.rows_selected_filter === 1 && out.rows_call_filter === out.calls_start && out.rows_all_again === out.sizing_rows &&
    out.editbar && out.selected_after_label_click === 1 && out.delete_enabled && out.popup.length >= 3 &&
    /Удалить обозначения|Delete Label/.test(out.popup[0]) && out.rename_submenu.length >= 2 &&
    out.calls_after_delete === out.calls_start - 1 && out.calls_after_undo === out.calls_start && out.clean_on && out.clean_off &&
    out.calls_after_click_delete === out.calls_start - 1 && out.range_selected > 1 && out.calls_after_second_undo === out.calls_start && out.range_deleted === out.pane_labels && out.range_deleted > 1 && !errors.length;
  await browser.close();
  process.exit(ok ? 0 : 1);
})().catch(e => { console.error(e); process.exit(2); });
