#!/usr/bin/env bash
# Первая установка OpenTrace: .env, каталоги данных, сборка образа, запуск, проверка.
# Повторный запуск безопасен: .env и данные в runtime/ сохраняются.
set -euo pipefail
cd "$(dirname "$0")/.."

command -v docker >/dev/null || { echo "Не найден docker. Установите Docker Engine." >&2; exit 1; }
docker compose version >/dev/null 2>&1 || { echo "Не найден Docker Compose v2 (docker compose)." >&2; exit 1; }

if [ ! -f .env ]; then
  cp .env.example .env
  chmod 600 .env
  echo "Создан .env"
fi
set_env() {  # set_env KEY VALUE — меняет или добавляет строку в .env
  if grep -q "^$1=" .env; then sed -i "s|^$1=.*|$1=$2|" .env; else echo "$1=$2" >> .env; fi
}
# Значения из окружения при запуске, например: WEB_BIND_IP=0.0.0.0 ALLOWED_HOSTS=192.168.1.16 ./scripts/install.sh
for key in WEB_BIND_IP WEB_PORT ALLOWED_HOSTS TZ; do
  [ -n "${!key:-}" ] && set_env "$key" "${!key}"
done
set_env OPENTRACE_UID "$(id -u)"
set_env OPENTRACE_GID "$(id -g)"

mkdir -p runtime/projects runtime/security runtime/backups
set -a; . ./.env; set +a

docker compose up -d --build

echo "Ожидание готовности..."
for _ in $(seq 1 60); do
  state=$(docker inspect -f '{{.State.Health.Status}}' opentrace 2>/dev/null || echo starting)
  [ "$state" = healthy ] && break
  sleep 2
done
if [ "${state:-}" != healthy ]; then
  echo "Контейнер не стал healthy. Журнал: docker compose logs --tail=100 opentrace" >&2
  exit 1
fi
host="${WEB_BIND_IP:-127.0.0.1}"; [ "$host" = 0.0.0.0 ] && host=localhost
echo "Готово: http://${host}:${WEB_PORT:-8765}/idx/"
