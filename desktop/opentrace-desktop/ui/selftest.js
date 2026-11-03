// Desktop self-test (injected only when OPENTRACE_DESKTOP_SELFTEST is set). Exercises the real UI code paths.
(function () {
  'use strict';
  if (!window.__OT_SELFTEST__ || window.__otSelftestStarted) return;
  var ours = location.protocol === 'opentrace:' || location.hostname === 'opentrace.localhost';
  if (!ours) return;
  window.__otSelftestStarted = true;
  var report = function (o) {
    return fetch('/desktop/api/selftest/report', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(o) });
  };
  var sleep = function (ms) { return new Promise(function (r) { setTimeout(r, ms); }); };
  var wait = async function (cond, ms) {
    var t0 = Date.now();
    while (Date.now() - t0 < (ms || 30000)) {
      try { if (cond()) return true; } catch (e) {}
      await sleep(100);
    }
    return false;
  };
  var path = location.pathname;
  var heap = function () { return performance.memory ? Math.round(performance.memory.usedJSHeapSize / 1048576) : null; };
  var rss = async function () { try { return (await (await fetch('/desktop/api/selftest/memory')).json()).process_tree_rss_mib; } catch (e) { return null; } };

  // Batch mode (OPENTRACE_DESKTOP_SELFTEST_BATCH=N): N invented 10 000-scan run files through the UI's own
  // "Add Samples" + "Analyze" code (4 requests in flight, slimAnalysis, table render), then all of them in the
  // Samples Plot window. Measures wall time and, where the webview exposes it (Chromium/WebView2), the JS heap.
  // Genotyping through the UI (OPENTRACE_DESKTOP_SELFTEST_GENOTYPE=N): synthetic kit imported like the Panel Manager
  // does, an allelic ladder plus N samples, Add Samples + Analyze (sizing, ladder, genotyping), calls vs truth.
  var genotypeRun = async function (n) {
    var g = { samples: n };
    var OT = window.OT;
    var kt = await (await fetch('/desktop/api/selftest/kit')).json();
    var parse = async function (body) {
      var r = await fetch('/api/import-gm-kit', { method: 'POST', headers: { 'X-Session-Token': window.OT_TOKEN, 'X-User-Ticket': window.Sec ? Sec.ticket() : '', 'Content-Type': 'application/json' }, body: JSON.stringify(body) });
      var j = await r.json();
      if (!r.ok) throw new Error('import-gm-kit: ' + (j.error || r.status));
      return j;
    };
    var db = KitStore.load();
    KitStore.addPanels(db, (await parse({ panels: kt.panels })).panels);
    KitStore.addBinSet(db, kt.kit, (await parse({ bins: kt.bins })).bins);
    KitStore.setStutter(db, kt.kit, (await parse({ stutter: kt.stutter })).stutter);
    KitStore.save(db);
    var names = (window.Methods ? Methods.list() : []).map(function (r) { return r.method.name; });
    var meth = names.find(function (x) { return /MiniFiler_GS500/.test(x); }) || names[0];
    var files = [new File([await (await fetch('/desktop/api/selftest/fsa?ladder=1')).blob()], 'Synthetic allelic ladder.fsa')];
    for (var k = 0; k < n; k++) files.push(new File([await (await fetch('/desktop/api/selftest/fsa?genotype=' + k)).blob()], 'gt' + String(k).padStart(3, '0') + '.fsa'));
    Object.assign(OT.state.project, { name: 'Selftest genotype', runFolders: [], samples: [], dirty: false });
    OT.state.selected.clear();
    if (window.ProjectOptions) ProjectOptions.get().ars = 'continue';
    OT.addSampleFiles(files, 'Genotype run', meth, kt.kit + '\u0001' + kt.panel, 'CE_G5_HID_GS500');
    if (window.__OT_SELFTEST_SERVER__ && window.__OT_SELFTEST_PROFILE__) {
      // Before any kit is built: genotyping only counts for the profile with kits built while it is active.
      g.lab_profile_status = (await fetch('/desktop/api/lab-profile', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(window.__OT_SELFTEST_PROFILE__) })).status;
    }
    var t0 = performance.now();
    await OT.analyze('pending');
    g.analyze_genotype_ms = Math.round(performance.now() - t0);
    g.message = OT.state.lastMessage || null;
    g.genotype_errors = (OT.state.genotypeErrors || []).slice(0, 5);
    var smp = OT.state.project.samples;
    g.ladder_types = smp.filter(function (x) { return x.type === 'ladder'; }).length;
    g.samples_genotyped = 0;
    g.markers_ok = 0;
    g.mismatches = 0;
    g.first_mismatches = [];
    for (var s of smp) {
      if (s.type === 'ladder') continue;
      var seed = parseInt(s.file.replace(/\D/g, ''), 10);
      var exp = await (await fetch('/desktop/api/selftest/expected?seed=' + seed)).json();
      var calls = (s.report && s.report.calls) || [];
      if (calls.length) g.samples_genotyped++;
      Object.keys(exp).forEach(function (m) {
        var got = Array.from(new Set(calls.filter(function (c) { return c.marker === m && c.label; }).map(function (c) { return c.label; }))).sort();
        if (JSON.stringify(got) === JSON.stringify(exp[m])) g.markers_ok++;
        else { g.mismatches++; if (g.first_mismatches.length < 5) g.first_mismatches.push({ file: s.file, marker: m, expected: exp[m], called: got }); }
      });
    }
    if (window.__OT_SELFTEST_SERVER__ && window.__OT_SELFTEST_PROFILE__) {
      await fetch('/desktop/api/workspace/source', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ source: 'server' }) });
      var gname = 'Selftest genotype ' + Date.now();
      g.saved = await OT.writeProject(gname, { create: true });
      var ws = await (await fetch('/desktop/api/workspace/status')).json();
      // What the provenance page shows (same proxied request).
      var an = (await (await fetch('/desktop/api/v1/projects/' + encodeURIComponent(gname) + '/analyses')).json()).analyses || [];
      g.server_rows = an.length;
      g.server_rows_with_profile = an.filter(function (a) { return a.lab_profile === window.__OT_SELFTEST_PROFILE__.name; }).length;
      // The provenance page itself (opened like the status cell does) lists them while the project is still current.
      var pw = window.open('/desktop/provenance.html', 'opentrace-provenance', 'width=980,height=640');
      await wait(function () { return pw && pw.document && pw.document.querySelectorAll('#list tbody tr td.ok, #list tbody tr td span.ok').length >= g.server_rows_with_profile; }, 20000);
      try { g.page_rows = pw.document.querySelectorAll('#list tbody tr').length; g.page_profile_rows = pw.document.querySelectorAll('#list tbody tr span.ok').length; pw.close(); } catch (e) { g.page_rows = String(e); }
      g.provenance = ws.provenance;
      g.lab_profile = ws.lab_profile;
      await fetch('/desktop/api/workspace/source', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ source: 'local' }) });
    }
    return g;
  };

  var popupReopen = async function () {
    var out = { rounds: [] };
    for (var i = 0; i < 3; i++) {
      var w = window.open('/idx/panels.html', 'opentrace-selftest-panels', 'width=' + (820 + 40 * i) + ',height=560');
      var ok = await wait(function () { try { return w && w.document && w.document.readyState === 'complete' && w.document.body && w.document.body.children.length > 0; } catch (e) { return false; } }, 15000);
      var r = { loaded: !!ok };
      try { r.size = w.outerWidth + 'x' + w.outerHeight; } catch (e) { r.size = String(e); }
      try { w.close(); } catch (e) {}
      r.closed = !!(await wait(function () { return w.closed; }, 10000));
      // The app scheme must still answer (a hung UI thread stops every response).
      r.scheme_alive = (await fetch('/api/health')).ok;
      out.rounds.push(r);
      if (!r.loaded || !r.closed) break;
      await sleep(300);
    }
    // Closed by script: no native panels window may remain (WebView2 used to leave an empty white one).
    var left = [];
    for (var t = Date.now(); Date.now() - t < 10000; await sleep(250)) {
      left = ((await (await fetch('/desktop/api/selftest/windows')).json()).windows || []).filter(function (l) { return l.indexOf('panels') === 0; });
      if (!left.length) break;
    }
    out.native_panels_left = left;
    out.pass = out.rounds.length === 3 && out.rounds.every(function (r) { return r.loaded && r.closed && r.scheme_alive; }) && left.length === 0;
    return out;
  };

  var batchRun = async function (n) {
    var b = { files: n };
    var OT = window.OT;
    var t0 = performance.now();
    var blobs = await Promise.all(Array.from({ length: n }, function (_, k) {
      return fetch('/desktop/api/selftest/fsa?seed=' + k).then(function (r) { return r.blob(); });
    }));
    var files = blobs.map(function (bl, k) { return new File([bl], 'batch' + String(k).padStart(3, '0') + '.fsa'); });
    b.fetch_ms = Math.round(performance.now() - t0);
    b.input_mib = Math.round(blobs.reduce(function (a, x) { return a + x.size; }, 0) / 10485.76) / 100;
    var names = (window.Methods ? Methods.list() : []).map(function (r) { return r.method.name; });
    var meth = names.find(function (x) { return /MiniFiler_GS500/.test(x); }) || names[0];
    b.method = meth;
    b.size_standard = 'CE_G5_HID_GS500';
    Object.assign(OT.state.project, { name: 'Selftest batch', runFolders: [], samples: [], dirty: false });
    // No panel in a fresh profile: continue past the Analysis Requirements Summary instead of asking (not saved).
    if (window.ProjectOptions) ProjectOptions.get().ars = 'continue';
    OT.state.selected.clear();
    if (window.__OT_SELFTEST_PROFILE__) {
      var lp = await fetch('/desktop/api/lab-profile', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(window.__OT_SELFTEST_PROFILE__) });
      b.lab_profile_status = lp.status;
    }
    b.heap_before_mib = heap();
    b.rss_before_mib = await rss();
    t0 = performance.now();
    OT.addSampleFiles(files, 'Batch run', meth, '', b.size_standard);
    b.add_ms = Math.round(performance.now() - t0);
    // Main-thread time spent parsing the analysis responses (Response.json), measured around the UI's own calls.
    var origJson = Response.prototype.json, parseMs = 0, parseBytes = 0;
    Response.prototype.json = async function () {
      if (!/\/api\/analyze/.test(this.url)) return origJson.call(this);
      var txt = await this.text();
      var p0 = performance.now();
      var v = JSON.parse(txt);
      parseMs += performance.now() - p0;
      parseBytes += txt.length;
      return v;
    };
    t0 = performance.now();
    try { await OT.analyze('pending'); } finally { Response.prototype.json = origJson; }
    b.analyze_ms = Math.round(performance.now() - t0);
    b.json_parse_ms = Math.round(parseMs);
    b.json_mib = Math.round(parseBytes / 10485.76) / 100;
    var smp = OT.state.project.samples;
    b.analyzed = smp.filter(function (x) { return x.status === 'analyzed'; }).length;
    b.sizing_accepted = smp.filter(function (x) { return x.analysis && x.analysis.sizing && x.analysis.sizing.status === 'accepted'; }).length;
    b.peaks = smp.reduce(function (a, x) { return a + ((x.analysis && x.analysis.channels) || []).reduce(function (m, ch) { return m + ((ch.peaks || []).length); }, 0); }, 0);
    b.message = OT.state.lastMessage || null;
    b.heap_after_mib = heap();
    b.rss_after_mib = await rss();
    // All samples in the plot window: time until its canvases exist and were drawn.
    smp.forEach(function (x) { OT.state.selected.add(x.id); });
    var oldDoc = OT.state.plotWin && !OT.state.plotWin.closed ? OT.state.plotWin.document : null;
    t0 = performance.now();
    OT.displayPlots();
    b.plot_ready = await wait(function () {
      var w = OT.state.plotWin;
      if (!w || w.closed || w.document === oldDoc || w.document.readyState !== 'complete') return false;
      var cs = w.document.querySelectorAll('#plotarea canvas');
      return cs.length > 0 && !!cs[0]._geom;
    }, 60000);
    await new Promise(function (r) { requestAnimationFrame(function () { requestAnimationFrame(r); }); });
    b.plot_ms = Math.round(performance.now() - t0);
    try {
      var cs = Array.prototype.slice.call(OT.state.plotWin.document.querySelectorAll('#plotarea canvas'));
      b.plot_canvases = cs.length;
      b.plot_painted = cs.filter(function (x) { return !!x._geom; }).length; // panes painted so far (visible ones)
    } catch (e) { b.plot_canvases = String(e); }
    b.heap_plot_mib = heap();
    b.rss_plot_mib = await rss();
    if (window.__OT_SELFTEST_SERVER__ && window.__OT_SELFTEST_PROFILE__) {
      // Save the batch as a server project: provenance for every sample, attributed to the active profile.
      await fetch('/desktop/api/workspace/source', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ source: 'server' }) });
      t0 = performance.now();
      b.saved = await OT.writeProject('Selftest batch ' + Date.now(), { create: true });
      b.save_ms = Math.round(performance.now() - t0);
      var ws = await (await fetch('/desktop/api/workspace/status')).json();
      b.provenance = ws.provenance;
      b.lab_profile = ws.lab_profile;
      await fetch('/desktop/api/workspace/source', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ source: 'local' }) });
    }
    return b;
  };

  if (path === '/idx/' || path === '/idx' || path === '/idx/project.html') {
    window.addEventListener('load', async function () {
      var c = {};
      try {
        c.ot_loaded = await wait(function () { return window.OT && window.OT.state && typeof window.OT.plotPayload === 'function'; });
        c.desktop_shim = !!window.OT_DESKTOP;
        c.user_agent = navigator.userAgent;
        c.local_storage = (function () { try { localStorage.setItem('ot.selftest', '1'); return localStorage.getItem('ot.selftest') === '1'; } catch (e) { return String(e); } })();
        var h = await (await fetch('/api/health')).json();
        c.health = h.status === 'ok';
        c.analysis_version = h.analysis_version;
        // Real core analysis of a synthetic ABIF file, sent as binary body exactly like the UI does.
        var fsa = await (await fetch('/desktop/api/selftest/fsa')).blob();
        var t0 = performance.now();
        var r = await fetch('/api/analyze?filename=selftest.fsa&threshold=100', { method: 'POST', headers: { 'X-Session-Token': window.OT_TOKEN }, body: fsa });
        var j = await r.json();
        c.analyze_status = r.status;
        c.analyze_ms = Math.round(performance.now() - t0);
        c.analyze_channels = (j.channels || []).length;
        c.analyze_peaks = (j.channels || []).reduce(function (n, ch) { return n + ((ch.peaks || []).length); }, 0);
        c.source_sha256 = j.source_sha256 || (j.file && j.file.sha256) || null;
        c.analyze_error = j.error || null;
        // Example project -> Samples Plot in a separate native window.
        await window.OT.loadExample();
        var s = window.OT.state.project.samples.find(function (x) { return x.type === 'sample'; });
        c.example_samples = window.OT.state.project.samples.length;
        window.OT.state.selected.clear();
        window.OT.state.selected.add(s.id);
        window.OT.displayPlots();
        c.plot_opened = await wait(function () { return window.OT.state.plotWin && !window.OT.state.plotWin.closed; }, 15000);
        await sleep(300);
        // Diagnostics for CI: what the opener can see of the popup (same origin).
        await wait(function () { var w = window.OT.state.plotWin; return w && w.document && w.document.readyState === 'complete' && w.__otSelftestStarted; }, 15000);
        try {
          var pw = window.OT.state.plotWin;
          c.plot_window = { location: String(pw.location.href), ready_state: pw.document.readyState, selftest_started: !!pw.__otSelftestStarted, has_opener: pw.opener === window, canvases: pw.document.querySelectorAll('#plotarea canvas').length };
        } catch (e) { c.plot_window = { error: String(e) }; }
        // Export through the UI's own download helper -> native Save As (no dialog in the self-test).
        window.__otDesktopLastSave = null;
        Methods.download('selftest-export.txt', 'OpenTrace export\n', 'text/plain');
        await wait(function () { return window.__otDesktopLastSave; }, 15000);
        c.export = window.__otDesktopLastSave;
        // Same popup kind opened, closed and opened again: the second window is created at the remembered geometry.
        // (On Windows the window-state plugin used to restore popups under a Tauri lock and hung the UI thread here.)
        c.popup_reopen = await popupReopen();
        if (window.__OT_SELFTEST_SERVER__) {
          // The analysed FSA must reach the server in the background (HEAD/PUT by SHA-256).
          var t1 = Date.now();
          while (Date.now() - t1 < 30000) {
            c.sync = await (await fetch('/desktop/api/sync/status')).json();
            if (c.sync.done >= 1 || c.sync.failed) break;
            await sleep(250);
          }
          c.sync_ms = Date.now() - t1;
          // Server projects through the UI's own save/open code: lease, guarded writes, read back.
          await sleep(5000); // let the plot window finish its checks on the example project first
          var src = await fetch('/desktop/api/workspace/source', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ source: 'server' }) });
          c.ws_source_status = src.status;
          // One sample with a real (synthetic) FSA, so the project on the server references a stored original.
          var mnames = (window.Methods ? Methods.list() : []).map(function (r) { return r.method.name; });
          var fsaFile = new File([await (await fetch('/desktop/api/selftest/fsa?genotype=0')).blob()], 'draft-fsa.fsa');
          window.OT.addSampleFiles([fsaFile], 'Draft run', mnames.find(function (x) { return /MiniFiler_GS500/.test(x); }) || mnames[0], '', 'CE_G5_HID_GS500');
          if (window.ProjectOptions) ProjectOptions.get().ars = 'continue';
          c.draft_fsa_analyze = await Promise.race([window.OT.analyze('pending').then(function () { return 'done'; }, function (e) { return String(e); }), sleep(30000).then(function () { return 'timeout'; })]);
          c.draft_fsa_dialog = (document.querySelector('.dialog, dialog[open], .modal') || {}).textContent || null;
          var pname = 'Selftest ' + Date.now();
          c.ws_saved = await window.OT.writeProject(pname, { create: true });
          c.ws_after_save = await (await fetch('/desktop/api/workspace/status')).json();
          window.OT.state.project.samples = [];
          await window.OT.loadProject(pname);
          c.ws_loaded_samples = window.OT.state.project.samples.length;
          c.ws_after_open = await (await fetch('/desktop/api/workspace/status')).json();
          // Lease lost while editing: the save is refused and a full copy goes into the local library as a draft.
          var dl = await (await fetch('/desktop/api/selftest/drop-lease', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ project: pname }) })).json();
          c.draft_lease_dropped = dl.dropped;
          window.__otDesktopLastDraft = null;
          // Example samples carry hashes of demo data that was never stored; only the real FSA must come back.
          c.draft_expected_fsa = window.OT.state.project.samples.filter(function (x) { return x.blob || x.file === 'draft-fsa.fsa'; }).length;
          // As for a sample opened from the server without its file: the draft must take the original from the cache.
          window.OT.state.project.samples.forEach(function (x) { if (x.file === 'draft-fsa.fsa') delete x.blob; });
          c.draft_blobs_in_window = window.OT.state.project.samples.filter(function (x) { return x.blob; }).length;
          window.OT.state.project.samples.forEach(function (x) { x.rev = (x.rev || 0) + 1; });
          window.OT.state.project.dirty = true;
          c.draft_save_refused = (await window.OT.writeProject(pname)) === false;
          await wait(function () { return window.__otDesktopLastDraft; }, 60000);
          c.draft = window.__otDesktopLastDraft;
          c.draft_index = ((await (await fetch('/desktop/api/drafts')).json()).drafts || []).length;
          // The local-write bypass is only for a draft registered by the app, never for an arbitrary name.
          c.draft_bypass_refused = (await fetch('/api/idx-project', { method: 'POST', headers: { 'Content-Type': 'application/json', 'X-Session-Token': window.OT_TOKEN, 'X-OT-Draft': 'Other' }, body: JSON.stringify({ action: 'save_meta', name: 'Other', project: {}, create: true }) })).status === 403;
          await fetch('/desktop/api/workspace/source', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ source: 'local' }) });
          if (c.draft && c.draft.draft) {
            window.OT.state.project.samples = [];
            await window.OT.loadProject(c.draft.draft);
            c.draft_loaded_samples = window.OT.state.project.samples.length;
            c.draft_loaded_analyses = window.OT.state.project.samples.filter(function (x) { return x.analysis; }).length;
          }
          // Lab standards: a local method imported as a server draft; desktop and server hash it identically.
          var lm = ((JSON.parse(localStorage.getItem('opentrace.methods.v1') || 'null') || {}).methods || []).map(function (r) { return r.method; }).filter(Boolean);
          c.local_methods = lm.length;
          if (lm.length) {
            var sr = await fetch('/desktop/api/v1/standards/analysis_method/' + encodeURIComponent(lm[0].name + ' ' + Date.now()), { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ content: lm[0], note: 'selftest' }) });
            c.std_import_status = sr.status;
            var sv = await sr.json();
            var hh = await (await fetch('/desktop/api/standards/hash', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ items: [{ content: lm[0] }] }) })).json();
            c.std_hash_match = sv.content_sha256 === hh.sha256[0];
            c.std_version = sv.version;
          }
        } else {
          c.sync = await (await fetch('/desktop/api/sync/status')).json();
        }
        if (window.__OT_SELFTEST_GENOTYPE__) {
          await sleep(3000);
          c.genotype = await genotypeRun(window.__OT_SELFTEST_GENOTYPE__);
        }
        if (window.__OT_SELFTEST_BATCH__) {
          await sleep(3000); // the example plot window reports first
          c.batch = await batchRun(window.__OT_SELFTEST_BATCH__);
        }
      } catch (e) {
        c.error = String((e && e.stack) || e);
      }
      report({ window: 'main', checks: c });
    });
  }

  if (path === '/idx/plot.html') {
    window.addEventListener('load', async function () {
      var c = {};
      try {
        c.opener_ot = await wait(function () { return window.opener && window.opener.OT && typeof window.opener.OT.plotPayload === 'function'; }, 15000);
        c.canvas = await wait(function () { return document.querySelector('#plotarea canvas'); }, 15000);
        await sleep(1500);
        var cv = Array.prototype.slice.call(document.querySelectorAll('#plotarea canvas'));
        var colored = 0;
        cv.forEach(function (cnv) {
          var d = cnv.getContext('2d').getImageData(0, 0, cnv.width, cnv.height).data;
          for (var i = 0; i < d.length; i += 16) {
            if (d[i + 3] > 0 && !(d[i] > 235 && d[i + 1] > 235 && d[i + 2] > 235) && !(d[i] < 20 && d[i + 1] < 20 && d[i + 2] < 20)) colored++;
          }
        });
        c.canvas_count = cv.length;
        c.canvas_size = cv.map(function (x) { return x.width + 'x' + x.height; });
        c.colored_pixels = colored;
        c.samples_in_payload = ((window.opener.OT.plotPayload() || {}).samples || []).length;
        c.menubar_items = document.querySelectorAll('#menubar > *').length;
      } catch (e) {
        c.error = String((e && e.stack) || e);
      }
      report({ window: 'plot', checks: c });
    });
  }
})();
