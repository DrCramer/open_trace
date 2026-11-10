# Сверка с веб-версией (Table Settings)

Тест `tablesettings::web_parity::same_as_web` сравнивает Rust-порт с кодом `resources/web/idx/tables.js`, выполненным
в Chromium (нужны Node.js и Playwright). Без переменных окружения тест пропускается (в CI не запускается).

```bash
D=$(mktemp -d); R=$(git rev-parse --show-toplevel); T=$R/desktop/opentrace-qt/tests/web_parity
python3 $T/ts_inputs.py $R $D
node $T/ts_web_pw.cjs $R/resources/web/idx/tables.js $D/ts_web.json $D/ts_xmls.json $D/ts_cases.json
OT_TS_WEB=$D/ts_web.json OT_TS_XMLS=$D/ts_xmls.json OT_TS_CASES=$D/ts_cases.json cargo test --manifest-path $R/desktop/opentrace-qt/Cargo.toml tablesettings
```
