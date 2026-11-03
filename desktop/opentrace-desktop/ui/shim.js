// OpenTrace Desktop shim: injected before page scripts in every OpenTrace window of the desktop app. It does not
// change how the web UI analyses or plots; it only adds desktop services around it:
//  * after a successful local analysis of an FSA blob, the same bytes go to the background FSA sync queue
//    (/desktop/api/fsa/sync, binary, SHA-256 deduplicated in Rust) — analysis never waits for the network;
//  * a status-bar cell with the server-sync state (separate from the analysis progress bar) and a "Server" button;
//  * close handling: unsaved changes (the page's own beforeunload logic) ask for confirmation before a native close.
(function () {
  'use strict';
  var ours = location.protocol === 'opentrace:' || location.hostname === 'opentrace.localhost';
  if (!ours || window.__otDesktop) return;
  window.__otDesktop = { version: 1 };
  window.OT_DESKTOP = true;

  var rawFetch = window.fetch.bind(window);
  // WebKitGTK (Linux) custom-scheme requests: a Blob/File body crashes webkit_uri_scheme_request_get_http_body
  // (WebKitGTK 2.52) and large bodies may arrive short. Send bytes as an ArrayBuffer and state the length; the Rust
  // side rejects a body whose length does not match (no silent truncation of FSA data).
  var needsBuffer = location.protocol === 'opentrace:';
  function withLength(init, n) {
    var h = new Headers((init && init.headers) || {});
    h.set('X-OT-Body-Length', String(n));
    return Object.assign({}, init, { headers: h });
  }
  function send(input, init) {
    var body = init && init.body;
    if (body instanceof Blob && needsBuffer) {
      return body.arrayBuffer().then(function (buf) {
        var i = withLength(init, buf.byteLength);
        i.body = buf;
        return rawFetch(input, i);
      });
    }
    if (body instanceof Blob) return rawFetch(input, withLength(init, body.size));
    if (body instanceof ArrayBuffer) return rawFetch(input, withLength(init, body.byteLength));
    if (ArrayBuffer.isView(body)) return rawFetch(input, withLength(init, body.byteLength));
    return rawFetch(input, init);
  }
  var origFetch = send;

  // ---- Local draft when saving a server project fails (drafts.rs) ----
  // The UI saves a project as one save_meta followed by save_sample requests. If any of them fails while projects are
  // kept on the server (lease lost, conflict, no connection), the edits live only in this window. A full copy of the
  // open project then goes into the LOCAL project library under a new name; nothing is sent to the server.
  var saveCycle = null;
  function blobB64(blob) {
    return new Promise(function (res, rej) {
      var r = new FileReader();
      r.onload = function () { var s = String(r.result); res(s.slice(s.indexOf(',') + 1)); };
      r.onerror = function () { rej(r.error); };
      r.readAsDataURL(blob);
    });
  }
  async function postJson(url, headers, body) {
    var r = await origFetch(url, { method: 'POST', headers: headers, body: JSON.stringify(body) });
    var j = null;
    try { j = await r.json(); } catch (e) {}
    if (!r.ok) throw Error((j && j.error) || ('HTTP ' + r.status));
    return j;
  }
  async function createDraft(cyc, why) {
    var st = window.OT && window.OT.state;
    var ids = ((cyc.meta && cyc.meta.samples) || []).map(function (x) { return x.id; });
    var samples = st ? st.project.samples.slice() : [];
    if (!st || ids.length !== samples.length) throw Error('состав проекта изменился во время сохранения');
    var hdr = Object.assign({ 'Content-Type': 'application/json' }, cyc.headers);
    var b = await postJson('/desktop/api/drafts/begin', hdr, { project: cyc.name });
    var dh = Object.assign({ 'X-OT-Draft': encodeURIComponent(b.draft) }, hdr);
    var meta = Object.assign({}, cyc.meta);
    delete meta.security_group; // the server's groups may not exist in the local library: default group
    await postJson('/api/idx-project', dh, { action: 'save_meta', name: b.draft, project: meta, create: true, overwrite: false });
    var withFsa = 0, failed = 0;
    for (var i = 0; i < samples.length; i++) {
      var s = samples[i];
      var payload = { action: 'save_sample', name: b.draft, id: ids[i], data: { analysis: s.analysis || null, compact: s.compact || null, report: s.report || null } };
      try {
        if (s.blob) { payload.fsa_b64 = await blobB64(s.blob); withFsa++; }
        var rj = await postJson('/api/idx-project', dh, payload);
        if (!payload.fsa_b64 && rj && rj.draft_fsa) withFsa++; // original taken from the FSA cache or the server
      } catch (e) { failed++; }
    }
    await postJson('/desktop/api/drafts/finish', hdr, { draft: b.draft, project: cyc.name, reason: why, samples: samples.length, samples_with_fsa: withFsa, samples_failed: failed });
    return { draft: b.draft, samples: samples.length, samples_with_fsa: withFsa, samples_failed: failed };
  }
  async function failedSave(cyc, why) {
    if (cyc.failed) return;
    cyc.failed = true;
    try {
      var ws = await (await origFetch('/desktop/api/workspace/status')).json();
      if (ws.source !== 'server') return;
    } catch (e) { return; }
    // Let the UI finish its own save attempt (it stops at the first error) before reading its project state.
    for (var t = Date.now(); window.OT && window.OT.state && window.OT.state.busy && Date.now() - t < 120000;) await new Promise(function (r) { setTimeout(r, 200); });
    var ru = (document.documentElement.lang || 'ru').indexOf('en') !== 0;
    try {
      var d = await createDraft(cyc, why);
      window.__otDesktopLastDraft = d;
      var text = ru
        ? 'Изменения не записаны на сервер (' + why + ').\n\nПолная копия открытого проекта сохранена на этом компьютере как локальный проект «' + d.draft + '»: образцов ' + d.samples + (d.samples_failed ? ', не удалось сохранить ' + d.samples_failed : '') + '. Открыть её: «Проекты: локально» → «Открыть проект». На сервере ничего не изменено.'
        : 'Changes were not saved to the server (' + why + ').\n\nA full copy of the open project was saved on this computer as the local project "' + d.draft + '" (' + d.samples + ' samples' + (d.samples_failed ? ', ' + d.samples_failed + ' failed' : '') + '). Open it with Projects: local -> Open Project. Nothing was changed on the server.';
      (window.message || window.alert)(text);
    } catch (e) {
      window.__otDesktopLastDraft = { error: String(e && e.message || e) };
      (window.message || window.alert)((ru ? 'Изменения не записаны на сервер, и локальную копию сохранить не удалось: ' : 'Changes were not saved to the server and the local copy failed: ') + (e && e.message || e) + (ru ? '. Не закрывайте окно; сохраните проект под другим именем, когда появится связь.' : '. Keep this window open and save under another name when the connection is back.'));
    }
  }
  function watchProjectSave(url, init, p) {
    if (!init || init.method !== 'POST' || url.split('?')[0].slice(-16) !== '/api/idx-project' || typeof init.body !== 'string') return;
    var h = new Headers(init.headers || {});
    if (h.get('X-OT-Draft')) return;
    var req;
    try { req = JSON.parse(init.body); } catch (e) { return; }
    if (req.action === 'save_meta') saveCycle = { name: req.name, meta: req.project, headers: { 'X-Session-Token': h.get('X-Session-Token') || '', 'X-User-Ticket': h.get('X-User-Ticket') || '' }, failed: false };
    else if (req.action !== 'save_sample') return;
    var cyc = saveCycle;
    if (!cyc || cyc.name !== req.name) return;
    p.then(function (resp) {
      if (resp.ok) return;
      return resp.clone().json().then(function (j) { return (j && j.error) || ('HTTP ' + resp.status); }, function () { return 'HTTP ' + resp.status; })
        .then(function (why) { return failedSave(cyc, why); });
    }, function (e) { return failedSave(cyc, String((e && e.message) || e)); }).catch(function () {});
  }

  window.fetch = function (input, init) {
    var p = send(input, init);
    try {
      var url = typeof input === 'string' ? input : (input && input.url) || '';
      watchProjectSave(url, init, p);
      var body = init && init.body;
      if (init && init.method === 'POST' && /\/api\/analyze\?/.test(url) && body instanceof Blob) {
        p.then(function (resp) {
          if (!resp.ok) return;
          var name = 'sample.fsa';
          try { name = new URLSearchParams(url.split('?')[1]).get('filename') || name; } catch (e) {}
          send('/desktop/api/fsa/sync?name=' + encodeURIComponent(name), { method: 'POST', body: body }).catch(function () {});
        }).catch(function () {});
      }
    } catch (e) {}
    return p;
  };

  // Unsaved state as the page itself defines it (its beforeunload handlers call preventDefault).
  window.__otDesktopIsDirty = function () {
    try {
      var ev = new Event('beforeunload', { cancelable: true });
      window.dispatchEvent(ev);
      return ev.defaultPrevented;
    } catch (e) { return false; }
  };
  window.__otDesktopRequestClose = function () {
    var ru = (document.documentElement.lang || 'ru').indexOf('en') !== 0;
    var msg = ru ? 'Есть несохранённые изменения. Закрыть окно без сохранения?' : 'There are unsaved changes. Close without saving?';
    if (window.__otDesktopIsDirty() && !window.confirm(msg)) return;
    origFetch('/desktop/api/window/close-confirmed', { method: 'POST' }).catch(function () {});
  };
  // window.close() from the page (its own Close/OK buttons, after its own unsaved-changes check) or from the opener:
  // WebView2 (wry) answers it by destroying only the webview, which leaves an empty white native window behind
  // (Windows; seen with the Panel Manager). Close the native window through the app instead.
  var nativeClose = window.close.bind(window);
  window.close = function () {
    origFetch('/desktop/api/window/close-confirmed', { method: 'POST' }).catch(function () { try { nativeClose(); } catch (e) {} });
  };
  // ---- Exports: native "Save As" (saveas.rs) ----
  // The UI exports by clicking a temporary <a download href="blob:…">. Keep the Blob behind each object URL (the UI
  // revokes it a second later) and turn such a click into a save request; one dialog at a time.
  var blobs = new Map();
  var createURL = URL.createObjectURL.bind(URL), revokeURL = URL.revokeObjectURL.bind(URL);
  URL.createObjectURL = function (obj) { var u = createURL(obj); if (obj instanceof Blob) blobs.set(u, obj); return u; };
  URL.revokeObjectURL = function (u) { blobs.delete(u); return revokeURL(u); };
  var saveChain = Promise.resolve();
  function nativeDownload(name, blob) {
    var u = createURL(blob);
    var a = document.createElement('a');
    a.href = u; a.download = name; a.setAttribute('data-ot-native', '1');
    document.body.appendChild(a); a.click(); a.remove();
    setTimeout(function () { revokeURL(u); }, 5000);
  }
  document.addEventListener('click', function (e) {
    var a = e.target && e.target.closest ? e.target.closest('a[download]') : null;
    if (!a || a.hasAttribute('data-ot-native')) return;
    var blob = blobs.get(a.href);
    if (!blob) return; // not an export blob: leave it to the engine
    e.preventDefault();
    var name = a.getAttribute('download') || 'export';
    saveChain = saveChain.then(function () {
      return blob.arrayBuffer().then(function (buf) {
        return origFetch('/desktop/api/save-as?name=' + encodeURIComponent(name), { method: 'POST', body: buf });
      }).then(function (r) { return r.json().then(function (j) { if (!r.ok) throw Error(j.error || ('HTTP ' + r.status)); return j; }); })
        .then(function (j) {
          window.__otDesktopLastSave = j;
          if (j.saved) window.__otDesktopDownloaded(j.saved, true);
        })
        .catch(function (err) {
          // Dialog unavailable: fall back to the engine's download into the Downloads folder.
          window.__otDesktopLastSave = { error: String((err && err.message) || err) };
          nativeDownload(name, blob);
        });
    });
  }, true);
  window.__otDesktopDownloaded = function (path, ok) {
    var el = document.getElementById('statusText') || document.getElementById('markerInfo');
    if (el) el.textContent = ok ? ((document.documentElement.lang || 'ru').indexOf('en') === 0 ? 'Saved: ' : 'Сохранено: ') + path : 'Download failed';
  };

  // Accidental reload (F5, Ctrl+R, Ctrl+Shift+R) would drop the in-memory project; the page's own shortcuts run
  // first (document listeners) and keep working, only the browser default is suppressed.
  window.addEventListener('keydown', function (e) {
    if (e.defaultPrevented) return;
    var k = e.key;
    if (k === 'F5' || ((e.ctrlKey || e.metaKey) && (k === 'r' || k === 'R'))) e.preventDefault();
  });

  function syncCell() {
    var bar = document.querySelector('.statusbar');
    if (!bar || !document.getElementById('statusText') || document.getElementById('otSync')) return;
    var cell = document.createElement('div');
    cell.className = 'cell';
    cell.id = 'otSync';
    cell.style.cssText = 'white-space:nowrap;cursor:pointer;min-width:190px';
    cell.title = 'Синхронизация исходных FSA с сервером (анализ выполняется локально и не ждёт сети)';
    cell.addEventListener('click', function () {
      window.open('/desktop/connection.html', 'opentrace-connection', 'width=760,height=640');
    });
    var progress = document.getElementById('progress');
    bar.insertBefore(cell, progress || null);
    function tick() {
      origFetch('/desktop/api/sync/status').then(function (r) { return r.json(); }).then(function (s) {
        var t;
        if (!s.server) t = 'Сервер: не настроен';
        else if (!s.logged_in) t = 'Сервер: нет входа' + (s.pending ? ' · ожидают ' + s.pending : '');
        else if (s.pending || s.uploading) t = 'Синхр. FSA: ' + (s.uploading ? 'передача, ' : '') + 'ожидают ' + (s.pending + s.uploading);
        else if (s.failed) t = 'Синхр. FSA: ошибок ' + s.failed;
        else t = 'Синхр. FSA: ✓ ' + s.done;
        if (s.last_error && (s.pending || s.failed)) t += ' ⚠';
        cell.textContent = t;
        cell.style.color = s.failed ? '#b00' : '';
      }).catch(function () { cell.textContent = 'Синхр.: —'; });
    }
    tick();
    setInterval(tick, 3000);

    // Project source / lock state: separate cell + a banner while the server project is read-only.
    var ws = document.createElement('div');
    ws.className = 'cell';
    ws.id = 'otWs';
    ws.style.cssText = 'white-space:nowrap;min-width:150px;cursor:pointer';
    ws.addEventListener('click', function () {
      window.open('/desktop/provenance.html', 'opentrace-provenance', 'width=980,height=640');
    });
    bar.insertBefore(ws, cell);
    var banner = null;
    function wsTick() {
      origFetch('/desktop/api/workspace/status').then(function (r) { return r.json(); }).then(function (w) {
        if (w.source !== 'server') { ws.textContent = 'Проекты: локально'; ws.title = 'Проекты хранятся на этом компьютере'; }
        else if (!w.project) { ws.textContent = 'Проекты: сервер'; ws.title = w.server || ''; }
        else if (w.editing) { ws.textContent = 'Сервер: редактирование ✎'; ws.title = 'Проект «' + w.project + '», ревизия ' + w.revision + '. Блокировка продлевается автоматически.'; }
        else { ws.textContent = 'Сервер: только чтение'; ws.title = w.readonly || ''; }
        // Provenance and the active laboratory profile in the tooltip; a warning mark when something was not recorded.
        var p = w.provenance || {}, lp = w.lab_profile;
        var lines = [];
        if (lp && lp.verified) lines.push('Лабораторный профиль: ' + lp.name + ' v' + lp.version);
        else if (lp && lp.verified === false) lines.push('Лабораторный профиль не используется: ' + lp.error);
        if (p.posted || p.ambiguous || p.failed || p.profile_mismatch) {
          lines.push('Происхождение анализов записано: ' + (p.posted || 0) + (p.ambiguous ? '; не записано (файл анализировался с разными настройками): ' + p.ambiguous : '') +
            (p.failed ? '; ошибок: ' + p.failed + ' (' + (p.last_error || '') + ')' : '') +
            (p.profile_mismatch ? '; без ссылки на профиль: ' + p.profile_mismatch + ' (' + (p.last_mismatch || '') + ')' : ''));
        }
        if (lines.length) {
          ws.title = (ws.title ? ws.title + '\n' : '') + lines.join('\n') + '\nЩелчок — происхождение анализов проекта';
          if ((p.failed || p.ambiguous || p.profile_mismatch || (lp && lp.verified === false)) && ws.textContent.indexOf('⚠') < 0) ws.textContent += ' ⚠';
        }
        var ro = w.source === 'server' && w.project && !w.editing ? (w.readonly || 'только чтение') : null;
        if (ro && !banner) {
          banner = document.createElement('div');
          banner.id = 'otReadOnly';
          banner.style.cssText = 'background:#fff3cd;border-bottom:1px solid #e0b000;padding:3px 8px;font-size:12px';
          document.body.insertBefore(banner, document.body.firstChild.nextSibling);
        }
        if (banner) {
          if (ro) banner.textContent = 'Проект «' + w.project + '» открыт только для чтения: ' + ro + '. Сохранение на сервер недоступно.';
          else { banner.remove(); banner = null; }
        }
      }).catch(function () {});
    }
    wsTick();
    setInterval(wsTick, 3000);
  }
  if (document.readyState === 'loading') document.addEventListener('DOMContentLoaded', syncCell);
  else syncCell();
})();
