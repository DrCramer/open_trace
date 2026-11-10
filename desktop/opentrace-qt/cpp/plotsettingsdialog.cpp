#include "opentrace-qt/cpp/plotsettingsdialog.h"
#include "opentrace-qt/src/bridge.cxx.h"

#include <QButtonGroup>
#include <QCheckBox>
#include <QColorDialog>
#include <QComboBox>
#include <QDialogButtonBox>
#include <QDoubleSpinBox>
#include <QFileDialog>
#include <QFileInfo>
#include <QFormLayout>
#include <QGridLayout>
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
#include <QRadioButton>
#include <QRegularExpression>
#include <QSettings>
#include <QSpinBox>
#include <QTabWidget>
#include <QTableWidget>
#include <QToolButton>
#include <QVBoxLayout>

namespace ot {

static QString q(const rust::String &s) { return QString::fromUtf8(s.data(), static_cast<int>(s.size())); }
#define RSTR(x) rust::Str(x.constData(), static_cast<std::size_t>(x.size()))

QJsonObject parseJsonObject(const QString &text) { return QJsonDocument::fromJson(text.toUtf8()).object(); }
static QString jsonText(const QJsonObject &o) { return QString::fromUtf8(QJsonDocument(o).toJson(QJsonDocument::Compact)); }

// [id, header] as in plotsettings.rs
static const QList<QPair<QString, QString>> SAMPLE_HEADER{{"sampleFileName", "Файл образца"}, {"sampleName", "Имя образца"}, {"panel", "Панель"}, {"SOS", "SOS"}, {"SQ", "SQ"},
                                                          {"SSPK", "SSPK"}, {"MIX", "MIX"}, {"OMR", "OMR"}, {"CGQ", "CGQ"}};
static const QList<QPair<QString, QString>> GENO_HEADER{{"sampleFileName", "Файл образца"}, {"sampleName", "Имя образца"}, {"panel", "Панель"}, {"marker", "Маркер"},
                                                        {"AN", "AN"}, {"BIN", "BIN"}, {"PHR", "PHR"}, {"LPH", "LPH"}, {"MPH", "MPH"}, {"BD", "BD"}, {"SPK", "SPK"},
                                                        {"OS", "OS"}, {"OVL", "OVL"}, {"ACC", "ACC"}, {"CC", "CC"}, {"GQ", "GQ"}};
static const QList<QPair<QString, QString>> SIZING_COLS{{"sampleFileName", "Файл образца"}, {"marker", "Маркер"}, {"dyeSamplePeak", "Краситель/пик"}, {"minutes", "Минуты"},
                                                        {"size", "Размер"}, {"height", "Высота"}, {"area", "Площадь"}, {"dataPoint", "Точка данных"}, {"alleleCall", "Аллель"}};
static const QList<QPair<QString, QString>> LABEL_TYPES{{"none", "Нет"}, {"alleleCall", "Аллель"}, {"size", "Размер"}, {"height", "Высота"}, {"area", "Площадь"},
                                                        {"dataPoint", "Точка данных"}, {"aeReason", "Причина правки (AE)"}};
static const QList<QPair<QString, QString>> LABEL_KINDS{{"assigned", "Назначенный аллель"}, {"custom", "Пользовательский аллель"}, {"ladder", "Аллельная лестница"}, {"artifact", "Артефакт"}};
static const QStringList FONTS{"Serif", "SansSerif", "Dialog", "DialogInput", "Monospaced", "Arial", "Tahoma", "Verdana", "Times New Roman", "Courier New"};
static const QList<QPair<QString, QString>> DYES{{"blue", "синий"}, {"green", "зелёный"}, {"yellow", "жёлтый"}, {"red", "красный"}, {"purple", "фиолетовый"}, {"orange", "оранжевый"}};

// ------------------------------------------------------------------ editor
PlotSettingsEditor::PlotSettingsEditor(const Backend &backend, const QJsonObject &setting, const QString &previous, bool readOnly, QWidget *parent)
    : QDialog(parent), backend_(backend), s_(setting), previous_(previous), ro_(readOnly) {
    setWindowTitle(previous.isEmpty() ? QString::fromUtf8("Новая настройка графика") : QString::fromUtf8("Редактор настроек графика"));
    build();
    resize(760, 600);
}

// Checkboxes bound to a JSON object of booleans, with "show all / hide all".
static QGroupBox *checkGroup(const QString &title, const QList<QPair<QString, QString>> &rows, const QJsonObject &obj, bool ro, int columns,
                             QList<std::function<void(QJsonObject &)>> &out) {
    auto *box = new QGroupBox(title);
    auto *v = new QVBoxLayout(box);
    auto *grid = new QGridLayout;
    QList<QCheckBox *> checks;
    for (int k = 0; k < rows.size(); ++k) {
        auto *c = new QCheckBox(rows[k].second);
        c->setChecked(obj.value(rows[k].first).toBool(true));
        c->setEnabled(!ro);
        c->setObjectName(rows[k].first);
        grid->addWidget(c, k / columns, k % columns);
        checks << c;
    }
    v->addLayout(grid);
    auto *bar = new QHBoxLayout;
    for (bool on : {true, false}) {
        auto *b = new QPushButton(on ? QString::fromUtf8("Показать все") : QString::fromUtf8("Скрыть все"));
        b->setEnabled(!ro);
        QObject::connect(b, &QPushButton::clicked, box, [checks, on] { for (auto *c : checks) c->setChecked(on); });
        bar->addWidget(b);
    }
    bar->addStretch(1);
    v->addLayout(bar);
    out << [checks](QJsonObject &o) { for (auto *c : checks) o[c->objectName()] = c->isChecked(); };
    return box;
}

static QComboBox *fontCombo(const QString &cur, bool ro) {
    auto *c = new QComboBox;
    c->addItems(FONTS);
    if (!FONTS.contains(cur)) c->addItem(cur);
    c->setCurrentText(cur);
    c->setEnabled(!ro);
    return c;
}

void PlotSettingsEditor::build() {
    auto *root = new QVBoxLayout(this);
    tabs_ = new QTabWidget;
    root->addWidget(tabs_);
    QList<std::function<void(QJsonObject &)>> sh, gh, stc;
    QList<std::function<void()>> steps;

    // General
    {
        auto *w = new QWidget;
        auto *f = new QFormLayout(w);
        auto *name = new QLineEdit(s_.value("name").toString());
        name->setMaxLength(255);
        name->setReadOnly(ro_);
        auto *desc = new QLineEdit(s_.value("description").toString());
        desc->setReadOnly(ro_);
        f->addRow(QString::fromUtf8("Название:"), name);
        f->addRow(QString::fromUtf8("Описание:"), desc);
        f->addRow(QString::fromUtf8("Группа безопасности:"), new QLabel("GeneMapper ID-X Security Group"));
        tabs_->addTab(w, QString::fromUtf8("Общие"));
        steps << [this, name, desc] { s_["name"] = name->text(); s_["description"] = desc->text(); };
    }
    // Sample Header / Genotype Header
    {
        auto *w = new QWidget;
        auto *v = new QVBoxLayout(w);
        v->addWidget(checkGroup(QString::fromUtf8("Поля заголовка образца:"), SAMPLE_HEADER, s_.value("sampleHeader").toObject(), ro_, 3, sh));
        v->addStretch(1);
        tabs_->addTab(w, QString::fromUtf8("Заголовок образца"));
        auto *w2 = new QWidget;
        auto *v2 = new QVBoxLayout(w2);
        v2->addWidget(checkGroup(QString::fromUtf8("Поля заголовка генотипа:"), GENO_HEADER, s_.value("genotypeHeader").toObject(), ro_, 4, gh));
        v2->addWidget(new QLabel(QString::fromUtf8("Используется графиком генотипов.")));
        v2->addStretch(1);
        tabs_->addTab(w2, QString::fromUtf8("Заголовок генотипа"));
    }
    // Sizing Table
    {
        const QJsonObject st = s_.value("sizingTable").toObject();
        auto *w = new QWidget;
        auto *v = new QVBoxLayout(w);
        v->addWidget(checkGroup(QString::fromUtf8("Колонки:"), SIZING_COLS, st.value("cols").toObject(), ro_, 3, stc));
        auto *fb = new QGroupBox(QString::fromUtf8("Шрифт:"));
        auto *ff = new QFormLayout(fb);
        auto *font = fontCombo(st.value("font").toString("Dialog"), ro_);
        auto *size = new QSpinBox;
        size->setRange(1, 20);
        size->setValue(st.value("size").toInt(11));
        size->setEnabled(!ro_);
        ff->addRow(QString::fromUtf8("Шрифт:"), font);
        ff->addRow(QString::fromUtf8("Размер:"), size);
        v->addWidget(fb);
        v->addStretch(1);
        tabs_->addTab(w, QString::fromUtf8("Таблица размеров"));
        steps << [this, stc, font, size] {
            QJsonObject st = s_.value("sizingTable").toObject();
            QJsonObject cols = st.value("cols").toObject();
            for (auto &f : stc) f(cols);
            st["cols"] = cols;
            st["font"] = font->currentText();
            st["size"] = size->value();
            s_["sizingTable"] = st;
        };
    }
    // Labels
    {
        const QJsonObject L = s_.value("labels").toObject();
        auto *w = new QWidget;
        auto *v = new QVBoxLayout(w);
        auto *gb = new QGroupBox(QString::fromUtf8("Метки на графиках образцов и генотипов:"));
        auto *g = new QGridLayout(gb);
        for (int i = 0; i < 4; ++i) g->addWidget(new QLabel(QString::fromUtf8("Метка %1:").arg(i + 1)), 0, i + 1);
        QList<QPair<QString, QList<QComboBox *>>> combos;
        for (int r = 0; r < LABEL_KINDS.size(); ++r) {
            g->addWidget(new QLabel(LABEL_KINDS[r].second), r + 1, 0);
            const QJsonArray cur = L.value(LABEL_KINDS[r].first).toArray();
            QList<QComboBox *> row;
            for (int i = 0; i < 4; ++i) {
                auto *c = new QComboBox;
                for (const auto &t : LABEL_TYPES) c->addItem(t.second, t.first);
                c->setCurrentIndex(std::max(0, c->findData(cur.at(i).toString("none"))));
                c->setEnabled(!ro_);
                g->addWidget(c, r + 1, i + 1);
                row << c;
            }
            combos << qMakePair(LABEL_KINDS[r].first, row);
        }
        auto *note = new QLabel(QString::fromUtf8("Метки после «Нет» не отображаются."));
        g->addWidget(note, LABEL_KINDS.size() + 1, 0, 1, 5);
        v->addWidget(gb);
        auto *fb = new QGroupBox(QString::fromUtf8("Шрифт:"));
        auto *ff = new QFormLayout(fb);
        auto *font = fontCombo(L.value("font").toString("Serif"), ro_);
        auto *size = new QSpinBox;
        size->setRange(1, 20);
        size->setValue(L.value("size").toInt(10));
        size->setEnabled(!ro_);
        ff->addRow(QString::fromUtf8("Шрифт:"), font);
        ff->addRow(QString::fromUtf8("Размер:"), size);
        v->addWidget(fb);
        auto check = [this, &L, v](const char *key, const char *label) {
            auto *c = new QCheckBox(QString::fromUtf8(label));
            c->setChecked(L.value(key).toBool());
            c->setEnabled(!ro_);
            v->addWidget(c);
            return c;
        };
        auto *pqv = check("pqvTrigger", "Показывать пики-триггеры PQV (LPH, MPH, BD, OS)");
        auto *pre = check("prefixes", "Показывать префиксы типа данных");
        auto *et = check("editType", "Показывать тип правки");
        auto *vb = check("virtualBlack", "Метки виртуальных аллелей чёрным");
        auto *color = new QComboBox;
        color->addItem(QString::fromUtf8("Чёрно-белые"), "bw");
        color->addItem(QString::fromUtf8("Цвет красителя — всё"), "dyeAll");
        color->addItem(QString::fromUtf8("Цвет красителя — рамка"), "dyeBorder");
        color->setCurrentIndex(std::max(0, color->findData(L.value("color").toString("bw"))));
        color->setEnabled(!ro_);
        auto *cf = new QFormLayout;
        cf->addRow(QString::fromUtf8("Цвет меток:"), color);
        v->addLayout(cf);
        v->addStretch(1);
        tabs_->addTab(w, QString::fromUtf8("Метки"));
        steps << [this, combos, font, size, pqv, pre, et, vb, color] {
            QJsonObject L = s_.value("labels").toObject();
            for (const auto &k : combos) {
                QJsonArray a;
                for (auto *c : k.second) a.append(c->currentData().toString());
                L[k.first] = a;
            }
            L["font"] = font->currentText();
            L["size"] = size->value();
            L["pqvTrigger"] = pqv->isChecked();
            L["prefixes"] = pre->isChecked();
            L["editType"] = et->isChecked();
            L["virtualBlack"] = vb->isChecked();
            L["color"] = color->currentData().toString();
            s_["labels"] = L;
        };
    }
    // Display Settings
    {
        auto D = std::make_shared<QJsonObject>(s_.value("display").toObject());
        auto *w = new QWidget;
        auto *v = new QVBoxLayout(w);
        auto radios = [this](QBoxLayout *lay, const QString &cur, const QList<QPair<QString, QString>> &opts) {
            auto *grp = new QButtonGroup(lay);
            for (int k = 0; k < opts.size(); ++k) {
                auto *r = new QRadioButton(opts[k].second);
                r->setChecked(cur == opts[k].first);
                r->setEnabled(!ro_);
                r->setProperty("value", opts[k].first);
                grp->addButton(r, k);
                lay->addWidget(r);
            }
            return grp;
        };
        auto value = [](QButtonGroup *g) { return g->checkedButton() ? g->checkedButton()->property("value").toString() : QString(); };
        auto *pl = new QHBoxLayout;
        pl->addWidget(new QLabel(QString::fromUtf8("При открытии окна графиков:")));
        auto *persist = radios(pl, D->value("persistence").toString("these"), {{"last", QString::fromUtf8("Использовать последние настройки этого графика")}, {"these", QString::fromUtf8("Использовать эти настройки")}});
        pl->addStretch(1);
        v->addLayout(pl);
        auto *panesRow = new QHBoxLayout;
        auto *panes = new QSpinBox;
        panes->setRange(1, 16);
        panes->setValue(D->value("panes").toInt(2));
        panes->setEnabled(!ro_);
        panesRow->addWidget(new QLabel(QString::fromUtf8("Панели:")));
        panesRow->addWidget(panes);
        panesRow->addStretch(1);
        v->addLayout(panesRow);
        auto *cols = new QHBoxLayout;
        auto *left = new QVBoxLayout, *right = new QVBoxLayout;
        cols->addLayout(left, 1);
        cols->addLayout(right, 1);
        v->addLayout(cols);
        auto *showBox = new QGroupBox(QString::fromUtf8("Показать / скрыть"));
        auto *sv = new QVBoxLayout(showBox);
        QList<QPair<QString, QCheckBox *>> shows;
        for (auto [k, label] : QList<QPair<QString, QString>>{{"header", "Заголовок графика"}, {"markerRange", "Диапазон маркеров"}, {"markerIndicator", "Индикаторы маркеров"},
                                                             {"bins", "Бины"}, {"peakPositions", "Положения пиков"}, {"alleleChanges", "Изменения аллелей"}, {"toolbar", "Панель инструментов"},
                                                             {"offscale", "Данные за шкалой"}, {"controlsTop", "Контроли наверх"}, {"laddersTop", "Лестницы наверх"}}) {
            auto *c = new QCheckBox(label);
            c->setChecked(D->value(k).toBool());
            c->setEnabled(!ro_);
            sv->addWidget(c);
            shows << qMakePair(k, c);
        }
        left->addWidget(showBox);
        auto *labBox = new QGroupBox(QString::fromUtf8("Обозначения"));
        auto *lv = new QVBoxLayout(labBox);
        auto *sizeStd = new QCheckBox(QString::fromUtf8("Метки размерного стандарта"));
        sizeStd->setChecked(D->value("sizeStdLabels").toBool());
        sizeStd->setEnabled(!ro_);
        lv->addWidget(sizeStd);
        auto *labels = radios(lv, D->value("labels").toString("h"), {{"none", QString::fromUtf8("Без обозначений")}, {"h", QString::fromUtf8("Горизонтальные обозначения")}, {"v", QString::fromUtf8("Вертикальные обозначения")}});
        left->addWidget(labBox);
        left->addStretch(1);
        auto *axBox = new QGroupBox(QString::fromUtf8("Оси"));
        auto *av = new QVBoxLayout(axBox);
        auto *xr = new QHBoxLayout;
        xr->addWidget(new QLabel("X:"));
        auto *xAxis = radios(xr, D->value("xAxis").toString("bp"), {{"dp", QString::fromUtf8("Точки данных")}, {"bp", QString::fromUtf8("Пары оснований")}});
        xr->addStretch(1);
        av->addLayout(xr);
        auto *yr = new QHBoxLayout;
        yr->addWidget(new QLabel("Y:"));
        auto *yAxis = radios(yr, D->value("yAxis").toString("individual"), {{"individual", QString::fromUtf8("Масштабировать отдельно")}, {"max", QString::fromUtf8("По максимуму Y")}, {"entered", QString::fromUtf8("По введённому значению")}});
        auto *yValue = new QDoubleSpinBox;
        yValue->setRange(1, 1e7);
        yValue->setDecimals(0);
        yValue->setValue(D->value("yValue").toDouble(1000));
        yValue->setEnabled(!ro_);
        yr->addWidget(yValue);
        av->addLayout(yr);
        right->addWidget(axBox);
        auto *spBox = new QGroupBox(QString::fromUtf8("Только для графика образцов:"));
        auto *spv = new QVBoxLayout(spBox);
        auto *dyesRow = new QHBoxLayout;
        dyesRow->addWidget(new QLabel(QString::fromUtf8("Красители")));
        QList<QPair<QString, QCheckBox *>> dyes;
        const QJsonObject dyeObj = D->value("dyes").toObject();
        for (const auto &d : DYES) {
            auto *c = new QCheckBox(d.second);
            c->setChecked(dyeObj.value(d.first).toBool(true));
            c->setEnabled(!ro_);
            dyesRow->addWidget(c);
            dyes << qMakePair(d.first, c);
        }
        dyesRow->addStretch(1);
        spv->addLayout(dyesRow);
        auto *allDyes = new QCheckBox(QString::fromUtf8("Все красители (весь диапазон)"));
        allDyes->setChecked(D->value("allDyes").toBool(true));
        allDyes->setEnabled(!ro_);
        spv->addWidget(allDyes);
        auto *rangeRow = new QHBoxLayout;
        auto *start = new QDoubleSpinBox, *end = new QDoubleSpinBox;
        for (auto *sb : {start, end}) { sb->setRange(0, 10000); sb->setDecimals(1); sb->setEnabled(!ro_); }
        start->setValue(D->value("start").toDouble(0));
        end->setValue(D->value("end").toDouble(500));
        rangeRow->addWidget(new QLabel(QString::fromUtf8("Диапазон (п.н.): начало")));
        rangeRow->addWidget(start);
        rangeRow->addWidget(new QLabel(QString::fromUtf8("конец")));
        rangeRow->addWidget(end);
        rangeRow->addStretch(1);
        spv->addLayout(rangeRow);
        auto connectRange = [allDyes, start, end, this] { start->setEnabled(!ro_ && !allDyes->isChecked()); end->setEnabled(!ro_ && !allDyes->isChecked()); };
        connect(allDyes, &QCheckBox::toggled, this, connectRange);
        connectRange();
        auto *sf = new QFormLayout;
        auto *table = new QComboBox;
        for (auto [k, label] : QList<QPair<QString, QString>>{{"none", "Без таблицы"}, {"sizing", "Таблица размеров"}, {"genotypes", "Таблица генотипов"}, {"lev", "Журнал правок обозначений"}})
            table->addItem(label, k);
        table->setCurrentIndex(std::max(0, table->findData(D->value("table").toString("genotypes"))));
        table->setEnabled(!ro_);
        auto *layout = new QComboBox;
        for (auto [k, label] : QList<QPair<QString, QString>>{{"combine", "Объединить красители"}, {"separate", "Разделить красители"}, {"overlay", "Наложить все"}}) layout->addItem(label, k);
        layout->setCurrentIndex(std::max(0, layout->findData(D->value("layout").toString("separate"))));
        layout->setEnabled(!ro_);
        sf->addRow(QString::fromUtf8("Таблица"), table);
        sf->addRow(QString::fromUtf8("Раскладка красителей"), layout);
        spv->addLayout(sf);
        auto *qvd = new QCheckBox(QString::fromUtf8("Печатать сведения о качестве (QVD)"));
        qvd->setChecked(D->value("printQVD").toBool());
        qvd->setEnabled(!ro_);
        spv->addWidget(qvd);
        auto *btnRow = new QHBoxLayout;
        auto *colorsBtn = new QPushButton(QString::fromUtf8("Свои цвета..."));
        auto *scalesBtn = new QPushButton(QString::fromUtf8("Масштабы красителей результатов..."));
        colorsBtn->setEnabled(!ro_);
        scalesBtn->setEnabled(!ro_);
        btnRow->addWidget(colorsBtn);
        btnRow->addWidget(scalesBtn);
        btnRow->addStretch(1);
        spv->addLayout(btnRow);
        right->addWidget(spBox);
        connect(colorsBtn, &QPushButton::clicked, this, [this, D] {
            // Custom colours of the traces 1..16 (overlay of samples)
            QDialog dlg(this);
            dlg.setWindowTitle(QString::fromUtf8("Свои цвета графика"));
            auto *lay = new QVBoxLayout(&dlg);
            lay->addWidget(new QLabel(QString::fromUtf8("Выберите цвета кривых электрофореграммы")));
            auto *grid = new QGridLayout;
            QJsonArray cols = D->value("customColors").toArray();
            QList<QToolButton *> btns;
            for (int i = 0; i < cols.size(); ++i) {
                auto *b = new QToolButton;
                b->setText(QString::number(i + 1));
                b->setProperty("color", cols[i].toString());
                b->setStyleSheet(QString("background:%1;color:white;min-width:36px").arg(cols[i].toString()));
                connect(b, &QToolButton::clicked, &dlg, [b, &dlg] {
                    const QColor c = QColorDialog::getColor(QColor(b->property("color").toString()), &dlg);
                    if (!c.isValid()) return;
                    b->setProperty("color", c.name());
                    b->setStyleSheet(QString("background:%1;color:white;min-width:36px").arg(c.name()));
                });
                grid->addWidget(b, i / 8, i % 8);
                btns << b;
            }
            lay->addLayout(grid);
            auto *bb = new QDialogButtonBox(QDialogButtonBox::Ok | QDialogButtonBox::Cancel);
            connect(bb, &QDialogButtonBox::accepted, &dlg, &QDialog::accept);
            connect(bb, &QDialogButtonBox::rejected, &dlg, &QDialog::reject);
            lay->addWidget(bb);
            if (dlg.exec() != QDialog::Accepted) return;
            QJsonArray out;
            for (auto *b : btns) out.append(b->property("color").toString());
            (*D)["customColors"] = out;
        });
        connect(scalesBtn, &QPushButton::clicked, this, [this, D] {
            QDialog dlg(this);
            dlg.setWindowTitle(QString::fromUtf8("Масштабы красителей результатов"));
            auto *lay = new QVBoxLayout(&dlg);
            auto *hint = new QLabel(QString::fromUtf8("Коэффициент отображения каждого красителя (от 0,1 до 100). 1,0 показывает данные без изменений."));
            hint->setWordWrap(true);
            lay->addWidget(hint);
            auto *f = new QFormLayout;
            QList<QPair<QString, QDoubleSpinBox *>> boxes;
            const QJsonObject sc = D->value("dyeScales").toObject();
            for (const auto &d : DYES) {
                auto *sb = new QDoubleSpinBox;
                sb->setRange(0.1, 100);
                sb->setDecimals(2);
                sb->setSingleStep(0.1);
                sb->setValue(sc.value(d.first).toDouble(1));
                f->addRow(d.second, sb);
                boxes << qMakePair(d.first, sb);
            }
            lay->addLayout(f);
            auto *bb = new QDialogButtonBox(QDialogButtonBox::Ok | QDialogButtonBox::Cancel);
            auto *factory = bb->addButton(QString::fromUtf8("Заводские настройки"), QDialogButtonBox::ResetRole);
            connect(factory, &QPushButton::clicked, &dlg, [boxes] { for (auto &b : boxes) b.second->setValue(1); });
            connect(bb, &QDialogButtonBox::accepted, &dlg, &QDialog::accept);
            connect(bb, &QDialogButtonBox::rejected, &dlg, &QDialog::reject);
            lay->addWidget(bb);
            if (dlg.exec() != QDialog::Accepted) return;
            QJsonObject out = sc;
            for (auto &b : boxes) out[b.first] = b.second->value();
            (*D)["dyeScales"] = out;
        });
        auto *gpBox = new QGroupBox(QString::fromUtf8("Только для графика генотипов:"));
        auto *gpl = new QHBoxLayout(gpBox);
        auto *margin = new QSpinBox;
        margin->setRange(0, 100);
        margin->setValue(D->value("markerMargin").toInt(5));
        margin->setEnabled(!ro_);
        gpl->addWidget(new QLabel(QString::fromUtf8("Поле маркера:")));
        gpl->addWidget(margin);
        gpl->addWidget(new QLabel(QString::fromUtf8("п.н.")));
        gpl->addStretch(1);
        right->addWidget(gpBox);
        right->addStretch(1);
        tabs_->addTab(w, QString::fromUtf8("Отображение"));
        steps << [this, D, value, persist, panes, shows, sizeStd, labels, xAxis, yAxis, yValue, dyes, allDyes, start, end, table, layout, qvd, margin] {
            QJsonObject d = *D;
            d["persistence"] = value(persist);
            d["panes"] = panes->value();
            for (const auto &s : shows) d[s.first] = s.second->isChecked();
            d["sizeStdLabels"] = sizeStd->isChecked();
            d["labels"] = value(labels);
            d["xAxis"] = value(xAxis);
            d["yAxis"] = value(yAxis);
            d["yValue"] = yValue->value();
            QJsonObject dy = d.value("dyes").toObject();
            for (const auto &x : dyes) dy[x.first] = x.second->isChecked();
            d["dyes"] = dy;
            d["allDyes"] = allDyes->isChecked();
            d["start"] = start->value();
            d["end"] = end->value();
            d["table"] = table->currentData().toString();
            d["layout"] = layout->currentData().toString();
            d["printQVD"] = qvd->isChecked();
            d["markerMargin"] = margin->value();
            s_["display"] = d;
        };
    }
    collect_ = [this, sh, gh, steps] {
        QJsonObject a = s_.value("sampleHeader").toObject(), b = s_.value("genotypeHeader").toObject();
        for (auto &f : sh) f(a);
        for (auto &f : gh) f(b);
        s_["sampleHeader"] = a;
        s_["genotypeHeader"] = b;
        for (auto &f : steps) f();
    };
    auto *bb = new QDialogButtonBox(ro_ ? QDialogButtonBox::Close : QDialogButtonBox::Ok | QDialogButtonBox::Cancel);
    connect(bb, &QDialogButtonBox::accepted, this, &QDialog::accept);
    connect(bb, &QDialogButtonBox::rejected, this, &QDialog::reject);
    root->addWidget(bb);
}

void PlotSettingsEditor::accept() {
    if (ro_) { QDialog::reject(); return; }
    collect_();
    const QString name = s_.value("name").toString().trimmed();
    if (name.isEmpty()) {
        QMessageBox::warning(this, "OpenTrace", QString::fromUtf8("Это новая настройка графика. Введите уникальное имя."));
        tabs_->setCurrentIndex(0);
        return;
    }
    s_["name"] = name;
    const QByteArray js = jsonText(s_).toUtf8(), prev = previous_.toUtf8();
    const QString err = q(backend_.plot_setting_put(RSTR(js), RSTR(prev)));
    if (!err.isEmpty()) { QMessageBox::warning(this, "OpenTrace", err); return; }
    saved_ = name;
    QDialog::accept();
}

// ------------------------------------------------------------------ manager (GeneMapper Manager > Plot Settings)
PlotSettingsManager::PlotSettingsManager(const Backend &backend, QWidget *parent) : QDialog(parent), backend_(backend) {
    setWindowTitle(QString::fromUtf8("Настройки графиков"));
    auto *lay = new QHBoxLayout(this);
    table_ = new QTableWidget;
    table_->setColumnCount(2);
    table_->setHorizontalHeaderLabels({QString::fromUtf8("Название"), QString::fromUtf8("Описание")});
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
    fill(q(backend_.plot_setting_active()));
    resize(720, 420);
}

void PlotSettingsManager::fill(const QString &select) {
    table_->setRowCount(0);
    int row = 0, sel = 0;
    for (const auto &n : backend_.plot_setting_names()) {
        const QString name = q(n);
        const QByteArray nb = name.toUtf8();
        const QJsonObject s = parseJsonObject(q(backend_.plot_setting_json(RSTR(nb))));
        table_->insertRow(row);
        table_->setItem(row, 0, new QTableWidgetItem(name));
        table_->setItem(row, 1, new QTableWidgetItem(s.value("description").toString()));
        if (name == select) sel = row;
        ++row;
    }
    table_->resizeColumnToContents(0);
    if (row) table_->selectRow(sel);
}

QString PlotSettingsManager::current() const {
    const int r = table_->currentRow();
    return r >= 0 && table_->item(r, 0) ? table_->item(r, 0)->text() : QString();
}

void PlotSettingsManager::edit(const QString &name) {
    if (name.isEmpty()) return;
    const QByteArray nb = name.toUtf8();
    PlotSettingsEditor ed(backend_, parseJsonObject(q(backend_.plot_setting_json(RSTR(nb)))), name, false, this);
    if (ed.exec() != QDialog::Accepted) return;
    fill(ed.savedName());
    if (onChanged) onChanged();
}

void PlotSettingsManager::create() {
    PlotSettingsEditor ed(backend_, parseJsonObject(q(backend_.plot_setting_new_json())), QString(), false, this);
    if (ed.exec() != QDialog::Accepted) return;
    fill(ed.savedName());
    if (onChanged) onChanged();
}

void PlotSettingsManager::saveAs() {
    const QString name = current();
    if (name.isEmpty()) return;
    bool ok = false;
    const QString n = QInputDialog::getText(this, QString::fromUtf8("Сохранить как"), QString::fromUtf8("Имя новой настройки:"), QLineEdit::Normal, name + QString::fromUtf8(" (копия)"), &ok).trimmed();
    if (!ok || n.isEmpty()) return;
    const QByteArray nb = name.toUtf8();
    QJsonObject s = parseJsonObject(q(backend_.plot_setting_json(RSTR(nb))));
    s["name"] = n;
    const QByteArray js = jsonText(s).toUtf8(), empty;
    const QString err = q(backend_.plot_setting_put(RSTR(js), RSTR(empty)));
    if (!err.isEmpty()) { QMessageBox::warning(this, "OpenTrace", err); return; }
    fill(n);
    if (onChanged) onChanged();
}

void PlotSettingsManager::remove() {
    const QString name = current();
    if (name.isEmpty()) return;
    if (QMessageBox::question(this, "OpenTrace", QString::fromUtf8("Удалить настройку графика %1?").arg(name)) != QMessageBox::Yes) return;
    const QByteArray nb = name.toUtf8();
    backend_.plot_setting_remove(RSTR(nb));
    fill(q(backend_.plot_setting_active()));
    if (onChanged) onChanged();
}

void PlotSettingsManager::importFile() {
    QSettings st;
    const QString path = QFileDialog::getOpenFileName(this, QString::fromUtf8("Импорт настройки графика"), st.value("dirs/plotSettings").toString(), QString::fromUtf8("Настройка графика (*.xml *.json);;Все файлы (*)"));
    if (path.isEmpty()) return;
    st.setValue("dirs/plotSettings", QFileInfo(path).absolutePath());
    const QByteArray pb = path.toUtf8();
    const QString js = q(backend_.plot_setting_import(RSTR(pb)));
    if (js.isEmpty()) { QMessageBox::warning(this, "OpenTrace", q(backend_.last_import_error())); return; }
    QJsonObject s = parseJsonObject(js);
    QString name = s.value("name").toString(), previous;
    const QByteArray nb = name.toUtf8();
    if (!q(backend_.plot_setting_json(RSTR(nb))).isEmpty()) {
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
    const QString err = q(backend_.plot_setting_put(RSTR(sj), RSTR(pv)));
    if (!err.isEmpty()) { QMessageBox::warning(this, "OpenTrace", err); return; }
    fill(name);
    if (onChanged) onChanged();
    QMessageBox::information(this, "OpenTrace", QString::fromUtf8("Настройка графика импортирована."));
}

void PlotSettingsManager::exportFile() {
    const QString name = current();
    if (name.isEmpty()) return;
    QSettings st;
    QString safe = name;
    safe.replace(QRegularExpression(R"([\\/:*?"<>|])"), "_");
    const QString path = QFileDialog::getSaveFileName(this, QString::fromUtf8("Экспорт настройки графика"), st.value("dirs/plotSettings").toString() + "/" + safe + ".xml", "XML (*.xml)");
    if (path.isEmpty()) return;
    st.setValue("dirs/plotSettings", QFileInfo(path).absolutePath());
    const QByteArray nb = name.toUtf8(), pb = path.toUtf8();
    const QString err = q(backend_.plot_setting_export(RSTR(nb), RSTR(pb)));
    if (!err.isEmpty()) QMessageBox::warning(this, "OpenTrace", err);
}

} // namespace ot
