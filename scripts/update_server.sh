#!/usr/bin/env bash
# Обновление: резервная копия данных -> git pull --ff-only -> пересборка -> проверка.
# .env и runtime/ не затрагиваются.
set -euo pipefail
cd "$(dirname "$0")/.."

[ -f .env ] || { echo "Нет .env — сначала выполните ./scripts/install.sh" >&2; exit 1; }
if [ -n "$(git status --porcelain --untracked-files=no)" ]; then
  echo "Есть локальные изменения, обновление остановлено:" >&2
  git status --short >&2
  exit 1
fi

mkdir -p runtime/backups
backup="runtime/backups/opentrace_$(date -u +%Y%m%d_%H%M%S).tar.gz"
tar -czf "$backup" -C runtime projects security
echo "Резервная копия: $backup"

branch=$(git branch --show-current)
git pull --ff-only origin "$branch"

./scripts/install.sh
docker image prune -f >/dev/null
docker compose ps
