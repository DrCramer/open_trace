# OpenTrace Server и Desktop на Linux

Основная серверная платформа — Ubuntu Server 22.04/24.04 x86_64. Прежний сервис `opentrace` (Docker Compose, `scripts/install.sh`) продолжает работать без изменений; OpenTrace Server — отдельный бинарник с API v1, который **также** раздаёт прежний веб-интерфейс `/idx/`. Если оба запущены на одном хосте, у них разные каталоги данных: WEB-проекты `runtime/projects` прежнего сервиса не видны OpenTrace Server, пока вы не перенесёте их (см. §5).

> Исследовательское ПО. Не валидировано для диагностики и судебно-экспертной работы.

## 1. Сборка

```bash
sudo apt-get install -y build-essential curl git
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
. "$HOME/.cargo/env"
git clone https://github.com/DrCramer/open_trace.git
cd open_trace
git checkout feature/desktop-server-platform
cargo build --release --locked --manifest-path server/opentrace-server/Cargo.toml
./server/opentrace-server/target/release/opentrace-server version
```

## 2. Пакет .deb (Ubuntu/Debian, рекомендуется)

```bash
cargo build --release --locked --manifest-path server/opentrace-server/Cargo.toml
sh server/opentrace-server/packaging/deb/build-deb.sh          # → server/opentrace-server/target/release/opentrace-server_<версия>_amd64.deb
sudo apt install ./server/opentrace-server/target/release/opentrace-server_*.deb
sudo editor /etc/opentrace/server.toml                          # allowed_hosts; transport "proxy" за HTTPS
sudo -u opentrace opentrace-server set-password Administrator --config /etc/opentrace/server.toml
sudo -u opentrace opentrace-server grant-approver "Lab Director" --config /etc/opentrace/server.toml
sudo systemctl enable --now opentrace-server
```

Пакет ставит `/usr/bin/opentrace-server`, ресурсы веб-интерфейса в `/usr/share/opentrace-server/resources`, конфигурацию `/etc/opentrace/server.toml` (conffile: ваши правки сохраняются при обновлении), unit `opentrace-server.service` (с ограничениями systemd: `ProtectSystem=strict`, запись только в каталоги данных и копий), системного пользователя `opentrace` и каталоги `/var/lib/opentrace-server`, `/var/backups/opentrace-server` (0750). Служба **не запускается сама** при первой установке — сначала пароль администратора. Обновление: `apt install ./новый.deb` (запущенная служба перезапускается). Удаление, в том числе `purge`, **не удаляет данные лаборатории** и пользователя `opentrace` — после финальной резервной копии удалите их сами.

Проверено (Ubuntu 24.04, контейнер без systemd): установка, `set-password`, запуск от `opentrace`, `/api/v1/health`, вход, `/idx/`; повторная установка сохраняет изменённую конфигурацию; `purge` оставляет данные; `systemd-analyze verify` для unit без замечаний. В CI (ubuntu-24.04) — то же при каждом запуске, пакет — артефакт `opentrace-server-Linux`.

## 2a. Ручная установка как службы systemd

```bash
sudo useradd --system --home /var/lib/opentrace-server --shell /usr/sbin/nologin opentrace
sudo install -m 0755 server/opentrace-server/target/release/opentrace-server /usr/local/bin/
sudo mkdir -p /usr/local/share/opentrace /etc/opentrace /var/lib/opentrace-server /var/backups/opentrace-server
sudo cp -r resources /usr/local/share/opentrace/
sudo install -m 0640 -o root -g opentrace server/opentrace-server/packaging/opentrace-server.example.toml /etc/opentrace/server.toml
sudo chown -R opentrace:opentrace /var/lib/opentrace-server /var/backups/opentrace-server
sudo editor /etc/opentrace/server.toml      # data_dir, allowed_hosts, transport
sudo -u opentrace OPENTRACE_RESOURCES=/usr/local/share/opentrace/resources \
     opentrace-server check-config --config /etc/opentrace/server.toml
```

Первый администратор (пароль читается со стандартного ввода; сервер должен быть остановлен — команда берёт блокировку каталога данных):

```bash
sudo -u opentrace OPENTRACE_RESOURCES=/usr/local/share/opentrace/resources \
     opentrace-server set-password Administrator --config /etc/opentrace/server.toml
sudo -u opentrace opentrace-server grant-approver "Lab Director" --config /etc/opentrace/server.toml   # кто утверждает стандарты
```

Остальных пользователей, их профили и группы создаёт администратор в веб-интерфейсе: «Администрирование → Менеджер безопасности» (тот же `SecurityStore`). Для обязательного входа в WEB включите там «Требовать вход».

```bash
sudo install -m 0644 server/opentrace-server/packaging/systemd/opentrace-server.service /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable --now opentrace-server
systemctl status opentrace-server
curl -fsS http://127.0.0.1:8770/api/v1/health
journalctl -u opentrace-server -n 50
```

## 3. HTTPS (рекомендуемый режим)

Сервер слушает `127.0.0.1:8770` (`transport = "proxy"`), TLS завершает обратный прокси. Пример Caddy — `server/opentrace-server/packaging/caddy/Caddyfile`:

```bash
sudo apt-get install -y caddy
sudo cp server/opentrace-server/packaging/caddy/Caddyfile /etc/caddy/Caddyfile
sudo editor /etc/caddy/Caddyfile   # имя хоста; tls internal или сертификат лабораторного CA
sudo systemctl reload caddy
```

С `tls internal` корневой сертификат Caddy (`/var/lib/caddy/.local/share/caddy/pki/authorities/local/root.crt`) нужно установить в доверенные на клиентах (Windows: `certlm.msc` → «Доверенные корневые центры сертификации»), либо указать путь к нему в десктопе («Сертификат CA (PEM)»). Добавьте публичное имя в `allowed_hosts`.

