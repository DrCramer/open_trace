#!/usr/bin/env python3
"""Builds docs/desktop/UI_COMMAND_INVENTORY.md from a registry dump of the four windows (ui_inventory_dump.cjs) and the
baseline capture of the classic UI before the shell (menus and toolbars as built by buildMenubar / the toolbar code).
    python3 ui_inventory_md.py registry.json baseline.json > docs/desktop/UI_COMMAND_INVENTORY.md
"""
import json, sys, collections

reg = json.load(open(sys.argv[1]))['res']
base = {w['window']: w for w in json.load(open(sys.argv[2]))['windows']}
WIN = {'main': 'Главное окно (project.html)', 'plot': 'Окно графиков (plot.html)', 'panels': 'Менеджер панелей (panels.html)', 'report': 'Менеджер отчётов (report.html)'}
BASEKEY = {'main': 'project', 'plot': 'plot', 'panels': 'panels', 'report': 'report'}

def count(items):
    return sum(0 if it == '-' else 1 + (count(it['sub']) if it.get('sub') else 0) for it in items)

def esc(s):
    return str(s).replace('|', '\\|').replace('\n', ' ')

NEW_AREAS = ('.tabs', '.right', '.menu.plotsMenu', '.menu.server', '.menu.view.plotSurface')
def is_new(c):
    a, local = c['area'], c['id'].split('/', 1)[1]
    if any(a.endswith(x) or (x + '.') in a for x in NEW_AREAS) or local.startswith('shell.') or local == 'plotSurface' and a.endswith('.menu.view'):
        return True
    return False

def kind_ru(c):
    return {'section': 'раздел меню', 'action': 'команда', 'submenu': 'подменю', 'element': 'элемент'}.get(c['kind'], c['kind'])

def where(c):
    a = c['area']
    if '.menu' in a:
        return 'меню: ' + c['path']
    if a.endswith('.toolbar') or '.toolbar' in a:
        return 'панель инструментов'
    if a.endswith('.editbar'):
        return 'панель правки аллелей'
    if a.endswith('.status'):
        return 'строка состояния'
    if a.endswith('.tabs'):
        return 'вкладки рабочих областей (2-я строка)'
    if a.endswith('.right'):
        return 'правая группа строки меню'
    return a

def origin(c):
    if is_new(c):
        return 'новое (оболочка)'
    a = c['area']
    if '.menu' in a:
        return 'то же меню (ID-X/классический вид)'
    if a.endswith('.status'):
        return 'строка состояния (как было)'
    if '.toolbar' in a or a.endswith('.editbar'):
        return 'та же панель (как было)'
    return '—'

def test_of(c):
    a, local = c['area'], c['id'].split('/', 1)[1]
    t = ['T1']                                            # registry coverage
    if c['critical']:
        t.append('T5')
    if a == 'main.toolbar' and local in ('tbFind', 'help', 'tbAnalyze'):
        t.append('T3/T4')
    if a == 'plot.toolbar2' and local == 'bins':
        t.append('T3')
    if local.startswith('shell.'):
        t.append('T2' if local == 'shell.palette' else 'T3' if local == 'shell.customize' else 'T6')
    if a.endswith('.tabs'):
        t.append('T7')
    if a.endswith('.right') and local in ('theme', 'themeToggle'):
        t.append('T8')
    return ', '.join(t)

out = []
P = out.append
P('# OpenTrace Desktop — реестр команд и элементов интерфейса (v10.4)\n')
P('Документ сгенерирован из живого реестра команд `shell.js` (`Shell.commands()` / `Shell.areas()`) всех четырёх окон '
  'после сборки меню и панелей (`desktop/opentrace-desktop/tests/e2e/ui_inventory_dump.cjs` + `ui_inventory_md.py`). '
  'Поле «до» — снимок того же UI до оболочки (меню через `buildMenubar`, кнопки панелей), снятый на коммите `e4fcfb6`.\n')
