// Two browser users on one project through the web UI of OpenTrace Server (Playwright, Chromium).
// One editor per project: the user who creates/opens it edits it; the other opens it read-only (banner), cannot save,
// can Save As; the editor's heartbeat keeps the lock beyond the lease time; after the editor closes the project the
// other user re-opens it as editor and saves. Run the server with OPENTRACE_SERVER_LEASE_TTL_SECONDS=10.
//   OT_URL=http://127.0.0.1:8770 OT_PASSWORD=... node two_web_users.cjs   (needs the npm package playwright + Chromium)
// Users "Casework Analyst" and "Databasing Analyst" need that password (opentrace-server set-password).
const { chromium } = require('playwright');
(async () => {
  const base = process.env.OT_URL || 'http://127.0.0.1:8770';
  const browser = await chromium.launch();
  const errors = [];
  const pw = process.env.OT_PASSWORD;
  if (!pw) throw new Error('set OT_PASSWORD');
  async function user(name) {
    const ctx = await browser.newContext();
    const page = await ctx.newPage();
    page.on('pageerror', e => errors.push(name + ': ' + e.message));
    await page.goto(base + '/idx/');
    await page.waitForFunction(() => window.OT && window.OT.state && window.Sec);
    const t = await page.evaluate(async ([n, pw]) => {
      const r = await fetch('/api/security', { method: 'POST', headers: { 'X-Session-Token': window.OT_TOKEN, 'Content-Type': 'application/json' }, body: JSON.stringify({ action: 'login', name: n, password: pw }) });
      const j = await r.json(); localStorage.setItem('opentrace.ticket', j.ticket || ''); return !!j.ticket;
    }, [name, pw]);
    await page.reload();
    await page.waitForFunction(() => window.OT && window.OT.state && window.Sec && window.Sec.user);
    return { page, t };
  }
  const A = await user('Casework Analyst'), B = await user('Databasing Analyst');
  const out = { logged_in: [A.t, B.t] };
  const lastMsg = p => p.evaluate(() => [...document.querySelectorAll('.dlg')].map(d => d.textContent).join(' | '));
  const closeDlgs = p => p.evaluate(() => document.querySelectorAll('.dlg .foot .btn').forEach(b => b.click()));
  await A.page.evaluate(() => window.OT.loadExample());
  out.a_create = await A.page.evaluate(() => window.OT.writeProject('Web P', { create: true }));
  out.a_again = await A.page.evaluate(() => window.OT.writeProject('Web P'));
  await B.page.evaluate(() => window.OT.loadProject('Web P'));
  out.b_samples = await B.page.evaluate(() => window.OT.state.project.samples.length);
  await A.page.evaluate(() => { window.OT.state.project.comment = 'from A'; return window.OT.writeProject('Web P'); }).then(v => out.a_after_b_open = v);
  out.b_save = await B.page.evaluate(() => window.OT.writeProject('Web P'));
  out.b_message = (await lastMsg(B.page)).slice(0, 300);
  await closeDlgs(B.page);
  out.b_banner = await B.page.evaluate(() => (document.getElementById('otWebReadOnly') || {}).textContent || null);
  out.b_save_as = await B.page.evaluate(() => window.OT.writeProject('Web P (B)', { create: true }));
  await closeDlgs(B.page);
  // A keeps editing longer than the lease time (10 s): the heartbeat keeps the lock.
  await A.page.waitForTimeout(16000);
  out.a_after_ttl = await A.page.evaluate(() => { window.OT.state.project.comment = 'A later'; return window.OT.writeProject('Web P'); });
  await B.page.evaluate(() => window.OT.loadProject('Web P'));
  out.b_still_read_only = !!(await B.page.evaluate(() => document.getElementById('otWebReadOnly')));
  await closeDlgs(B.page);
  // A closes the project: lock released; B re-opens as editor, sees A's work, saves.
  await A.page.evaluate(() => { window.OT.state.project.dirty = false; return window.OT.closeProject ? window.OT.closeProject() : null; });
  out.a_closed = await A.page.evaluate(() => !window.OT.state.project.name);
  await B.page.evaluate(() => window.OT.loadProject('Web P'));
  out.b_comment = await B.page.evaluate(() => window.OT.state.project.comment);
  out.b_banner_after = await B.page.evaluate(() => !!document.getElementById('otWebReadOnly'));
  out.b_reopen_save = await B.page.evaluate(() => window.OT.writeProject('Web P'));
  out.page_errors = errors;
  console.log(JSON.stringify(out, null, 1));
  const ok = out.a_create && out.a_again && out.a_after_b_open && out.b_save === false && /редактирует/.test(out.b_message) &&
    /только для чтения/.test(out.b_banner || '') && out.b_save_as && out.a_after_ttl && out.b_still_read_only && out.a_closed &&
    out.b_comment === 'A later' && !out.b_banner_after && out.b_reopen_save && !errors.length;
  console.log(ok ? 'PASS' : 'FAIL');
  process.exitCode = ok ? 0 : 1;
  await browser.close();
})().catch(e => { console.error(e); process.exit(1); });
