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
          var pname = 'Selftest ' + Date.now();
          c.ws_saved = await window.OT.writeProject(pname, { create: true });
          c.ws_after_save = await (await fetch('/desktop/api/workspace/status')).json();
          window.OT.state.project.samples = [];
          await window.OT.loadProject(pname);
          c.ws_loaded_samples = window.OT.state.project.samples.length;
          c.ws_after_open = await (await fetch('/desktop/api/workspace/status')).json();
          await fetch('/desktop/api/workspace/source', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ source: 'local' }) });
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
