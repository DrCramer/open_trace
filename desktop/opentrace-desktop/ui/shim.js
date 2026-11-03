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
  window.fetch = function (input, init) {
    var p = send(input, init);
    try {
      var url = typeof input === 'string' ? input : (input && input.url) || '';
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