P('**Принцип.** Оболочка — адаптер над существующими обработчиками: каждый пункт меню, кнопка, вкладка и элемент статуса '
  'получает стабильный ID `<область>/<id>` (для меню id = ключ i18n пункта), видимость и порядок задаются раскладкой '
  'пользователя (`desktop_ui_layout_v1`). Обработчики, алгоритмы анализа, API и форматы данных не менялись. '
  'Скрытие убирает один способ вызова: команда остаётся в палитре (Ctrl+Shift+P) и на своих горячих клавишах.\n')

P('## Сводка «до → после»\n')
P('| Окно | Разделов меню до → после | Пунктов меню до → после | Элементов панелей до → после | Всего ID в реестре | Новые ID оболочки |')
P('|---|---|---|---|---|---|')
for w in ('main', 'plot', 'panels', 'report'):
    b = base[BASEKEY[w]]; c = reg[w]['commands']
    mb = len(b['menus']); ib = sum(count(t['items']) for t in b['menus']); tb = sum(len([i for i in t['items'] if i != '-']) for t in b['toolbars'])
    ma = len([x for x in c if x['kind'] == 'section']); ia = len([x for x in c if '.menu.' in x['area'] and x['kind'] != 'section'])
    ta = len([x for x in c if '.toolbar' in x['area'] or x['area'].endswith('.editbar')])
    P(f'| {WIN[w]} | {mb} → {ma} | {ib} → {ia} | {tb} → {ta} | {len(c)} | {len([x for x in c if is_new(x)])} |')
P('')
P('Пояснения к счёту:\n')
P('* пункты меню «после» = прежние + 3 пункта оболочки в «Вид» каждого окна (Настроить интерфейс…, Палитра команд…, Восстановить интерфейс); '
  'в главном окне добавлен раздел «Графики» (5 команд, все — существующие обработчики: показать графики, журнал правок, смещения бинов, приборы, настройки графиков) '
  'только для промышленного вида; в окне графиков — подменю «Фон графиков (это окно)» (4 ID);')
P('* панели: подпись и её выпадающий список («Настройка таблицы:», «Настройка графика:», «Панели:», «Набор бинов:», «Настройка отчёта:») '
  'раньше считались двумя элементами, теперь это один элемент (показываются/скрываются вместе); в окне графиков добавлена панель правки аллелей '
  '(9 ID — она была и раньше, но не входила в снимок «до», так как это не `.toolbar`);')
P('* desktop-приложение дополнительно имеет раздел «Сервер» (Сервер организации…, Лабораторные стандарты…, Происхождение анализов…) и '
  'критичные ячейки статуса `otSync`/`otWs` из `shim.js`; в WEB-снимке их нет (они видны только в desktop).\n')

P('## Где теперь что (сопоставление верхнего уровня)\n')
P('| Было (ID-X / классический OpenTrace) | Стало (промышленная оболочка) |')
P('|---|---|')
for a, b in [
    ('Строка меню окна проекта: Файл, Правка, Анализ, Вид, Инструменты, Администрирование, Справка', 'Одна строка: те же меню + «Графики» (+ «Сервер» в desktop); справа закреплены статус редактирования/только чтения, «Команды» (Ctrl+Shift+P), лабораторный профиль (desktop), тема, ⚙ быстрые настройки, ◐ светлая/тёмная, пользователь'),
    ('Вкладки Образцы / Сводка анализа / Генотипы / Информация / Raw Data / EPT', '2-я строка: Проекты | Графики | Генотипы | Стандарты | Журнал. «Проекты» содержит прежние вкладки Образцы, Сводка, Информация, Raw, EPT; «Генотипы» — прежняя вкладка Генотипы'),
    ('Инструменты → Manager, Panel Manager, Report Manager, редакторы методов, стандартов, Size Matching, таблиц', 'Те же пункты меню + страница «Стандарты» (список кнопок тех же обработчиков; в desktop — «Лабораторные стандарты…» и «Сервер организации…»)'),
    ('Вид → журнал правок обозначений; Администрирование → аудит, эл. подпись; desktop: происхождение анализов', 'Те же пункты + страница «Журнал» (журнал правок, происхождение, отчёт и настройки аудита, эл. подпись, приборы)'),
    ('Показать графики (Ctrl+L), журнал правок, смещения бинов, приборы', 'Те же пункты + меню «Графики» и страница «Графики»'),
    ('Окно графиков: меню Файл, Правка, Вид, Графики, Инструменты, Аллели, Справка; 2 панели инструментов; панель правки', 'Без изменений состава; меню перенесено в одну строку с правой группой; все кнопки (в т.ч. красители) получили ID и настраиваются; «Вид → Фон графиков (это окно)»'),
    ('Менеджер панелей, Менеджер отчётов', 'Без изменений состава; та же оболочка (строка меню + правая группа, тема, палитра, настройка)'),
]:
    P(f'| {esc(a)} | {esc(b)} |')