## 4. Явно разрешённый HTTP (только изолированная доверенная сеть)

```toml
transport = "http"
listen = "0.0.0.0:8770"
allow_insecure_http = true
```

Без `allow_insecure_http = true` сервер в таком режиме не стартует. Пароли, токены, генотипы и FSA передаются открыто.

## 5. Docker Compose (альтернатива systemd)

```bash
mkdir -p runtime/server
docker compose -f server/opentrace-server/packaging/docker/docker-compose.server.yml up -d --build
curl -fsS http://127.0.0.1:8770/api/v1/health
```

`set-password` требует остановленного сервера (одна запись в каталог безопасности): `docker compose … stop`, затем `docker compose … run --rm opentrace-server opentrace-server set-password Administrator`, затем `start`.

Перенос проектов прежнего сервиса: остановите оба сервиса и скопируйте `runtime/projects/*` в `<data_dir>/projects/`, `runtime/security/*` в `<data_dir>/security/` (пользователи и журнал аудита переносятся целиком). Ревизии и блокировки для перенесённых проектов создаются при первом обращении.

## 6. Резервное копирование и восстановление

```bash
sudo -u opentrace opentrace-server backup --config /etc/opentrace/server.toml            # работает при запущенном сервере
sudo -u opentrace opentrace-server verify-backup /var/backups/opentrace-server/opentrace-backup-20261102T120000
# восстановление — только в пустой каталог данных, сервер остановлен:
sudo systemctl stop opentrace-server
sudo -u opentrace OPENTRACE_SERVER_DATA_DIR=/var/lib/opentrace-server-restored \
     opentrace-server restore /var/backups/opentrace-server/opentrace-backup-20261102T120000 --config /etc/opentrace/server.toml
```

Пример ежедневного расписания: `sudo systemctl edit --force --full opentrace-backup.service` с `ExecStart=/usr/local/bin/opentrace-server backup --config /etc/opentrace/server.toml` и `User=opentrace`, плюс `opentrace-backup.timer` с `OnCalendar=daily`. Копируйте каталог резервной копии за пределы сервера защищённым способом: в нём исходные FSA и генотипы.

## 7. Обновление и откат

```bash
cd ~/open_trace && git fetch && git checkout <тег или коммит>
cargo build --release --locked --manifest-path server/opentrace-server/Cargo.toml
sudo -u opentrace opentrace-server backup --config /etc/opentrace/server.toml
sudo systemctl stop opentrace-server
sudo install -m 0755 server/opentrace-server/target/release/opentrace-server /usr/local/bin/
sudo cp -r resources /usr/local/share/opentrace/
sudo systemctl start opentrace-server     # миграции схемы применяются при старте
```

Откат на версию со старой схемой БД невозможен поверх мигрированной базы (сервер откажется стартовать с «более новой схемой»): восстановите резервную копию, сделанную перед обновлением, в пустой каталог.

Проверено на настоящих бинарниках (сервер `8015bc4`, схема 2 → `065e32a`, схема 3): данные старой версии (проект под блокировкой, утверждённый метод) после запуска новой версии читаются без изменений (ревизия, статус, SHA-256 содержимого), миграция 3 применилась при старте; старый бинарник на мигрированной базе отказывается стартовать с понятным сообщением; резервная копия старой версии проходит `verify-backup`, восстанавливается и запускается и старым (откат), и новым бинарником. Модульный тест `upgrade_from_schema_2_keeps_data` повторяет переход схемы при каждом `cargo test`.

## 8. OpenTrace Desktop на Linux

Сборка (нужны WebKitGTK 4.1 и Node.js ≥ 18 только для упаковщика):

```bash
sudo apt-get install -y libwebkit2gtk-4.1-dev libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev
cd desktop/opentrace-desktop
cargo build --release --manifest-path src-tauri/Cargo.toml       # бинарник src-tauri/target/release/opentrace-desktop
npm install && npx tauri build --bundles deb appimage            # пакеты в src-tauri/target/release/bundle/
sudo apt install ./src-tauri/target/release/bundle/deb/*.deb
```

Данные пользователя: `~/.local/share/org.opentrace.desktop/` (проекты, security, `fsa-cache`), настройки: `~/.config/org.opentrace.desktop/desktop.json`. Токен сервера хранится в ключнице ядра Linux (keyutils) до выхода из сеанса; после перезагрузки потребуется повторный вход.

Пакеты: `.deb` (≈6 МБ; ставит `/usr/bin/opentrace-desktop`, ресурсы в `/usr/lib/OpenTrace Desktop/resources`, ярлык меню; зависимости — WebKitGTK 4.1 и GTK 3) и AppImage (≈80 МБ, включает библиотеки; на системах без FUSE запуск `APPIMAGE_EXTRACT_AND_RUN=1 ./OpenTrace*.AppImage`). Удаление: `sudo apt remove open-trace-desktop` (данные пользователя остаются).

Проверка установленного пакета (самотест в реальном webview; на сервере без дисплея — через `xvfb-run`):

```bash
OPENTRACE_DESKTOP_DATA_DIR=$(mktemp -d) OPENTRACE_DESKTOP_SELFTEST=$PWD/selftest.json xvfb-run -a /usr/bin/opentrace-desktop
grep '"pass"' selftest.json   # "pass": true
```

Проверено (Ubuntu 24.04 x86_64, WebKitGTK 2.52): `.deb` и AppImage собраны, установлены и прошли самотест при скрытом дереве исходников — интерфейс и данные берутся из пакета.
