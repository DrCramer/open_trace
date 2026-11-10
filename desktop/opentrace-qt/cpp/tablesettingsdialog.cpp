#include "opentrace-qt/cpp/tablesettingsdialog.h"
#include "opentrace-qt/cpp/plotsettingsdialog.h"
#include "opentrace-qt/src/bridge.cxx.h"

#include <QCheckBox>
#include <QComboBox>
#include <QDialogButtonBox>
#include <QFileDialog>
#include <QFileInfo>
#include <QFormLayout>
#include <QGroupBox>
#include <QHBoxLayout>
#include <QHeaderView>
#include <QInputDialog>
#include <QJsonArray>
#include <QJsonDocument>
#include <QLabel>
#include <QLineEdit>
#include <QMessageBox>
#include <QPushButton>
#include <QRegularExpression>
#include <QSettings>
#include <QSpinBox>
#include <QTabWidget>
#include <QTableWidget>
#include <QVBoxLayout>

namespace ot {

static QString q(const rust::String &s) { return QString::fromUtf8(s.data(), static_cast<int>(s.size())); }
#define RSTR(x) rust::Str(x.constData(), static_cast<std::size_t>(x.size()))
static QString jsonText(const QJsonObject &o) { return QString::fromUtf8(QJsonDocument(o).toJson(QJsonDocument::Compact)); }

TableSettingEditor::TableSettingEditor(const Backend &backend, const QJsonObject &setting, const QString &previous, QWidget *parent)
    : QDialog(parent), backend_(backend), s_(setting), previous_(previous) {
    setWindowTitle(QString::fromUtf8("Редактор настройки таблицы"));
    auto *lay = new QVBoxLayout(this);
    auto *form = new QFormLayout;
    name_ = new QLineEdit(s_.value("name").toString());
    name_->setObjectName("tsName");
    name_->setMaxLength(240);
    form->addRow(QString::fromUtf8("Название:"), name_);
    lay->addLayout(form);
    tabs_ = new QTabWidget;
    tabs_->addTab(page("samples"), QString::fromUtf8("Образцы"));
    tabs_->addTab(page("genotypes"), QString::fromUtf8("Генотипы"));
    lay->addWidget(tabs_, 1);
    auto *bb = new QDialogButtonBox(QDialogButtonBox::Ok | QDialogButtonBox::Cancel);
    bb->button(QDialogButtonBox::Cancel)->setText(QString::fromUtf8("Отмена"));
    connect(bb, &QDialogButtonBox::accepted, this, &TableSettingEditor::accept);
    connect(bb, &QDialogButtonBox::rejected, this, &QDialog::reject);
    lay->addWidget(bb);
    resize(760, 680);
}

QWidget *TableSettingEditor::page(const QString &kind) {
    const QByteArray kb = kind.toUtf8();
    const QJsonArray cat = QJsonDocument::fromJson(q(backend_.table_setting_catalog(RSTR(kb))).toUtf8()).array();
    const QJsonObject t = s_.value(kind).toObject();
    QSet<QString> vis;
    for (const auto &v : t.value("visible").toArray()) vis << v.toString();
    const QJsonObject filters = t.value("filters").toObject();

    auto *w = new QWidget;
    auto *lay = new QVBoxLayout(w);
    auto *colBox = new QGroupBox(QString::fromUtf8("Показываемые столбцы"));
    auto *cl = new QVBoxLayout(colBox);
    auto *grid = new QTableWidget(cat.size(), 4);
    grid->setObjectName("tsGrid_" + kind);
    grid->setHorizontalHeaderLabels({QString::fromUtf8("Показать"), QString::fromUtf8("Столбец"), QString::fromUtf8("Фильтрация"), QString::fromUtf8("Содержимое")});
    grid->verticalHeader()->hide();
    grid->setSelectionBehavior(QAbstractItemView::SelectRows);
    grid->setSelectionMode(QAbstractItemView::ExtendedSelection);
    grid->setEditTriggers(QAbstractItemView::NoEditTriggers);
    grid->horizontalHeader()->setStretchLastSection(true);
    grid->verticalHeader()->setDefaultSectionSize(grid->fontMetrics().height() + 12);
    QList<QComboBox *> ops;
    QList<QLineEdit *> texts;
    for (int i = 0; i < cat.size(); ++i) {
        const QJsonObject c = cat[i].toObject();
        const QString id = c.value("id").toString(), head = c.value("head").toString(), title = c.value("title").toString();
        auto *chk = new QTableWidgetItem;
        chk->setFlags(Qt::ItemIsEnabled | Qt::ItemIsSelectable | Qt::ItemIsUserCheckable);
        chk->setCheckState(vis.contains(id) ? Qt::Checked : Qt::Unchecked);
        chk->setData(Qt::UserRole, id);
        grid->setItem(i, 0, chk);
        auto *nameItem = new QTableWidgetItem(title != head ? head + QString::fromUtf8(" — ") + title : head);
        nameItem->setToolTip(title);
        grid->setItem(i, 1, nameItem);
        const QJsonObject f = filters.value(id).toObject();
        auto *op = new QComboBox;
        op->addItem(QString::fromUtf8("Показать все записи"), 0);
        for (const auto &o : c.value("options").toArray()) op->addItem(o.toObject().value("label").toString(), o.toObject().value("op").toInt());
        op->setCurrentIndex(std::max(0, op->findData(f.value("op").toInt())));
        auto *text = new QLineEdit(f.value("text").toString());
        text->setMaxLength(200);
        const bool contains = c.value("contains").toBool();
        text->setEnabled(contains && op->currentIndex() > 0);
        connect(op, &QComboBox::currentIndexChanged, text, [text, contains](int k) { text->setEnabled(contains && k > 0); });
        grid->setCellWidget(i, 2, op);
        grid->setCellWidget(i, 3, text);
        ops << op;
        texts << text;
    }
    grid->resizeColumnToContents(0);
    grid->setColumnWidth(1, 300);
    grid->setColumnWidth(2, 190);
    cl->addWidget(grid, 1);
    // Show / Hide of the selected rows, Show All / Hide All (ID-X editor buttons)
    auto *bl = new QHBoxLayout;
    auto setVis = [grid](bool all, bool on) {
        for (int i = 0; i < grid->rowCount(); ++i)
            if (all || grid->item(i, 0)->isSelected()) grid->item(i, 0)->setCheckState(on ? Qt::Checked : Qt::Unchecked);
    };
    auto btn = [&](const char *label, bool all, bool on) {
        auto *b = new QPushButton(QString::fromUtf8(label));
        b->setAutoDefault(false);
        connect(b, &QPushButton::clicked, grid, [setVis, all, on] { setVis(all, on); });
        bl->addWidget(b);
    };
    btn("Показать", false, true);
    btn("Скрыть", false, false);
    btn("Показать все", true, true);
    btn("Скрыть все", true, false);
    bl->addStretch(1);
    cl->addLayout(bl);
    lay->addWidget(colBox, 1);

    auto *sortBox = new QGroupBox(QString::fromUtf8("Сортировка"));
    auto *sf = new QFormLayout(sortBox);
    const QJsonArray sort = t.value("sort").toArray();
    QList<QComboBox *> sortCols, sortDirs;
    for (int i = 0; i < 3; ++i) {
        auto *col = new QComboBox;
        col->addItem(QString::fromUtf8("Нет"), QString());
        for (const auto &c : cat) col->addItem(c.toObject().value("head").toString(), c.toObject().value("id").toString());
        auto *dir = new QComboBox;
        dir->addItem(QString::fromUtf8("По возрастанию"));
        dir->addItem(QString::fromUtf8("По убыванию"));
        if (i < sort.size()) {
            col->setCurrentIndex(std::max(0, col->findData(sort[i].toObject().value("col").toString())));
            dir->setCurrentIndex(sort[i].toObject().value("desc").toBool() ? 1 : 0);
        }
        auto *h = new QHBoxLayout;
        h->addWidget(col, 1);
        h->addWidget(dir);
        sf->addRow(QString::fromUtf8(i ? "Затем по" : "Сортировать по"), h);
        sortCols << col;
        sortDirs << dir;
    }
    lay->addWidget(sortBox);

    QSpinBox *alleles = nullptr;
    QCheckBox *limit = nullptr, *only = nullptr;
    auto *of = new QFormLayout;
    if (kind == "genotypes") {
        alleles = new QSpinBox;
        alleles->setObjectName("tsAlleles");
        alleles->setRange(1, 20);
        alleles->setValue(t.value("numAlleles").toInt(2));
        of->addRow(QString::fromUtf8("Число показываемых аллелей:"), alleles);
        limit = new QCheckBox(QString::fromUtf8("Ограничить число показанных аллелей числом бинов"));
        limit->setChecked(t.value("limitToBins").toBool());
        only = new QCheckBox(QString::fromUtf8("Показывать только аллели с бином, в позиции своего бина"));
        only->setChecked(t.value("onlyBinned").toBool());
        of->addRow(limit);
        of->addRow(only);
    }
    auto *font = new QSpinBox;
    font->setRange(0, 24);
    font->setSpecialValueText(QString::fromUtf8("по умолчанию"));
    font->setToolTip(QString::fromUtf8("От 8 до 24; 0 — по умолчанию"));
    font->setValue(t.value("fontSize").toInt());
    of->addRow(QString::fromUtf8("Размер шрифта таблицы:"), font);
    lay->addLayout(of);

    collect_ << [=] {
        QJsonObject o = s_.value(kind).toObject();
        QJsonArray visible, sortKeys;
        QJsonObject fl;
        for (int i = 0; i < grid->rowCount(); ++i) {
            const QString id = grid->item(i, 0)->data(Qt::UserRole).toString();
            if (grid->item(i, 0)->checkState() == Qt::Checked) visible << id;
            const int op = ops[i]->currentData().toInt();
            if (!op) continue;
            QJsonObject f{{"op", op}};
            if (cat[i].toObject().value("contains").toBool()) f["text"] = texts[i]->text();
            fl[id] = f;
        }
        for (int i = 0; i < 3; ++i) {
            const QString c = sortCols[i]->currentData().toString();
            if (!c.isEmpty()) sortKeys << QJsonObject{{"col", c}, {"desc", sortDirs[i]->currentIndex() == 1}};
        }
        o["visible"] = visible;
        o["filters"] = fl;
        o["sort"] = sortKeys;
        const int fz = font->value();
        o["fontSize"] = fz >= 8 ? fz : 0;
        if (alleles) {
            o["numAlleles"] = alleles->value();
            o["limitToBins"] = limit->isChecked();
            o["onlyBinned"] = only->isChecked();
        }
        s_[kind] = o;
    };
    return w;
}

void TableSettingEditor::accept() {
    for (const auto &f : collect_) f();
    const QString name = name_->text().trimmed();
    if (name.isEmpty()) { QMessageBox::warning(this, "OpenTrace", QString::fromUtf8("Введите имя.")); name_->setFocus(); return; }
    s_["name"] = name;
    const QByteArray js = jsonText(s_).toUtf8(), prev = previous_.toUtf8();
    const QString err = q(backend_.table_setting_put(RSTR(js), RSTR(prev)));
    if (!err.isEmpty()) { QMessageBox::warning(this, "OpenTrace", err); return; }
    saved_ = name;
    QDialog::accept();
}

// ------------------------------------------------------------------ list
TableSettingsManager::TableSettingsManager(const Backend &backend, QWidget *parent) : QDialog(parent), backend_(backend) {
    setWindowTitle(QString::fromUtf8("Настройки таблиц"));
    auto *lay = new QHBoxLayout(this);
    table_ = new QTableWidget;
    table_->setColumnCount(3);
    table_->setHorizontalHeaderLabels({QString::fromUtf8("Название"), QString::fromUtf8("Столбцов «Образцы»"), QString::fromUtf8("Столбцов «Генотипы»")});
    table_->setSelectionBehavior(QAbstractItemView::SelectRows);
    table_->setSelectionMode(QAbstractItemView::SingleSelection);
    table_->setEditTriggers(QAbstractItemView::NoEditTriggers);
    table_->horizontalHeader()->setStretchLastSection(true);
    table_->verticalHeader()->hide();
    connect(table_, &QTableWidget::cellDoubleClicked, this, [this] { edit(current()); });
    lay->addWidget(table_, 1);
    auto *bv = new QVBoxLayout;
    auto add = [this, bv](const char *label, std::function<void()> f) {
        auto *b = new QPushButton(QString::fromUtf8(label));
        b->setAutoDefault(false);
        connect(b, &QPushButton::clicked, this, f);
        bv->addWidget(b);
    };
    add("Создать...", [this] { create(); });
    add("Открыть...", [this] { edit(current()); });
    add("Сохранить как...", [this] { saveAs(); });
    add("Импорт...", [this] { importFile(); });
    add("Экспорт...", [this] { exportFile(); });
    add("Удалить", [this] { remove(); });
    bv->addStretch(1);
    add("Закрыть", [this] { accept(); });
    lay->addLayout(bv);
    fill(q(backend_.table_setting_active()));
    resize(760, 440);
}

void TableSettingsManager::fill(const QString &select) {
    table_->setRowCount(0);
    int row = 0, sel = 0;
    for (const auto &n : backend_.table_setting_names()) {
        const QString name = q(n);
        const QByteArray nb = name.toUtf8();
        const QJsonObject s = parseJsonObject(q(backend_.table_setting_json(RSTR(nb))));
        table_->insertRow(row);
        table_->setItem(row, 0, new QTableWidgetItem(name));
        table_->setItem(row, 1, new QTableWidgetItem(QString::number(s.value("samples").toObject().value("visible").toArray().size())));
        table_->setItem(row, 2, new QTableWidgetItem(QString::number(s.value("genotypes").toObject().value("visible").toArray().size())));
        if (name == select) sel = row;
        ++row;
    }
    table_->resizeColumnToContents(0);
    if (row) table_->selectRow(sel);
}

QString TableSettingsManager::current() const {
    const int r = table_->currentRow();
    return r >= 0 && table_->item(r, 0) ? table_->item(r, 0)->text() : QString();
}

void TableSettingsManager::edit(const QString &name) {
    if (name.isEmpty()) return;
    const QByteArray nb = name.toUtf8();
    TableSettingEditor ed(backend_, parseJsonObject(q(backend_.table_setting_json(RSTR(nb)))), name, this);
    if (ed.exec() != QDialog::Accepted) return;
    fill(ed.savedName());
    if (onChanged) onChanged();
}

void TableSettingsManager::create() {
    TableSettingEditor ed(backend_, parseJsonObject(q(backend_.table_setting_new_json())), QString(), this);
    if (ed.exec() != QDialog::Accepted) return;
    fill(ed.savedName());
    if (onChanged) onChanged();
}

void TableSettingsManager::saveAs() {
    const QString name = current();
    if (name.isEmpty()) return;
    bool ok = false;
    const QString n = QInputDialog::getText(this, QString::fromUtf8("Сохранить как"), QString::fromUtf8("Имя новой настройки:"), QLineEdit::Normal, name + QString::fromUtf8(" (копия)"), &ok).trimmed();
    if (!ok || n.isEmpty()) return;
    const QByteArray nb = name.toUtf8();
    QJsonObject s = parseJsonObject(q(backend_.table_setting_json(RSTR(nb))));
    s["name"] = n;
    const QByteArray js = jsonText(s).toUtf8(), empty;
    const QString err = q(backend_.table_setting_put(RSTR(js), RSTR(empty)));
    if (!err.isEmpty()) { QMessageBox::warning(this, "OpenTrace", err); return; }
    fill(n);
    if (onChanged) onChanged();
}

void TableSettingsManager::remove() {
    const QString name = current();
    if (name.isEmpty()) return;
    if (QMessageBox::question(this, "OpenTrace", QString::fromUtf8("Удалить настройку таблицы %1?").arg(name)) != QMessageBox::Yes) return;
    const QByteArray nb = name.toUtf8();
    backend_.table_setting_remove(RSTR(nb));
    fill(q(backend_.table_setting_active()));
    if (onChanged) onChanged();
}

void TableSettingsManager::importFile() {
    QSettings st;
    const QString path = QFileDialog::getOpenFileName(this, QString::fromUtf8("Импорт настройки таблицы"), st.value("dirs/tableSettings").toString(), QString::fromUtf8("Настройка таблицы (*.xml *.json);;Все файлы (*)"));
    if (path.isEmpty()) return;
    st.setValue("dirs/tableSettings", QFileInfo(path).absolutePath());
    const QByteArray pb = path.toUtf8();
    const QString js = q(backend_.table_setting_import(RSTR(pb)));
    if (js.isEmpty()) { QMessageBox::warning(this, "OpenTrace", q(backend_.last_import_error())); return; }
    QJsonObject s = parseJsonObject(js);
    QString name = s.value("name").toString(), previous;
    const QByteArray nb = name.toUtf8();
    if (!q(backend_.table_setting_json(RSTR(nb))).isEmpty()) {
        // the name is taken: replace that setting or import under another name (never silently)
        QMessageBox box(QMessageBox::Question, "OpenTrace", QString::fromUtf8("Настройка «%1» уже есть.").arg(name), QMessageBox::NoButton, this);
        auto *rep = box.addButton(QString::fromUtf8("Заменить"), QMessageBox::AcceptRole);
        auto *ren = box.addButton(QString::fromUtf8("Импортировать под другим именем..."), QMessageBox::ActionRole);
        box.addButton(QMessageBox::Cancel);
        box.exec();
        if (box.clickedButton() == rep) previous = name;
        else if (box.clickedButton() == ren) {
            bool ok = false;
            name = QInputDialog::getText(this, QString::fromUtf8("Импорт"), QString::fromUtf8("Имя настройки:"), QLineEdit::Normal, name + " (2)", &ok).trimmed();
            if (!ok || name.isEmpty()) return;
            s["name"] = name;
        } else return;
    }
    const QByteArray sj = jsonText(s).toUtf8(), pv = previous.toUtf8();
    const QString err = q(backend_.table_setting_put(RSTR(sj), RSTR(pv)));
    if (!err.isEmpty()) { QMessageBox::warning(this, "OpenTrace", err); return; }
    fill(name);
    if (onChanged) onChanged();
    QMessageBox::information(this, "OpenTrace", QString::fromUtf8("Настройка таблицы импортирована."));
}

void TableSettingsManager::exportFile() {
    const QString name = current();
    if (name.isEmpty()) return;
    QSettings st;
    QString safe = name;
    safe.replace(QRegularExpression(R"([\\/:*?"<>|])"), "_");
    const QString path = QFileDialog::getSaveFileName(this, QString::fromUtf8("Экспорт настройки таблицы"), st.value("dirs/tableSettings").toString() + "/" + safe + ".xml", "XML (*.xml)");
    if (path.isEmpty()) return;
    st.setValue("dirs/tableSettings", QFileInfo(path).absolutePath());
    const QByteArray nb = name.toUtf8(), pb = path.toUtf8();
    const QString err = q(backend_.table_setting_export(RSTR(nb), RSTR(pb)));
    if (!err.isEmpty()) QMessageBox::warning(this, "OpenTrace", err);
}

} // namespace ot