P('')

P('## Тесты (ссылки в колонке «Тест»)\n')
P('Все проверки — `desktop/opentrace-desktop/tests/e2e/ui_shell.cjs` (Playwright, Chromium; встроенный пример или локальный набор FSA).\n')
for k, v in [('T1', 'реестр: команды окна зарегистрированы, у каждой активной команды меню есть обработчик'),
             ('T2', 'палитра команд: поиск, выполнение команды (Enter), закрытие Esc'),
             ('T3', 'скрытие через редактор «Настроить интерфейс…» / API: элемент не показывается, команда остаётся в реестре и палитре'),
             ('T4', 'изменение порядка и сохранение после перезагрузки окна'),
             ('T5', 'критичные элементы (статус, ⚙, строка состояния, «Стоп») не скрываются даже при записи их ID в раскладку'),
             ('T6', 'аварийный сброс Ctrl+Shift+F12 и откат повреждённой раскладки к заводской с сообщением'),
             ('T7', 'вкладки рабочих областей и страницы «Графики», «Стандарты», «Журнал»'),
             ('T8', 'темы (Системная, Светлая, Графитовая, Тёмная, Контрастная) и независимый фон графиков; генотипы не меняются')]:
    P(f'* **{k}** — {v}')
P('')

for w in ('main', 'plot', 'panels', 'report'):
    c = reg[w]['commands']; areas = reg[w]['areas']
    P(f'## {WIN[w]}\n')
    P('| ID | Название | Тип | Где было | Где сейчас | Клавиши | Состояние в снимке | Видимость настраивается | Тест |')
    P('|---|---|---|---|---|---|---|---|---|')
    order = {a['area']: i for i, a in enumerate(areas)}
    for x in sorted(c, key=lambda x: (order.get(x['area'], 999), next((i for i, a in enumerate(areas) if a['area'] == x['area'] and x['id'].split('/', 1)[1] in a['items']), 0))):
        cfg = 'нет (критичный)' if x['critical'] else 'да'
        st = 'недоступна (по состоянию)' if x['disabled'] else ('обработчик' if x['hasRun'] or x['kind'] in ('section', 'submenu') else 'индикатор')
        P(f"| `{esc(x['id'])}` | {esc(x['label'])} | {kind_ru(x)} | {origin(x)} | {esc(where(x))} | {esc(x.get('sc') or '')} | {st} | {cfg} | {test_of(x)} |")
    P('')

P('## Desktop-only элементы (нет в WEB-снимке)\n')
P('| ID | Название | Где | Видимость настраивается |')
P('|---|---|---|---|')
for i, n, wh, cfg in [('main.menu/server', 'Сервер', 'раздел меню', 'да'), ('main.menu.server/connection', 'Сервер организации…', 'Сервер', 'да'),
                      ('main.menu.server/standards', 'Лабораторные стандарты…', 'Сервер', 'да'), ('main.menu.server/provenance', 'Происхождение анализов…', 'Сервер', 'да'),
                      ('main.right/profile', 'Лабораторный профиль', 'правая группа', 'да'),
                      ('main.status/otSync', 'Синхронизация FSA', 'строка состояния (shim.js)', 'нет (критичный)'),
                      ('main.status/otWs', 'Проект: источник и блокировка', 'строка состояния (shim.js)', 'нет (критичный)')]:
    P(f'| `{i}` | {n} | {wh} | {cfg} |')
