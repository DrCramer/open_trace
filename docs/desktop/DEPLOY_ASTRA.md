# OpenTrace Qt для Astra Linux 1.7.x

> Исследовательское ПО: не валидировано для диагностического и судебно-экспертного применения.

Нативный OpenTrace Qt (Rust + CXX-Qt + Qt 6 Widgets) собирается отдельно для Astra Linux Special Edition 1.7.x
(glibc 2.28) и ставится **без Docker, без Qt6 из репозиториев и без замены системных библиотек**.
Сборка: workflow `.github/workflows/native-qt-astra.yml`, артефакт `opentrace-qt-astra-1.7-x86_64`
(действующие артефакты `opentrace-qt-linux` и `opentrace-qt-windows` не затронуты).

## Что проверено (CI-прогон native-qt-astra #42877017056, коммит 3660127, зелёный)

| Проверка | Статус |
|---|---|
| Сборка в окружении с glibc 2.28 (AlmaLinux 8, образ manylinux_2_28) |выполнено в CI |
| Ни один ELF-файл пакета не требует `GLIBC_` новее 2.28 (`check_abi.py`) |выполнено в CI |
| Самотест (6 выдуманных FSA → анализ → генотипы → окно графиков → PDF → проект → правка/отмена) на чистом Debian 10 (glibc 2.28) с `offscreen` и с X-сервером Xvfb (`xcb`) |выполнено в CI |
| Установка/запуск/удаление DEB на Debian 10 |выполнено в CI |
| Путь с кириллицей, каталог приложения только для чтения |выполнено в CI |
| **Реальная Astra Linux 1.7.9.41, Fly Desktop (X11), печать, системные диалоги, масштаб/DPI, сетевые каталоги** | **не проверено — ждёт ручной проверки** |

Debian 10 имеет ту же glibc 2.28, но это не Astra: результаты CI не заменяют проверку на реальной системе.
Факты прогона: Qt 6.8.3; 49 ELF-файлов, максимальный требуемый `GLIBC_` = 2.28, `GLIBCXX_3.4.21` (поставляется libstdc++ до 3.4.25);
в `lib/` 40 библиотек (Qt6 Core/Gui/Widgets/PrintSupport/XcbQpa, xcb-*, xkbcommon(-x11), libstdc++, libgcc_s и транзитивные
зависимости из AlmaLinux 8, в т.ч. цепочка libcups: gcrypt/nettle/p11-kit/mount и др.); системными остаются libc, X11,
fontconfig, freetype, cups, uuid, selinux, cap-ng, Xau. Тест Debian 10: «ALL PASSED» (120/120 маркеров, PDF, повторное
открытие проекта идентично, правка, 11 настроек графика, кириллический путь `/opt/тест`, Xvfb+xcb, DEB установка/удаление).
Размер: DEB 13,8 МБ, tar.gz 18,9 МБ. Результаты каждого прогона — в артефакте (`abi-report.txt`, `test/test-debian10.log`, `test/*/result.json`).

## Поддерживаемые системы

* Astra Linux Special Edition 1.7.x x86_64 (цель: 1.7.9.41) — основная.
* Astra Linux 1.8.x и другие дистрибутивы с glibc ≥ 2.28 и X11 — возможны, не проверялись.
* Графика: X11 (Fly Desktop). Аппаратное ускорение не нужно: интерфейс рисуется программно (Qt собран без OpenGL).

Системные требования: x86_64, 4 ГБ ОЗУ (рекомендуется 8+), ~250 МБ на диске, X-сеанс пользователя.
Из системных библиотек нужны те, что есть в любой Astra с графикой: `libx11-6 libxext6 libxrender1 libfontconfig1 libfreetype6`
(для печати — `libcups2`, для шрифтов — например `fonts-dejavu-core`). Qt 6, libstdc++ и библиотеки xcb/xkbcommon
идут внутри пакета и используются только этим приложением.

## Установка DEB

```bash
sudo apt install ./opentrace-qt-astra-1.7-amd64.deb     # или: sudo dpkg -i opentrace-qt-astra-1.7-amd64.deb
```

Файлы: `/opt/opentrace-qt/` (приложение, `lib/`, `plugins/`, `resources/`, `licenses/`), команда `opentrace-qt`
(`/usr/bin/opentrace-qt`), пункт меню «OpenTrace» (категории Наука/Графика), иконка. Системные Qt и glibc не затрагиваются.
Запуск: меню Fly или `opentrace-qt` из любого каталога; запуск от root не нужен.

Удаление: `sudo apt remove opentrace-qt`. Обновление: установить новый `.deb` поверх (`sudo apt install ./новый.deb`).
Данные пользователя при удалении сохраняются.

## Portable-версия

```bash
tar -xzf OpenTrace-Qt-AstraLinux-1.7-x86_64.tar.gz
cd OpenTrace-Qt && ./launch.sh
```

Работает из любого каталога без root. Каталог приложения может быть только для чтения.

## Ресурсы (`resources/web`, `resources/data`)

Приложение ищет их автоматически: `OPENTRACE_RESOURCES` (если задана), затем каталог `resources/` рядом с исполняемым
файлом. `launch.sh` сам задаёт `OPENTRACE_RESOURCES`, поэтому ошибка «OpenTrace resources (web/, data/) not found»
возникнуть не должна. Свой набор ресурсов: `OPENTRACE_RESOURCES=/путь opentrace-qt`.

## Данные пользователя

