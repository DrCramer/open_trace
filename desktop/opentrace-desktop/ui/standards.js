// Laboratory standards page of OpenTrace Desktop. Server calls go through Rust (/desktop/api/v1/*), which holds the
// session token; the server enforces roles (author, approver = second person, admin).
(function () {
  'use strict';
  var $ = function (id) { return document.getElementById(id); };
  var esc = function (s) { return String(s == null ? '' : s).replace(/[&<>"]/g, function (c) { return { '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' }[c]; }); };
  var KINDS = { analysis_method: 'Метод анализа', kit: 'Набор (панели)', bin_set: 'Бины', stutter: 'Статтеры', size_standard: 'Стандарт длин', matrix: 'Матрица', lab_profile: 'Лабораторный профиль' };
  var STATUS = { draft: 'черновик', pending_review: 'на утверждении', approved: 'утверждён', deprecated: 'выведен', archived: 'в архиве' };
  var enc = encodeURIComponent;
  async function v1(method, path, body) {
    var r = await fetch('/desktop/api/v1' + path, body === undefined ? { method: method } : { method: method, headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(body) });
    var j = {};
    try { j = await r.json(); } catch (e) {}
    if (!r.ok) throw new Error(j.error || ('HTTP ' + r.status));
    return j;
  }
  var all = [];
  var selected = null;

  document.querySelectorAll('.tabsx button').forEach(function (b) {
    b.addEventListener('click', function () {
      document.querySelectorAll('.tabsx button').forEach(function (x) { x.classList.toggle('on', x === b); });
      ['list', 'import', 'profile'].forEach(function (t) { $('t-' + t).hidden = t !== b.dataset.t; });
      if (b.dataset.t === 'profile') buildProfileForm();
    });
  });
  Object.keys(KINDS).forEach(function (k) { $('kind').append(new Option(KINDS[k], k)); });

  async function loadAll() {
    try {
      var me = await v1('GET', '/me');
      $('who').textContent = 'Пользователь: ' + me.name + ' · роли: ' + (me.roles || []).join(', ') + '. Исследовательское ПО, не валидировано для диагностики и судебной экспертизы.';
    } catch (e) { $('who').innerHTML = '<span class="err">' + esc(e.message) + ' — войдите на сервер в окне «Сервер организации».</span>'; return; }
    all = (await v1('GET', '/standards')).standards || [];
    drawList();
  }

  function latestOnly(rows) {
    var best = {};
    rows.forEach(function (r) { var k = r.kind + '\u0001' + r.name; if (!best[k] || best[k].version < r.version) best[k] = r; });
    return rows.filter(function (r) { return best[r.kind + '\u0001' + r.name] === r; });
  }

  function drawList() {
    var rows = all.filter(function (r) { return !$('kind').value || r.kind === $('kind').value; });
    if ($('latest').checked) rows = latestOnly(rows);
    var tb = $('list').tBodies[0];
    tb.replaceChildren();
    rows.forEach(function (r) {
      var tr = document.createElement('tr');
      tr.innerHTML = '<td>' + esc(KINDS[r.kind] || r.kind) + '</td><td>' + esc(r.name) + '</td><td class="num">' + r.version + '</td><td><span class="st ' + esc(r.status) + '">' + esc(STATUS[r.status] || r.status) + '</span></td><td>' + esc(r.created_by) + '</td><td>' + esc(r.approved_by || '') + '</td><td><code>' + esc(String(r.content_sha256).slice(0, 12)) + '</code></td>';
      if (selected && selected.kind === r.kind && selected.name === r.name && selected.version === r.version) tr.className = 'sel';
      tr.addEventListener('click', function () { selected = r; drawList(); showDetail(r); });
      tb.append(tr);
    });
    if (!rows.length) tb.innerHTML = '<tr><td colspan="7" class="note">нет стандартов</td></tr>';
  }
  $('kind').addEventListener('change', drawList);
  $('latest').addEventListener('change', drawList);
  $('reload').addEventListener('click', loadAll);

  async function showDetail(r) {
    var d = $('detail');
    d.innerHTML = 'Загрузка…';
    try {
      var v = await v1('GET', '/standards/' + enc(r.kind) + '/' + enc(r.name) + '/' + r.version);
      var acts = [];
      if (v.status === 'draft') acts.push(['submit', 'Отправить на утверждение']);
      if (v.status === 'pending_review') acts.push(['approve', 'Утвердить'], ['reject', 'Отклонить…']);
      if (v.status === 'approved') acts.push(['deprecate', 'Вывести из употребления']);
      if (v.status === 'deprecated') acts.push(['archive', 'В архив']);
      d.innerHTML = '<h3 style="font-size:13px;margin:10px 0 4px">' + esc(KINDS[v.kind] || v.kind) + ' «' + esc(v.name) + '», версия ' + v.version + ' — ' + esc(STATUS[v.status]) + '</h3>' +
        '<div class="note">SHA-256: <code>' + esc(v.content_sha256) + '</code><br>Автор: ' + esc(v.created_by) + (v.submitted_by ? ' · отправил: ' + esc(v.submitted_by) : '') + (v.approved_by ? ' · утвердил: ' + esc(v.approved_by) + ' ' + new Date(v.approved_ms).toLocaleString() : '') + (v.note ? '<br>Примечание: ' + esc(v.note) : '') + '</div>' +
        '<div class="row" id="acts"></div><div id="actOut"></div><pre>' + esc(JSON.stringify(v.content, null, 1).slice(0, 20000)) + '</pre>';
      acts.forEach(function (a) {
        var b = document.createElement('button');
        b.className = 'btn';
        b.textContent = a[1];
        b.addEventListener('click', async function () {
          var body = {};
          if (a[0] === 'reject') { var reason = window.prompt('Причина отклонения'); if (!reason) return; body.reason = reason; }
          try {
            await v1('POST', '/standards/' + enc(v.kind) + '/' + enc(v.name) + '/' + v.version + '/' + a[0], body);
            await loadAll();
            selected = { kind: v.kind, name: v.name, version: v.version };
            drawList();
            showDetail(selected);
          } catch (e) { $('actOut').innerHTML = '<span class="err">' + esc(e.message) + '</span>'; }
        });
        $('acts').append(b);
      });
    } catch (e) { d.innerHTML = '<span class="err">' + esc(e.message) + '</span>'; }
  }

  // ---------------------------------------------------------------- import from localStorage
  function ls(key) { try { return JSON.parse(localStorage.getItem(key) || 'null'); } catch (e) { return null; } }
  function localItems() {
    var out = [];
    // Stores keep records {method|standard|matrix: object, created, modified}; only the object itself is a standard.
    ((ls('opentrace.methods.v1') || {}).methods || []).forEach(function (r) { var m = r && r.method; if (m && m.name) out.push({ kind: 'analysis_method', name: m.name, content: m }); });
    ((ls('opentrace.kits.v1') || {}).kits || []).forEach(function (k) {
      if (!k || !k.name) return;
      var kit = {};
      Object.keys(k).forEach(function (f) { if (f !== 'binSets' && f !== 'stutter') kit[f] = k[f]; });
      out.push({ kind: 'kit', name: k.name, content: kit });
      (k.binSets || []).forEach(function (s) { out.push({ kind: 'bin_set', name: k.name + ' · ' + s.name, content: Object.assign({ kit: k.name }, s) }); });
      if (k.stutter) out.push({ kind: 'stutter', name: k.name, content: { kit: k.name, stutter: k.stutter } });
    });
    ((ls('opentrace.sizestd.v1') || {}).standards || []).forEach(function (r) { var s = r && r.standard; if (s && s.name) out.push({ kind: 'size_standard', name: s.name, content: s }); });
    ((ls('opentrace.matrices.v1') || {}).matrices || []).forEach(function (r) { var m = r && r.matrix; if (m && m.name) out.push({ kind: 'matrix', name: m.name, content: m }); });
    return out;
  }
  var scanned = [];
  async function scan(keepMessages) {
    if (!keepMessages) $('importOut').textContent = '';
    try {
      await loadAll();
      var items = localItems();
      var h = await (await fetch('/desktop/api/standards/hash', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ items: items }) })).json();
      scanned = items.map(function (it, i) {
        it.sha256 = h.sha256[i];
        var versions = all.filter(function (r) { return r.kind === it.kind && r.name === it.name; });
        var same = versions.find(function (r) { return r.content_sha256 === it.sha256; });
        var last = versions.reduce(function (a, r) { return !a || r.version > a.version ? r : a; }, null);
        it.same = same || null;
        it.state = same ? 'совпадает с v' + same.version + ' (' + (STATUS[same.status] || same.status) + ')' : last ? 'отличается от v' + last.version + ' (' + (STATUS[last.status] || last.status) + ') → новый черновик v' + (last.version + 1) : 'нет на сервере → черновик v1';
        return it;
      });
      var tb = $('local').tBodies[0];
      tb.replaceChildren();
      scanned.forEach(function (it, i) {
        var tr = document.createElement('tr');
        tr.innerHTML = '<td><input type="checkbox" data-i="' + i + '"' + (it.same ? ' disabled' : ' checked') + '></td><td>' + esc(KINDS[it.kind]) + '</td><td>' + esc(it.name) + '</td><td><code>' + esc(it.sha256.slice(0, 12)) + '</code></td><td>' + esc(it.state) + '</td>';
        tb.append(tr);
      });
      if (!scanned.length) tb.innerHTML = '<tr><td colspan="5" class="note">в этом приложении нет сохранённых методов, наборов, стандартов длин и матриц</td></tr>';
      $('doImport').disabled = !scanned.some(function (it) { return !it.same; });
    } catch (e) { $('importOut').innerHTML = '<span class="err">' + esc(e.message) + '</span>'; }
  }
  $('scan').addEventListener('click', function () { scan(false); });
  $('doImport').addEventListener('click', async function () {
    var picks = Array.prototype.slice.call(document.querySelectorAll('#local input[type=checkbox]:checked')).map(function (c) { return scanned[+c.dataset.i]; });
    var note = 'Импорт из OpenTrace Desktop (localStorage), ' + new Date().toISOString().slice(0, 10);
    var lines = [];
    for (var i = 0; i < picks.length; i++) {
      var it = picks[i];
      try {
        var v = await v1('POST', '/standards/' + enc(it.kind) + '/' + enc(it.name), { content: it.content, note: note });
        lines.push('<span class="ok">✓ ' + esc(KINDS[it.kind]) + ' «' + esc(it.name) + '» → черновик v' + v.version + '</span>');
      } catch (e) { lines.push('<span class="err">✗ ' + esc(KINDS[it.kind]) + ' «' + esc(it.name) + '»: ' + esc(e.message) + '</span>'); }
      $('importOut').innerHTML = lines.join('<br>');
    }
    await scan(true); // results stay visible; the table now shows the new server versions
  });

  // ---------------------------------------------------------------- lab profile
  var MEMBERS = [['analysis_method', true], ['kit', true], ['bin_set', true], ['size_standard', true], ['stutter', false], ['matrix', false]];
  async function buildProfileForm() {
    if (!all.length) await loadAll();
    var box = $('members');
    box.replaceChildren();
    MEMBERS.forEach(function (m) {
      var row = document.createElement('div');
      row.className = 'row';
      var sel = document.createElement('select');
      sel.id = 'm-' + m[0];
      sel.append(new Option(m[1] ? '— выберите —' : '— нет —', ''));
      all.filter(function (r) { return r.kind === m[0] && r.status === 'approved'; }).forEach(function (r) { sel.append(new Option(r.name + ' v' + r.version, JSON.stringify({ kind: r.kind, name: r.name, version: r.version }))); });
      var l = document.createElement('label');
      l.className = 'l';
      l.textContent = KINDS[m[0]] + (m[1] ? ' *' : '');
      row.append(l, sel);
      box.append(row);
    });
  }
  $('mkProfile').addEventListener('click', async function () {
    var name = $('pname').value.trim();
    if (!name) { $('profileOut').innerHTML = '<span class="err">Укажите имя профиля</span>'; return; }
    var members = [];
    for (var i = 0; i < MEMBERS.length; i++) {
      var v = $('m-' + MEMBERS[i][0]).value;
      if (v) members.push(JSON.parse(v));
      else if (MEMBERS[i][1]) { $('profileOut').innerHTML = '<span class="err">Выберите: ' + esc(KINDS[MEMBERS[i][0]]) + '</span>'; return; }
    }
    try {
      var r = await v1('POST', '/standards/lab_profile/' + enc(name), { content: { members: members } });
      $('profileOut').innerHTML = '<span class="ok">Черновик профиля «' + esc(name) + '» v' + r.version + ' создан. Отправьте его на утверждение во вкладке «Стандарты сервера».</span>';
      await loadAll();
    } catch (e) { $('profileOut').innerHTML = '<span class="err">' + esc(e.message) + '</span>'; }
  });

  loadAll().catch(function (e) { $('who').innerHTML = '<span class="err">' + esc(e.message) + '</span>'; });
})();