P('')

P('## Контекстные меню (не настраиваются: действие зависит от места щелчка)\n')
P('| Окно | Где | Пункты |')
P('|---|---|---|')
for w, wh, it in [
    ('Графики', 'ось X', 'Полный вид; Масштаб…; Частичный локус (маркер)'),
    ('Графики', 'ось Y', 'Полный вид; Масштаб…'),
    ('Графики', 'заголовок маркера', 'Переопределить GQ; PLI (частичный локус)'),
    ('Графики', 'заголовок образца', 'Переопределить CGQ'),
    ('Графики', 'пик / метка аллеля или артефакта', 'меню пика ID-X: добавить/удалить/переименовать метку, артефакт, Peak Raw Data и др. (`peakMenu`)'),
    ('Графики', 'пустое место панели', 'Удалить все метки образца'),
    ('Менеджер панелей', 'бин / область бинов', 'Добавить бин; Изменить бин; Удалить бин'),
]:
    P(f'| {w} | {wh} | {it} |')
P('')
P('## Горячие клавиши вне меню (обработчики `keydown`, не изменялись)\n')
P('* **Главное окно:** Delete — очистить ячейки; F1 — справка; Ctrl+F1/F2/F3 — Информация/Raw/EPT; Ctrl+] — полный вид Raw/EPT; '
  'Ctrl+Shift+1/2/3 — Образцы/Сводка/Генотипы; Ctrl+Shift+R — анализ всего; Ctrl+Shift+N — навигатор; Ctrl+Shift+Z — повтор; '
  'Ctrl+Z/I/U/Y/O/S/D/C/V/T/F/G/M/J/N/K… — как в пунктах меню.')
P('* **Окно графиков:** ←/→ — соседний образец/маркер; Delete — удалить выбранные метки; Esc — снять выделение/закрыть меню; Ctrl+1…5, Ctrl+0 — красители, Ctrl+6 — все; '
  'Ctrl+Shift+B/M/D/Y/G/N/H/V — бины, оси, подписи; Ctrl+Y/N — ось X в п.н./точках; Ctrl+Q/A/W — таблица; Ctrl+J — следующий маркер; Ctrl+] — полный вид.')
P('* **Менеджер панелей:** Delete, Esc, Ctrl+D, Ctrl+N/M/E/K/Z/C/V/B/H/G, Ctrl+Shift+N/B/M/E/X, Ctrl+].')
P('* **Менеджер отчётов:** F5, Delete, Ctrl+O/S/E/C/X/V/F/G/D/P/L/A.')
P('* **Оболочка (новые, во всех окнах):** Ctrl+Shift+P — палитра команд; Ctrl+Shift+F12 — восстановить интерфейс. '
  'Конфликтов с существующими сочетаниями нет (проверено по обработчикам выше; Ctrl+K уже занят «Добавить образцы» и «Индикатор маркера», поэтому палитра не на Ctrl+K). '
  'Перехват идёт в фазе захвата, поэтому Ctrl+Shift+P не доходит до Ctrl+P (печать) менеджера отчётов. '
  '**NOT VERIFIED:** поведение этих сочетаний в нативных WebView2 (Windows) и WebKitGTK (Linux).')
P('')
P('## Существующие неактивные пункты (не новые заглушки)\n')
P('`main.menu.edit/associateSamples`, `main.menu.analysis/analyzeMarkers`, `main.menu.admin/scRemoteShutdown` — были отключены в OpenTrace и до оболочки '
  '(функции ID-X, не реализованные в OpenTrace); оболочка их не включает и не имитирует.')
print('\n'.join(out))
