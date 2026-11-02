// Two browser users on one project through the unchanged web UI of OpenTrace Server (Playwright, Chromium).
// The second user's save of a project the first one changed meanwhile must be refused (409 revision_conflict);
// Save As and save after re-opening must work.
//   OT_URL=http://127.0.0.1:8770 OT_PASSWORD=... node two_web_users.cjs
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
  out.b_save_as = await B.page.evaluate(() => window.OT.writeProject('Web P (B)', { create: true }));
  await B.page.evaluate(() => window.OT.loadProject('Web P'));
  out.b_reopen_save = await B.page.evaluate(() => window.OT.writeProject('Web P'));
  out.page_errors = errors;
  console.log(JSON.stringify(out, null, 1));
  const ok = out.a_create && out.a_again && out.a_after_b_open && out.b_save === false && /изменён на сервере/.test(out.b_message) &&
    out.b_save_as && out.b_reopen_save && !errors.length;
  console.log(ok ? 'PASS' : 'FAIL');
  process.exitCode = ok ? 0 : 1;
  await browser.close();
})().catch(e => { console.error(e); process.exit(1); });