Настройки, библиотеки панелей/методов, настройки графиков — в `~/.local/share/OpenTrace/OpenTrace Desktop/`
(`library.json`), параметры окон — в `~/.config/OpenTrace/`. В каталог приложения ничего не пишется. Проекты (`.otproj`),
экспорт и PDF сохраняются туда, куда укажет пользователь.

## Диагностика Qt / xcb

```bash
/opt/opentrace-qt/launch.sh --diag      # папка, ресурсы, ненайденные библиотеки, QT_DEBUG_PLUGINS=1
```

| Симптом | Причина / действие |
|---|---|
| `could not connect to display` | нет X-сеанса: запускать из графической сессии пользователя (`DISPLAY` задаётся сессией, приложение его не фиксирует) |
| `Could not load the Qt platform plugin "xcb"` | см. `--diag`; обычно нет `libx11-6/libxext6/libxrender1/libfontconfig1/libfreetype6`: `sudo apt install ...` |
| `libfontconfig.so.1: cannot open shared object file` | `sudo apt install libfontconfig1 libfreetype6` |
| квадраты вместо букв | нет шрифтов: `sudo apt install fonts-dejavu-core` |
| окно не открывается удалённо (ssh -X) | `QT_XCB_NO_MITSHM=1 opentrace-qt` |
| список принтеров пуст | нет `libcups2`/службы CUPS; PDF сохраняется без принтера |
| `GLIBC_2.xx not found` | собран не этим конвейером: для Astra 1.7 использовать только артефакт `opentrace-qt-astra-1.7-x86_64` |

## Лицензии

Qt 6.8.3 (LGPL-3.0) поставляется без изменений динамическими библиотеками в `lib/` и `plugins/`; их можно заменить
собственной сборкой той же версии. Тексты лицензий, список поставляемых системных библиотек и Rust-крейтов —
`licenses/THIRD_PARTY_NOTICES.txt`, `licenses/Qt-LICENSES/`, `licenses/qt-build-info.txt` (параметры сборки Qt и адрес исходников).
glibc, загрузчик и библиотеки X11/fontconfig/freetype/OpenGL/CUPS не поставляются — используются системные.

## Как достигнута совместимость с glibc 2.28

* Приложение и Qt компилируются **внутри образа manylinux_2_28** (AlmaLinux 8, glibc 2.28): бинарники физически не могут
  потребовать более новых символов glibc, чем есть в этом окружении. Сборка на Ubuntu 24.04 не используется.
* Qt 6.8.3 (только `qtbase`) собирается из официальных исходников (`build-qt.sh`, md5 из `md5sums.txt` сервера Qt):
  `-no-opengl -no-dbus -no-glib -no-icu -no-openssl -qt-zlib -qt-libpng -qt-libjpeg -qt-harfbuzz -qt-pcre -system-freetype
  -fontconfig -xcb -xkbcommon -bundled-xcb-xinput -cups`.
* `package.py` кладёт в `lib/` Qt и транзитивные зависимости плагина xcb (xcb-*, xkbcommon, libstdc++ сборочного
  компилятора), кроме glibc и системных библиотек из `sysdeps.py`; `patchelf` задаёт `$ORIGIN`-пути поиска.
* `check_abi.py` проверяет **каждый** ELF (exe, `.so`, плагины): `GLIBC_` ≤ 2.28, `GLIBCXX/CXXABI/GCC` определены в
  поставляемой libstdc++, нет внешних путей поиска, нет зависимостей вне `lib/` и списка системных. Ошибка — ошибка CI.

## Локальная сборка (разработчикам)

Нужны Docker и доступ к `quay.io` и `download.qt.io` (после сборки Qt каталог `~/astra-qt` можно переиспользовать):

```bash
B=quay.io/pypa/manylinux_2_28_x86_64:latest; P=desktop/opentrace-qt/packaging/astra
mkdir -p ~/astra-qt ~/astra-cache
docker run --rm -v $PWD:/work -v ~/astra-qt:/opt/qt -v ~/astra-cache:/cache $B bash /work/$P/build-qt.sh        # один раз (ставит системные -devel через deps.sh)
docker run --rm -v $PWD:/work -v ~/astra-qt:/opt/qt -v ~/astra-cache:/cache $B bash -c "bash /work/$P/deps.sh && bash /work/$P/build-app.sh"
docker run --rm -v $PWD:/work -v ~/astra-qt:/opt/qt -v ~/astra-cache:/cache $B bash -c "bash /work/$P/deps.sh && . /work/$P/common.sh && cd /work && python3 /work/$P/package.py"
docker run --rm -v $PWD:/work $B /opt/python/cp312-cp312/bin/python3 /work/$P/check_abi.py /work/_astra/dist/OpenTrace-Qt
bash $P/make-packages.sh _astra/dist/OpenTrace-Qt _astra/out 0.1.0-local
bash $P/test-debian10.sh _astra/out _astra/out/test
```

## Ограничения

* Не проверено на реальной Astra 1.7.9.41 (см. таблицу): Fly Desktop, диалоги выбора файлов (используется диалог Qt, не
  системный), печать на принтер, DPI, сетевые каталоги — ждут ручной проверки.
* Режим повышенной защищённости Astra (замкнутая программная среда, мандатный контроль целостности) может потребовать
  подписи/разрешения для запуска из `/opt`; настройки защиты приложением не меняются.
* Qt собран без OpenGL: интерфейс программный (в проекте GPU не используется).
