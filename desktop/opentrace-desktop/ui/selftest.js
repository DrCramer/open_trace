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
        } else {
          c.sync = await (await fetch('/desktop/api/sync/status')).json();
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
