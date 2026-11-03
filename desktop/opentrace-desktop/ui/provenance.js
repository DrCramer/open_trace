// Analysis provenance of the server project open in the main window: what was analysed (FSA hash), with which exact
// settings, algorithm version, by whom, and whether it is attributed to an approved laboratory profile. Read-only;
// the records are written by the desktop when samples are saved (see provenance.rs).
(function () {
  'use strict';
  var $ = function (id) { return document.getElementById(id); };
  var esc = function (s) { return String(s == null ? '' : s).replace(/[&<>"]/g, function (c) { return { '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' }[c]; }); };
  var short = function (h) { return h ? '<code title="' + esc(h) + '">' + esc(String(h).slice(0, 12)) + '</code>' : '—'; };
  var rows = [];
  async function load() {
    $('detail').innerHTML = '';
    var ws = await (await fetch('/desktop/api/workspace/status')).json();
    var p = ws.provenance || {}, lp = ws.lab_profile;
    $('local').innerHTML = 'В этом сеансе: записано ' + (p.posted || 0) +
      (p.ambiguous ? ', <span class="warn">не записано (файл анализировался с разными настройками): ' + p.ambiguous + '</span>' : '') +
      (p.profile_mismatch ? ', <span class="warn">без ссылки на профиль: ' + p.profile_mismatch + ' — ' + esc(p.last_mismatch || '') + '</span>' : '') +
      (p.failed ? ', <span class="err">ошибок: ' + p.failed + ' — ' + esc(p.last_error || '') + '</span>' : '') +
      '. Активный профиль: ' + (lp && lp.verified ? esc(lp.name) + ' v' + lp.version : (lp && lp.error ? '<span class="err">' + esc(lp.error) + '</span>' : 'не выбран')) + '.';
    if (ws.source !== 'server' || !ws.project) {
      $('head').innerHTML = 'Записи происхождения есть только у проектов на сервере. Откройте проект в режиме «Проекты на сервере».';
      $('list').tBodies[0].replaceChildren();
      return;
    }
    $('head').innerHTML = 'Проект «' + esc(ws.project) + '», ревизия ' + esc(ws.revision) + '. Исследовательское ПО, не валидировано для диагностики и судебной экспертизы.';
    var r = await fetch('/desktop/api/v1/projects/' + encodeURIComponent(ws.project) + '/analyses');
    var j = {};
    try { j = await r.json(); } catch (e) {}
    if (!r.ok) { $('list').tBodies[0].innerHTML = '<tr><td colspan="8" class="err">' + esc(j.error || ('HTTP ' + r.status)) + '</td></tr>'; return; }
    rows = j.analyses || [];
    var tb = $('list').tBodies[0];
    tb.replaceChildren();
    rows.slice().reverse().forEach(function (a) {
      var tr = document.createElement('tr');
      tr.innerHTML = '<td>' + esc(a.sample_id) + '</td><td>' + esc(new Date(a.created_ms).toLocaleString()) + '</td><td>' + esc(a.created_by) + '</td><td>' + esc(a.analysis_version) +
        '</td><td>' + short(a.fsa_sha256) + '</td><td>' + short(a.settings_sha256) + '</td><td>' +
        (a.lab_profile ? '<span class="ok">' + esc(a.lab_profile) + ' v' + esc(a.lab_profile_version) + '</span> ' + short(a.lab_profile_sha256) : '—') + '</td><td>' + esc(a.project_revision) + '</td>';
      tr.addEventListener('click', function () {
        Array.prototype.forEach.call(tb.rows, function (x) { x.className = ''; });
        tr.className = 'sel';
        $('detail').innerHTML = '<h3 style="font-size:13px;margin:10px 0 4px">Образец ' + esc(a.sample_id) + '</h3><div class="note">FSA: <code>' + esc(a.fsa_sha256) +
          '</code><br>Результат ядра (SHA-256 ответа): <code>' + esc(a.result_sha256 || '—') + '</code><br>Настройки (канонический SHA-256): <code>' + esc(a.settings_sha256) +
          '</code></div><pre>' + esc(JSON.stringify(a.settings, null, 1)) + '</pre>';
      });
      tb.append(tr);
    });
    if (!rows.length) tb.innerHTML = '<tr><td colspan="8" class="note">записей нет — они появляются при сохранении проанализированных образцов</td></tr>';
  }
  $('reload').addEventListener('click', function () { load().catch(function (e) { $('head').innerHTML = '<span class="err">' + esc(e.message) + '</span>'; }); });
  load().catch(function (e) { $('head').innerHTML = '<span class="err">' + esc(e.message) + '</span>'; });
})();
