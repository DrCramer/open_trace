// Desktop connection page. All server traffic goes through Rust (/desktop/api/*); the token never reaches this page.
(function () {
  'use strict';
  var $ = function (id) { return document.getElementById(id); };
  var api = async function (path, body) {
    var r = await fetch('/desktop/api/' + path, body === undefined ? {} : { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(body) });
    var j = {};
    try { j = await r.json(); } catch (e) {}
    if (!r.ok) throw new Error(j.error || ('HTTP ' + r.status));
    return j;
  };
  var esc = function (s) { return String(s == null ? '' : s).replace(/[&<>"]/g, function (c) { return { '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' }[c]; }); };
  var fmtTime = function (ms) { return ms ? new Date(ms).toLocaleString() : ''; };

  function warn() {
    var insecure = $('http').checked || /^http:/i.test($('url').value.trim());
    $('httpWarn').style.display = insecure ? 'block' : 'none';
  }
  $('http').addEventListener('change', warn);
  $('url').addEventListener('input', warn);

  async function load() {
    var i = await api('info');
    $('url').value = i.server.url || '';
    $('http').checked = !!i.server.allow_insecure_http;
    $('ca').value = i.server.ca_cert_pem || '';
    $('syncFsa').checked = i.server.sync_fsa !== false;
    $('user').value = i.server.username || '';
    $('who').textContent = i.server.logged_in ? ('Вход выполнен: ' + i.server.user + ' (роли: ' + (i.server.roles || []).join(', ') + ')') : 'Вход не выполнен.';
    $('loginBox').hidden = !!i.server.logged_in;
    $('logoutBox').hidden = !i.server.logged_in;
    document.title = 'OpenTrace ' + i.app_version + ' — сервер организации';
    $('srcLocal').checked = i.project_source !== 'server';
    $('srcServer').checked = i.project_source === 'server';
    $('srcServer').disabled = !i.server.logged_in && i.project_source !== 'server';
    warn();
    if (i.server.logged_in) { lists(); }
  }

  async function lists() {
    try {
      var p = await api('server/projects');
      $('projects').tBodies[0].innerHTML = (p.projects || []).map(function (x) {
        return '<tr><td>' + esc(x.name) + '</td><td class="num">' + esc(x.revision) + '</td><td>' + (x.lease ? esc(x.lease.user) + ' до ' + esc(fmtTime(x.lease.expires_ms)) : '') + '</td></tr>';
      }).join('') || '<tr><td colspan="3" class="note">нет проектов</td></tr>';
    } catch (e) { $('projects').tBodies[0].innerHTML = '<tr><td colspan="3" class="err">' + esc(e.message) + '</td></tr>'; }
    try {
      var l = await api('server/lab-profiles');
      $('profiles').tBodies[0].innerHTML = (l.lab_profiles || []).map(function (x) {
        return '<tr><td>' + esc(x.name) + '</td><td class="num">' + esc(x.version) + '</td><td>' + esc(x.approved_by) + '</td><td><code>' + esc(String(x.content_sha256).slice(0, 16)) + '…</code></td></tr>';
      }).join('') || '<tr><td colspan="4" class="note">нет утверждённых профилей</td></tr>';
    } catch (e) { $('profiles').tBodies[0].innerHTML = '<tr><td colspan="4" class="err">' + esc(e.message) + '</td></tr>'; }
  }

  async function save() {
    $('checkOut').textContent = '';
    try {
      var r = await api('server/configure', { url: $('url').value, allow_insecure_http: $('http').checked, ca_cert_pem: $('ca').value, sync_fsa: $('syncFsa').checked });
      $('checkOut').innerHTML = '<span class="ok">Сохранено: ' + esc(r.url) + '</span>' + (r.insecure ? ' <span class="err">(HTTP без шифрования)</span>' : '');
      await load();
      return true;
    } catch (e) { $('checkOut').innerHTML = '<span class="err">' + esc(e.message) + '</span>'; return false; }
  }
  $('save').addEventListener('click', save);
  $('check').addEventListener('click', async function () {
    if (!(await save())) return;
    $('checkOut').textContent = 'Проверка…';
    try {
      var r = await api('server/check', {});
      var s = r.server || {};
      $('checkOut').innerHTML = (r.compatible ? '<span class="ok">Сервер доступен, API v1 совместим</span>' : '<span class="err">Несовместимая версия API сервера</span>') +
        ' · ' + esc(s.server_version) + ' · ' + r.latency_ms + ' мс<br>Алгоритм сервера: ' + esc(s.analysis_version) + ' / клиента: ' + esc(r.analysis_version_local) +
        (r.analysis_version_match ? '' : ' <span class="err">— версии алгоритма различаются</span>') +
        '<br>Математика: сервер ' + esc(s.math_backend) + ', клиент ' + esc(r.math_backend_local);
    } catch (e) { $('checkOut').innerHTML = '<span class="err">' + esc(e.message) + '</span>'; }
  });
  $('login').addEventListener('click', async function () {
    $('authOut').textContent = '';
    try {
      await api('server/login', { username: $('user').value, password: $('pw').value });
      $('pw').value = '';
      await load();
    } catch (e) { $('authOut').innerHTML = '<span class="err">' + esc(e.message) + '</span>'; }
  });
  $('pw').addEventListener('keydown', function (e) { if (e.key === 'Enter') $('login').click(); });
  $('logout').addEventListener('click', async function () { await api('server/logout', {}); await load(); });
  ['srcLocal', 'srcServer'].forEach(function (id) {
    $(id).addEventListener('change', async function () {
      $('srcOut').textContent = '';
      try {
        await api('workspace/source', { source: $(id).value });
        $('srcOut').innerHTML = '<span class="ok">' + ($(id).value === 'server' ? 'Проекты: сервер. Откройте проект заново (Файл → Открыть проект).' : 'Проекты: этот компьютер.') + '</span>';
      } catch (e) { $('srcOut').innerHTML = '<span class="err">' + esc(e.message) + '</span>'; await load(); }
    });
  });
  $('openStandards').addEventListener('click', function () { window.open('/desktop/standards.html', 'opentrace-standards', 'width=1100,height=760'); });
  $('retry').addEventListener('click', async function () { await api('sync/retry', {}); syncTick(); });

  async function syncTick() {
    try {
      var s = await api('sync/status');
      $('sync').innerHTML = 'Ожидают: ' + s.pending + ' · передаётся: ' + s.uploading + ' · на сервере: ' + s.done + ' · ошибок: ' + s.failed +
        ' · уже были на сервере (без повторной передачи): ' + s.deduplicated + (s.paused ? '<br>Пауза: ' + esc(s.paused) : '') +
        (s.last_error ? '<br><span class="err">' + esc(s.last_error) + '</span>' : '') +
        (s.failed_items || []).map(function (f) { return '<br><span class="err">' + esc(f.name) + ': ' + esc(f.error) + '</span>'; }).join('');
    } catch (e) { $('sync').textContent = e.message; }
  }
  async function diagShow() {
    try {
      var d = await api('diag');
      $('diag').innerHTML = 'Журнал и дампы зависаний: ' + esc(d.log_dir) + '<br>Зависаний интерфейса в этом сеансе: ' + d.hangs +
        (d.last_dump ? '<br>Последний дамп: ' + esc(d.last_dump) + ' (может содержать фрагменты данных из памяти; передавайте только разработчикам)' : '');
    } catch (e) { $('diag').textContent = e.message; }
  }
  $('drafts').addEventListener('click', async function () {
    try {
      var r = await api('drafts');
      var rows = (r.drafts || []).slice(-20).reverse();
      $('draftsOut').innerHTML = rows.length ? rows.map(function (d) {
        return esc(d.created) + ' · «' + esc(d.draft) + '» из «' + esc(d.server_project) + '» · образцов ' + d.samples + ' (с FSA ' + d.samples_with_fsa + ')' +
          (d.samples_failed ? ' · <span class="err">не сохранено ' + d.samples_failed + '</span>' : '') + '<br><span class="note">' + esc(d.reason) + '</span>';
      }).join('<br>') + '<br>Открыть: «Проекты: на этом компьютере» → Файл → Открыть проект.' : 'Черновиков нет.';
    } catch (e) { $('draftsOut').innerHTML = '<span class="err">' + esc(e.message) + '</span>'; }
  });
  diagShow();
  load().catch(function (e) { $('checkOut').innerHTML = '<span class="err">' + esc(e.message) + '</span>'; });
  syncTick();
  setInterval(syncTick, 2000);
})();
