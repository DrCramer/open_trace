# OpenTrace core (Rust)

Весь расчётный код, сервер и командная строка OpenTrace — один Rust-пакет `opentrace-core`
(лицензия MIT, зависимости: `serde_json`, `rusqlite`, `roxmltree`, `num-*`, `sha2`/`hmac`/`pbkdf2`, `flate2`, `zip`, `libc`).

- `src/` — библиотека: чтение ABIF, предобработка и SG-пики, калибровка и калибровочные модели, определение аллелей,
  лестницы и прогоны, контроль качества, проекты (SQLite), безопасность и журнал аудита, экспорт CODIS, смеси и профили,
  HTTP-сервер (`server.rs`) и CLI (`cli.rs`).
- `src/bin/opentrace.rs` — бинарник `opentrace` (`serve`, `demo`, `inspect`, `export`, `size`, `str`, `str-run`,
  `str-demo`, `str-run-demo`, `controls`, `marker-quality`, `pullup-quality`, `import-kit`, `project …`, `health`).
- `examples/*_dump.rs` — программы-«дампы», по которым сверялась каждая часть с прежней реализацией на Python.
- `tests/` — регрессионные тесты. Каталог `tests/golden` содержит запросы и ответы эталонной реализации на Python
  (алгоритмы, демо-фикстуры, HTTP-обмены, сессии CLI); тесты проверяют совпадение до байта.
- `../resources/` — веб-интерфейс (`web/`) и поставочные данные (`data/`); сервер ищет их через `OPENTRACE_RESOURCES`,
  рядом с бинарником (`resources/`) или в дереве исходников.

Эталон на Python удалён из дерева; его последняя версия — коммит `9c447fb32` (тег `python-reference-final`
в локальной копии). Числовые функции CPython воспроизведены явно (`lgamma`, `pow`, `sum`, `statistics.median`,
`float()`/`int()`/`%g`), чтобы результаты совпадали побитно.

Сборка и тесты:

    cargo build --release --manifest-path core/opentrace-core/Cargo.toml --bin opentrace
    cargo test --release --manifest-path core/opentrace-core/Cargo.toml
