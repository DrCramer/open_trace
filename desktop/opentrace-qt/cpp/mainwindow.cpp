#include "opentrace-qt/cpp/mainwindow.h"
#include "opentrace-qt/cpp/plotwindow.h"
#include "opentrace-qt/cpp/plotsettingsdialog.h"
#include "opentrace-qt/cpp/tablesettingsdialog.h"
#include "opentrace-qt/cpp/samplemodel.h"
#include "opentrace-qt/cpp/style.h"
#include "opentrace-qt/cpp/simpleplot.h"
#include "opentrace-qt/src/bridge.cxx.h"

#include <QAction>
#include <QApplication>
#include <QCloseEvent>
#include <QComboBox>
#include <QDialog>
#include <QDialogButtonBox>
#include <QFile>
#include <QFileDialog>
#include <QFileInfo>
#include <QDir>
#include <QFormLayout>
#include <QHeaderView>
#include <QKeySequence>
#include <QLabel>
#include <QLineEdit>
#include <QMenuBar>
#include <QMessageBox>
#include <QPlainTextEdit>
#include <QProgressBar>
#include <QPushButton>
#include <QSettings>
#include <QSortFilterProxyModel>
#include <QSplitter>
#include <QStatusBar>
#include <QStyle>
#include <QTabWidget>
#include <QTableView>
#include <QTextStream>
#include <QTimer>
#include <QRegularExpression>
#include <QEventLoop>
#include <QToolBar>
#include <QTreeWidget>
#include <QSet>
#include <QScrollArea>
#include <QInputDialog>
#include <QTableWidget>
#include <QRadioButton>
#include <QButtonGroup>
#include <QGroupBox>
#include <QCheckBox>
#include <QDoubleSpinBox>
#include <QVBoxLayout>
#include <QHBoxLayout>
#include <QJsonDocument>
#include <QJsonArray>
#include <QJsonObject>
#include <QStandardPaths>

namespace ot {

static QString q(const rust::String &s) { return QString::fromUtf8(s.data(), static_cast<int>(s.size())); }
static rust::String r(const QString &s) { const QByteArray b = s.toUtf8(); return rust::String(b.constData(), static_cast<std::size_t>(b.size())); }

MainWindow::MainWindow(QWidget *parent) : QMainWindow(parent), backend_(new_backend()) {
    setWindowTitle(QString::fromUtf8("OpenTrace Desktop — без имени"));
    backend_->set_data_dir(r(QStandardPaths::writableLocation(QStandardPaths::AppDataLocation)));
    model_ = new SampleModel(*backend_, this);
    proxy_ = new RowFilterProxy(this);
    proxy_->setSourceModel(model_);
    table_ = new QTableView;
    table_->setModel(proxy_);
    table_->setSelectionBehavior(QAbstractItemView::SelectRows);
    table_->setSelectionMode(QAbstractItemView::ExtendedSelection);
    // the rows follow the sort keys of the table setting until a column header is clicked (connected before the view's
    // own handler, so the proxy already compares by the clicked column when the view sorts)
    connect(table_->horizontalHeader(), &QHeaderView::sortIndicatorChanged, this, [this] {
        proxy_->setSettingOrder(false);
        table_->horizontalHeader()->setSortIndicatorShown(true);
    });
    table_->setSortingEnabled(true);
    table_->setAlternatingRowColors(true);
    table_->verticalHeader()->setDefaultSectionSize(table_->fontMetrics().height() + 6);
    table_->horizontalHeader()->setStretchLastSection(true);
    table_->setEditTriggers(QAbstractItemView::NoEditTriggers);
    connect(table_, &QTableView::doubleClicked, this, [this] { showPlots(); });
    connect(table_->selectionModel(), &QItemSelectionModel::selectionChanged, this, [this] { updateInfo(); refreshSampleViews(); });

    nav_ = new QTreeWidget;
    nav_->setHeaderHidden(true);
    nav_->setMinimumWidth(180);
    info_ = new QPlainTextEdit;
    info_->setReadOnly(true);
    tabs_ = new QTabWidget;
    tabs_->setDocumentMode(true);
    auto *samplesPage = new QWidget;
    {
        auto *v = new QVBoxLayout(samplesPage);
        v->setContentsMargins(0, 0, 0, 0);
        v->setSpacing(0);
        filterBar_ = new QWidget;
        auto *h = new QHBoxLayout(filterBar_);
        h->setContentsMargins(6, 3, 6, 3);
        filterText_ = new QLabel;
        auto *clear = new QPushButton(QString::fromUtf8("Показать все образцы"));
        connect(clear, &QPushButton::clicked, this, [this] { filterRows({}, QString()); });
        h->addWidget(filterText_, 1);
        h->addWidget(clear);
        filterBar_->setStyleSheet("background:#fff3cd");
        filterBar_->hide();
        v->addWidget(filterBar_);
        v->addWidget(table_, 1);
    }
    tabs_->addTab(samplesPage, QString::fromUtf8("Образцы"));
    // Analysis Summary
    auto *sumScroll = new QScrollArea;
    sumScroll->setWidgetResizable(true);
    summary_ = new QWidget;
    summaryLay_ = new QVBoxLayout(summary_);
    summaryLay_->setAlignment(Qt::AlignTop);
    sumScroll->setWidget(summary_);
    tabs_->addTab(sumScroll, QString::fromUtf8("Сводка анализа"));
    genoTable_ = new QTableWidget;
    genoTable_->setEditTriggers(QAbstractItemView::NoEditTriggers);
    genoTable_->setSelectionBehavior(QAbstractItemView::SelectRows);
    genoTable_->setAlternatingRowColors(true);
    genoTable_->verticalHeader()->setDefaultSectionSize(table_->fontMetrics().height() + 6);
    genoTable_->setSortingEnabled(true);
    tabs_->addTab(genoTable_, QString::fromUtf8("Генотипы"));
    tabs_->addTab(info_, QString::fromUtf8("Результат анализа"));
    // ID-X Sample Info / Raw Data / EPT Data of the selected sample, read from its file
    infoTree_ = new QTreeWidget;
    infoTree_->setColumnCount(2);
    infoTree_->setHeaderLabels({QString::fromUtf8("Параметр"), QString::fromUtf8("Значение")});
    infoTree_->setAlternatingRowColors(true);
    tabs_->addTab(infoTree_, QString::fromUtf8("Сведения о файле"));
    rawPage_ = new QWidget;
    {
        auto *v = new QVBoxLayout(rawPage_);
        rawTitle_ = new QLabel;
        rawLegend_ = new QWidget;
        new QHBoxLayout(rawLegend_);
        rawLegend_->layout()->setContentsMargins(0, 0, 0, 0);
        auto *top = new QHBoxLayout;
        top->addWidget(rawTitle_);
        top->addWidget(rawLegend_, 1);
        v->addLayout(top);
        rawPlot_ = new SimplePlot;
        v->addWidget(rawPlot_, 1);
        rawReadout_ = new QLabel(QString::fromUtf8("Протяжка — увеличить участок, двойной щелчок — полный вид"));
        rawPlot_->setReadout(rawReadout_);
        v->addWidget(rawReadout_);
    }
    tabs_->addTab(rawPage_, QString::fromUtf8("Необработанные данные"));
    eptPage_ = new QWidget;
    eptLay_ = new QVBoxLayout(eptPage_);
    tabs_->addTab(eptPage_, QString::fromUtf8("Данные EPT"));
    connect(tabs_, &QTabWidget::currentChanged, this, [this] { refreshSampleViews(); updateTableNote(); });
    connect(model_, &QAbstractItemModel::modelReset, this, [this] { updateTableNote(); });
    connect(model_, &QAbstractItemModel::dataChanged, this, [this] { updateTableNote(); });
    auto *split = new QSplitter;
    split->addWidget(nav_);
    split->addWidget(tabs_);
    split->setStretchFactor(1, 1);
    split->setSizes({210, 1100});
    setCentralWidget(split);

    status_ = new QLabel(QString::fromUtf8("Готово"));
    progress_ = new QProgressBar;
    progress_->setMaximumWidth(220);
    progress_->setTextVisible(true);
    progress_->setValue(0);
    stop_ = new QPushButton(QString::fromUtf8("Стоп"));
    stop_->setEnabled(false);
    connect(stop_, &QPushButton::clicked, this, [this] { backend_->stop_analysis(); });
    statusBar()->addWidget(status_, 1);
    statusBar()->addPermanentWidget(progress_);
    statusBar()->addPermanentWidget(stop_);

    poll_ = new QTimer(this);
    poll_->setInterval(150);
    connect(poll_, &QTimer::timeout, this, [this] { pollProgress(); });

    buildMenus();
    refreshNavigator();
    QSettings st;
    restoreGeometry(st.value("main/geometry").toByteArray());
    if (!st.contains("main/geometry")) resize(1360, 820);
    if (qEnvironmentVariableIsSet("OT_QT_AUTOTEST_FILES")) QTimer::singleShot(0, this, [this] { autotest(); });
    const QString problem = q(backend_->load_problem());
    if (!problem.isEmpty())
        QTimer::singleShot(200, this, [this, problem] {
            QMessageBox::warning(this, "OpenTrace", QString::fromUtf8("Ресурсы OpenTrace не найдены (методы анализа):\n") + problem);
        });
}

MainWindow::~MainWindow() = default;

void MainWindow::closeEvent(QCloseEvent *e) {
    if (backend_->analysis_progress().running) {
        if (QMessageBox::question(this, "OpenTrace", QString::fromUtf8("Идёт анализ. Остановить и закрыть?")) != QMessageBox::Yes) { e->ignore(); return; }
        backend_->stop_analysis();
    }
    if (!autotest_ && !confirmDiscard()) { e->ignore(); return; }
    QSettings().setValue("main/geometry", saveGeometry());
    e->accept();
}

void MainWindow::buildMenus() {
    QStyle *s = style();
    auto *file = menuBar()->addMenu(QString::fromUtf8("&Файл"));
    auto *aNew = file->addAction(s->standardIcon(QStyle::SP_FileIcon), QString::fromUtf8("&Новый проект"), this, [this] {
        if (backend_->analysis_progress().running || !confirmDiscard()) return;
        projectPath_.clear();
        dirty_ = false;
        updateTitle();
        std::vector<std::size_t> all;
        rust::Vec<std::size_t> rows;
        for (std::size_t i = 0; i < backend_->sample_count(); ++i) rows.push_back(i);
        backend_->remove_samples(rows);
        model_->refresh();
        refreshNavigator();
        updateInfo();
    });
    aNew->setShortcut(QKeySequence::New);
    auto *aOpen = file->addAction(s->standardIcon(QStyle::SP_DirOpenIcon), QString::fromUtf8("&Открыть проект..."), this, [this] { openProject(); });
    aOpen->setShortcut(QKeySequence::Open);
    auto *aSave = file->addAction(s->standardIcon(QStyle::SP_DialogSaveButton), QString::fromUtf8("&Сохранить проект"), this, [this] { saveProject(false); });
    aSave->setShortcut(QKeySequence::Save);
    file->addAction(QString::fromUtf8("Сохранить проект &как..."), QKeySequence("Ctrl+Shift+S"), this, [this] { saveProject(true); });
    file->addSeparator();
    auto *aAdd = file->addAction(s->standardIcon(QStyle::SP_DialogOpenButton), QString::fromUtf8("&Добавить образцы в проект..."), this, [this] { addSamples(); });
    aAdd->setShortcut(QKeySequence("Ctrl+K"));
    file->addAction(QString::fromUtf8("Параметры &проекта..."), this, [this] { projectOptions(); });
    auto *aExport = file->addAction(s->standardIcon(QStyle::SP_DialogSaveButton), QString::fromUtf8("&Экспорт таблицы..."), this, [this] { exportTable(); });
    aExport->setShortcut(QKeySequence("Ctrl+E"));
    file->addSeparator();
    file->addAction(QString::fromUtf8("В&ыход"), QKeySequence::Quit, this, &QWidget::close);

    auto *edit = menuBar()->addMenu(QString::fromUtf8("&Правка"));
    edit->addAction(QString::fromUtf8("Выделить &всё"), QKeySequence::SelectAll, table_, &QTableView::selectAll);
    auto *aDel = edit->addAction(s->standardIcon(QStyle::SP_TrashIcon), QString::fromUtf8("&Удалить из проекта"), this, [this] { removeSelected(); });
    edit->addSeparator();
    edit->addAction(QString::fromUtf8("&Панель для выделенных образцов..."), this, [this] { setPanelForSelected(); });
    auto *types = edit->addMenu(QString::fromUtf8("&Тип образца"));
    for (auto [t, label] : {std::pair{"sample", "Образец"}, std::pair{"ladder", "Аллельная лестница"}, std::pair{"positive", "Положительный контроль"}, std::pair{"negative", "Отрицательный контроль"}}) {
        const QString type = t;
        types->addAction(QString::fromUtf8(label), this, [this, type] {
            rust::Vec<std::size_t> v;
            for (int i : selectedRows()) v.push_back(static_cast<std::size_t>(i));
            backend_->set_samples_type(v, r(type));
            model_->refreshValues();
        });
    }
    aDel->setShortcut(QKeySequence::Delete);

    auto *an = menuBar()->addMenu(QString::fromUtf8("&Анализ"));
    analyzeAct_ = an->addAction(s->standardIcon(QStyle::SP_MediaPlay), QString::fromUtf8("&Анализировать"), this, [this] { analyze(false); });
    analyzeAct_->setShortcut(QKeySequence("Ctrl+R"));
    analyzeAllAct_ = an->addAction(QString::fromUtf8("Анализировать &все заново"), this, [this] { analyze(true); });
    analyzeAllAct_->setShortcut(QKeySequence("Ctrl+Shift+R"));
    an->addSeparator();
    an->addAction(s->standardIcon(QStyle::SP_MediaStop), QString::fromUtf8("&Остановить"), this, [this] { backend_->stop_analysis(); });

    auto *view = menuBar()->addMenu(QString::fromUtf8("&Вид"));
    view->addAction(QString::fromUtf8("&Образцы"), QKeySequence("Ctrl+Shift+1"), this, [this] { tabs_->setCurrentIndex(0); });
    view->addAction(QString::fromUtf8("&Сводка анализа"), QKeySequence("Ctrl+Shift+2"), this, [this] { refreshSummary(); tabs_->setCurrentIndex(1); });
    view->addAction(QString::fromUtf8("&Генотипы"), QKeySequence("Ctrl+Shift+3"), this, [this] { tabs_->setCurrentWidget(genoTable_); });
    view->addAction(QString::fromUtf8("&Информация об образце"), QKeySequence("Ctrl+F1"), this, [this] { tabs_->setCurrentWidget(infoTree_); });
    view->addAction(QString::fromUtf8("&Необработанные данные"), QKeySequence("Ctrl+F2"), this, [this] { tabs_->setCurrentWidget(rawPage_); });
    view->addAction(QString::fromUtf8("Данные &EPT"), QKeySequence("Ctrl+F3"), this, [this] { tabs_->setCurrentWidget(eptPage_); });
    view->addAction(QString::fromUtf8("&Журнал правок обозначений..."), this, [this] { labelEditViewer(); });
    auto *aNav = view->addAction(QString::fromUtf8("&Навигатор"));
    aNav->setCheckable(true);
    aNav->setChecked(true);
    aNav->setShortcut(QKeySequence("Ctrl+Shift+N"));
    connect(aNav, &QAction::toggled, nav_, &QWidget::setVisible);

    auto *plots = menuBar()->addMenu(QString::fromUtf8("&Графики"));
    auto *aPlots = plots->addAction(s->standardIcon(QStyle::SP_ComputerIcon), QString::fromUtf8("Показать &графики"), this, [this] { showPlots(); });
    aPlots->setShortcut(QKeySequence("Ctrl+L"));

    auto *tools = menuBar()->addMenu(QString::fromUtf8("&Инструменты"));
    tools->addAction(QString::fromUtf8("Менеджер &панелей..."), QKeySequence("Ctrl+J"), this, [this] { panelManager(); });
    tools->addAction(QString::fromUtf8("Настройки &графиков..."), this, [this] { PlotSettingsManager m(*backend_, this); m.exec(); });
    tools->addAction(QString::fromUtf8("Настройки &таблиц..."), this, [this] {
        TableSettingsManager m(*backend_, this);
        m.onChanged = [this] { fillTableSettings(); applyTableSetting(); };
        m.exec();
    });
    tools->addAction(QString::fromUtf8("&Редактор настройки таблицы..."), QKeySequence("Ctrl+T"), this, [this] { editTableSetting(); });
    tools->addSeparator();
    tools->addAction(QString::fromUtf8("Импорт &метода анализа (GeneMapper XML)..."), this, [this] { importLibraryFile(true); });
    tools->addAction(QString::fromUtf8("Импорт &стандарта длин (GeneMapper XML)..."), this, [this] { importLibraryFile(false); });

    auto *help = menuBar()->addMenu(QString::fromUtf8("&Справка"));
    help->addAction(QString::fromUtf8("&О программе"), this, [this] { about(); });

    auto *tb = addToolBar(QString::fromUtf8("Основная"));
    tb->setObjectName("mainToolbar");
    tb->setIconSize(QSize(18, 18));
    tb->addAction(aNew);
    tb->addAction(aOpen);
    tb->addAction(aSave);
    tb->addSeparator();
    tb->addAction(aAdd);
    tb->addAction(aExport);
    tb->addSeparator();
    tb->addAction(analyzeAct_);
    tb->addSeparator();
    tb->addAction(aPlots);
    tb->addSeparator();
    tb->addWidget(new QLabel(QString::fromUtf8(" Настройка таблиц: ")));
    tableSetting_ = new QComboBox;
    tableSetting_->setObjectName("tableSetting");
    tableSetting_->setMinimumWidth(210);
    tableSetting_->setToolTip(QString::fromUtf8("Столбцы, фильтры и сортировка таблиц «Образцы» и «Генотипы»"));
    connect(tableSetting_, &QComboBox::activated, this, [this] {
        const QByteArray n = tableSetting_->currentText().toUtf8();
        backend_->table_setting_set_active(rust::Str(n.constData(), n.size()));
        applyTableSetting();
    });
    tb->addWidget(tableSetting_);
    auto *aTs = tb->addAction(s->standardIcon(QStyle::SP_FileDialogDetailedView), QString::fromUtf8("Редактор настройки таблицы (Ctrl+T)"), this, [this] { editTableSetting(); });
    aTs->setToolTip(QString::fromUtf8("Редактор настройки таблицы (Ctrl+T)"));
    tableNote_ = new QLabel;
    tableNote_->setStyleSheet("color:#7a4b00; padding-left:8px");
    tb->addWidget(tableNote_);
    fillTableSettings();
    applyTableSetting();
}

void MainWindow::addSamples() {
    QSettings st;
    const QStringList files = QFileDialog::getOpenFileNames(this, QString::fromUtf8("Добавить образцы"), st.value("main/fsaDir").toString(),
                                                            QString::fromUtf8("Файлы образцов (*.fsa *.hid *.FSA *.HID);;Все файлы (*)"));
    if (files.isEmpty()) return;
    st.setValue("main/fsaDir", QFileInfo(files.first()).absolutePath());
    // method, size standard and run folder for the new samples
    QDialog dlg(this);
    dlg.setWindowTitle(QString::fromUtf8("Добавить образцы"));
    auto *form = new QFormLayout(&dlg);
    auto *method = new QComboBox;
    for (const auto &m : backend_->method_list()) method->addItem(q(m));
    method->setCurrentText(st.value("main/method").toString());
    auto *stdBox = new QComboBox;
    for (const auto &m : backend_->size_standard_list()) stdBox->addItem(q(m));
    stdBox->setCurrentText(st.value("main/sizeStd").toString());
    auto *run = new QLineEdit(QFileInfo(files.first()).dir().dirName());
    form->addRow(QString::fromUtf8("Файлов:"), new QLabel(QString::number(files.size())));
    form->addRow(QString::fromUtf8("Метод анализа:"), method);
    form->addRow(QString::fromUtf8("Стандарт длин:"), stdBox);
    form->addRow(QString::fromUtf8("Папка запуска:"), run);
    auto *panel = new QComboBox;
    panel->addItem(QString::fromUtf8("(без панели — только калибровка)"), QString());
    for (const auto &o : backend_->panel_options()) panel->addItem(q(o.label), q(o.key));
    {
        const int at = panel->findData(st.value("main/panel").toString());
        if (at >= 0) panel->setCurrentIndex(at);
        else if (panel->count() > 1) panel->setCurrentIndex(1);
    }
    form->addRow(QString::fromUtf8("Панель:"), panel);
    auto *bb = new QDialogButtonBox(QDialogButtonBox::Ok | QDialogButtonBox::Cancel);
    connect(bb, &QDialogButtonBox::accepted, &dlg, &QDialog::accept);
    connect(bb, &QDialogButtonBox::rejected, &dlg, &QDialog::reject);
    form->addRow(bb);
    if (dlg.exec() != QDialog::Accepted) return;
    st.setValue("main/method", method->currentText());
    st.setValue("main/sizeStd", stdBox->currentText());
    rust::Vec<rust::String> paths;
    for (const QString &f : files) paths.push_back(r(f));
    QApplication::setOverrideCursor(Qt::WaitCursor);
    st.setValue("main/panel", panel->currentData().toString());
    auto errors = backend_->add_sample_files(paths, r(run->text()), r(method->currentText()), r(stdBox->currentText()), r(panel->currentData().toString()));
    QApplication::restoreOverrideCursor();
    model_->refresh();
    refreshNavigator();
    markDirty();
    status_->setText(QString::fromUtf8("Образцов в проекте: %1").arg(backend_->sample_count()));
    if (!errors.empty()) {
        QStringList l;
        for (const auto &e : errors) l << q(e);
        QMessageBox::warning(this, "OpenTrace", QString::fromUtf8("Не добавлены:\n") + l.join("\n"));
    }
}

void MainWindow::importLibraryFile(bool method) {
    QSettings st;
    const QString path = QFileDialog::getOpenFileName(this, method ? QString::fromUtf8("Импорт метода анализа") : QString::fromUtf8("Импорт стандарта длин"),
                                                      st.value("main/xmlDir").toString(), "XML (*.xml);;All files (*)");
    if (path.isEmpty()) return;
    st.setValue("main/xmlDir", QFileInfo(path).absolutePath());
    const QString name = q(method ? backend_->import_method(r(path)) : backend_->import_size_standard(r(path)));
    if (name.isEmpty()) QMessageBox::warning(this, "OpenTrace", QString::fromUtf8("Не удалось импортировать:\n") + q(backend_->last_import_error()));
    else status_->setText((method ? QString::fromUtf8("Импортирован метод: ") : QString::fromUtf8("Импортирован стандарт длин: ")) + name);
}

QVector<int> MainWindow::selectedRows() const {
    QVector<int> out;
    for (const QModelIndex &i : table_->selectionModel()->selectedRows()) out << proxy_->mapToSource(i).row();
    std::sort(out.begin(), out.end());
    return out;
}

void MainWindow::removeSelected() {
    const auto rows = selectedRows();
    if (rows.isEmpty() || backend_->analysis_progress().running) return;
    if (QMessageBox::question(this, "OpenTrace", QString::fromUtf8("Удалить из проекта образцов: %1?").arg(rows.size())) != QMessageBox::Yes) return;
    rust::Vec<std::size_t> v;
    for (int i : rows) v.push_back(static_cast<std::size_t>(i));
    backend_->remove_samples(v);
    model_->refresh();
    refreshNavigator();
}

void MainWindow::analyze(bool all) {
    if (backend_->sample_count() == 0 || backend_->analysis_progress().running) return;
    if (all && !autotest_) {   // re-analysis replaces the results: label edits of the samples are dropped (kept in the journal as undone)
        rust::Vec<std::size_t> v;
        for (std::size_t i = 0; i < backend_->sample_count(); ++i) v.push_back(i);
        const std::size_t n = backend_->active_edit_count(v);
        if (n && QMessageBox::question(this, "OpenTrace", QString::fromUtf8("Повторный анализ отменит правки меток: %1. Продолжить?").arg(n)) != QMessageBox::Yes) return;
    }
    backend_->start_analysis(all);
    analyzeAct_->setEnabled(false);
    analyzeAllAct_->setEnabled(false);
    stop_->setEnabled(true);
    poll_->start();
    pollProgress();
}

void MainWindow::pollProgress() {
    const ProgressInfo p = backend_->analysis_progress();
    progress_->setMaximum(static_cast<int>(std::max<std::size_t>(1, p.total)));
    progress_->setValue(static_cast<int>(p.done));
    model_->refreshValues();
    if (p.running) {
        const QString phase = q(p.phase);
        status_->setText((phase == "ladders" ? QString::fromUtf8("Оценка аллельных лестниц: ") : phase == "genotype" ? QString::fromUtf8("Генотипирование: ") : QString::fromUtf8("Анализ: ")) + q(p.message));
        const QString ask = q(backend_->ask_pending());
        if (!ask.isEmpty() && !asking_) {
            asking_ = true;
            const QString a = autotest_ ? QString(qEnvironmentVariable("OT_QT_AUTOTEST_ALAS", "passing")) : alasDialog(ask);
            backend_->ask_answer(r(a));
            asking_ = false;
        }
        return;
    }
    poll_->stop();
    analyzeAct_->setEnabled(true);
    analyzeAllAct_->setEnabled(true);
    stop_->setEnabled(false);
    int failed = 0;
    for (int i = 0; i < model_->rowCount(); ++i) if (model_->row(i).status == "failed") ++failed;
    status_->setText(q(p.message) == "stopped" ? QString::fromUtf8("Анализ остановлен")
                     : failed ? QString::fromUtf8("Анализ завершён; ошибок: %1").arg(failed)
                              : QString::fromUtf8("Анализ завершён"));
    updateInfo();
    refreshGenotypes();
    refreshSummary();
    markDirty();
    if (autotest_) QTimer::singleShot(300, this, [this] {
        const QString out = qEnvironmentVariable("OT_QT_AUTOTEST_OUT", ".");
        QDir().mkpath(out);
        table_->selectAll();
        grab().save(out + "/main.png");
        tabs_->setCurrentWidget(genoTable_);
        QApplication::processEvents();
        grab().save(out + "/genotypes.png");
        refreshSummary();
        tabs_->setCurrentIndex(1);
        QApplication::processEvents();
        grab().save(out + "/summary.png");
        for (auto [w, name] : {std::pair<QWidget *, const char *>{infoTree_, "/info.png"}, {rawPage_, "/raw.png"}, {eptPage_, "/ept.png"}}) {
            tabs_->setCurrentWidget(w);
            QApplication::processEvents();
            grab().save(out + name);
        }
        tabs_->setCurrentIndex(0);
        QVector<int> ok;
        int failed = 0;
        QStringList rows;
        for (int i = 0; i < model_->rowCount(); ++i) {
            const RowView &rv = model_->row(i);
            if (rv.status == "analyzed") ok << i;
            if (rv.status == "failed") ++failed;
            QString err = rv.error;
            err.replace("\\", "\\\\").replace("\"", "\\\"").replace("\n", " ");
            rows << QString("{\"file\":\"%1\",\"status\":\"%2\",\"sq\":\"%3\",\"error\":\"%4\"}").arg(rv.file, rv.status, rv.sq, err);
        }
        bool pdf = false;
        int plotSettings = 0;
        if (!ok.isEmpty()) {
            auto *w = new PlotWindow(*backend_, ok.mid(0, 3), nullptr);
            w->resize(1400, 900);
            w->show();
            QEventLoop wait;                        // let the window lay out its panes before the screenshot
            QTimer::singleShot(400, &wait, &QEventLoop::quit);
            wait.exec();
            w->grab().save(out + "/plot.png");
            pdf = w->savePdf(out + "/plots.pdf");
            // every factory plot setting applied to the same window (layouts, tables, labels), one screenshot each
            for (const auto &n : backend_->plot_setting_names()) {
                const QString name = q(n);
                w->applySetting(name);
                QTimer::singleShot(150, &wait, &QEventLoop::quit);
                wait.exec();
                QString file = name;
                file.replace(QRegularExpression("[^A-Za-z0-9]+"), "_");
                w->grab().save(out + "/plot_setting_" + file + ".png");
                ++plotSettings;
            }
            w->applySetting("Data Interpretation");
            w->close();
        }
        // label edit round trip: delete one allele label of the first genotyped sample, then undo it
        bool editOk = false;
        {
            auto count = [this] { std::size_t n = 0; for (const auto &g : backend_->genotype_rows()) n += g.alleles.size(); return n; };
            const std::size_t before = count();
            for (std::size_t i = 0; i < backend_->sample_count() && !editOk; ++i) {
                for (const auto &l : backend_->plot_labels(i, true)) {
                    if (q(l.kind) != "call" || q(backend_->sample_row(i).stype) == "ladder") continue;
                    const QByteArray spec = QString("{\"action\":\"delete\",\"marker\":\"%1\",\"callId\":\"%2\"}").arg(q(l.marker), q(l.id)).toUtf8();
                    const bool applied = backend_->apply_edit_json(i, rust::Str(spec.constData(), spec.size()), "self-test").empty();
                    const bool fewer = count() == before - 1;
                    rust::Vec<std::size_t> v;
                    v.push_back(i);
                    const bool undone = !backend_->undo_last_edit(v).empty() && count() == before;
                    editOk = applied && fewer && undone;
                    break;
                }
            }
        }
        {
            QFile ff(out + "/flags.json");
            if (ff.open(QIODevice::WriteOnly)) ff.write(q(backend_->flags_json()).toUtf8());
        }
        const QString digest = q(backend_->genotype_digest());
        {
            QFile d(out + "/digest.txt");
            if (d.open(QIODevice::WriteOnly)) d.write(digest.toUtf8());
        }
        // project file round trip: save, reopen, same genotypes
        bool reopen = q(backend_->save_project_file(r(out + "/project.otproj"))).isEmpty() && q(backend_->open_project_file(r(out + "/project.otproj"))).isEmpty() &&
                      q(backend_->genotype_digest()) == digest;
        model_->refresh();
        // table settings: a setting with column filters, its own columns, sort keys and three alleles; then back
        const int tableSettings = static_cast<int>(backend_->table_setting_names().size());
        bool tableOk = false;
        {
            const QByteArray js = R"({"name":"self-test","samples":{"visible":["sampleName","sampleType","SQ"],"sort":[{"col":"sampleName","desc":true}],"filters":{"sampleType":{"op":3}}},
                "genotypes":{"visible":["sampleName","marker","dye","allele","size","height","GQ"],"numAlleles":3,"sort":[{"col":"marker","desc":true}],"filters":{"dye":{"op":1}}}})";
            const QByteArray none, name("self-test"), dflt("Data Interpretation");
            const bool put = backend_->table_setting_put(rust::Str(js.constData(), js.size()), rust::Str(none.constData(), none.size())).empty();
            backend_->table_setting_set_active(rust::Str(name.constData(), name.size()));
            fillTableSettings();
            applyTableSetting();
            int ladders = 0;
            for (int i = 0; i < model_->rowCount(); ++i) if (model_->row(i).stype == "ladder") ++ladders;
            bool blue = genoShown_ > 0 && genoShown_ < genoTotal_, sorted = true;
            // independent check of the order: digit runs compare as numbers, letters without regard to case
            auto natural = [](const QString &a, const QString &b) {
                static const QRegularExpression part("(\\d+|\\D+)");
                auto ia = part.globalMatch(a.toLower()), ib = part.globalMatch(b.toLower());
                while (ia.hasNext() && ib.hasNext()) {
                    const QString x = ia.next().captured(), y = ib.next().captured();
                    bool nx = false, ny = false;
                    const qlonglong vx = x.toLongLong(&nx), vy = y.toLongLong(&ny);
                    if (nx && ny) { if (vx != vy) return vx < vy ? -1 : 1; }
                    else if (x != y) return x < y ? -1 : 1;
                }
                return ia.hasNext() ? 1 : ib.hasNext() ? -1 : 0;
            };
            for (int i = 0; i < genoTable_->rowCount(); ++i) {
                if (genoTable_->item(i, 2)->text() != "B") blue = false;
                if (i && natural(genoTable_->item(i - 1, 1)->text(), genoTable_->item(i, 1)->text()) < 0) sorted = false;   // marker, descending
            }
            tableOk = put && model_->columnCount() == 3 && ladders > 0 && proxy_->rowCount() == ladders && genoTable_->columnCount() == 13 && blue && sorted && !tableNote_->text().isEmpty();
            tabs_->setCurrentIndex(0);
            QApplication::processEvents();
            grab().save(out + "/table_setting_samples.png");
            tabs_->setCurrentWidget(genoTable_);
            QApplication::processEvents();
            grab().save(out + "/table_setting_genotypes.png");
            {
                TableSettingEditor ed(*backend_, parseJsonObject(q(backend_->table_setting_json(rust::Str(name.constData(), name.size())))), "self-test", this);
                ed.show();
                QApplication::processEvents();
                ed.grab().save(out + "/table_setting_editor.png");
            }
            backend_->table_setting_set_active(rust::Str(dflt.constData(), dflt.size()));
            backend_->table_setting_remove(rust::Str(name.constData(), name.size()));
            fillTableSettings();
            applyTableSetting();
            tableOk = tableOk && model_->columnCount() == 16 && proxy_->rowCount() == model_->rowCount() && tableNote_->text().isEmpty();
            tabs_->setCurrentIndex(0);
            QApplication::processEvents();
            grab().save(out + "/table_setting_default.png");
        }
        QFile f(out + "/result.json");
        if (f.open(QIODevice::WriteOnly)) {
            f.write(QString("{\"samples\":%1,\"analyzed\":%2,\"failed\":%3,\"pdf\":%4,\"genotype_rows\":%5,\"reopen_identical\":%7,\"edit_ok\":%8,\"plot_settings\":%9,\"table_settings\":%10,\"table_setting_ok\":%11,\"rows\":[%6]}\n")
                        .arg(model_->rowCount()).arg(ok.size()).arg(failed).arg(pdf ? "true" : "false").arg(genoTable_->rowCount()).arg(rows.join(",")).arg(reopen ? "true" : "false").arg(editOk ? "true" : "false").arg(plotSettings)
                        .arg(tableSettings).arg(tableOk ? "true" : "false").toUtf8());
        }
        QApplication::exit(ok.isEmpty() ? 1 : 0);
    });
}

// Scripted run (CI, screenshots): add OT_QT_AUTOTEST_FILES (separated by ';'), analyse with OT_QT_AUTOTEST_METHOD /
// OT_QT_AUTOTEST_STD, then save main.png, plot.png, plots.pdf and result.json into OT_QT_AUTOTEST_OUT and quit.
void MainWindow::autotest() {
    autotest_ = true;
    QStringList files = qEnvironmentVariable("OT_QT_AUTOTEST_FILES").split(';', Qt::SkipEmptyParts);
    QString synthStd, synthPanel;
    if (files.size() == 1 && files.first().startsWith("synthetic:")) {   // invented run files of the core (CI)
        const int n = files.first().mid(10).toInt();
        files.clear();
        for (const auto &p : backend_->autotest_synthetic(r(qEnvironmentVariable("OT_QT_AUTOTEST_OUT", ".") + "/fsa"), static_cast<std::size_t>(std::max(1, n)))) files << q(p);
        synthStd = q(synthetic_standard_name());
        synthPanel = q(backend_->last_import_error());   // panel key of the invented kit
    }
    // kit files of a local data set (OT_QT_AUTOTEST_KIT_DIR: GeneMapper *Panels*.txt, *bins*.txt, *stutter*.txt)
    if (qEnvironmentVariableIsSet("OT_QT_AUTOTEST_KIT_DIR")) {
        QDir kd(qEnvironmentVariable("OT_QT_AUTOTEST_KIT_DIR"));
        for (const QString &kind : {QString("panels"), QString("bins"), QString("stutter")})
            for (const QString &fn : kd.entryList({"*.txt"}, QDir::Files))
                if (fn.contains(kind, Qt::CaseInsensitive)) backend_->import_kit(r(kd.filePath(fn)), r(kind), r(""));
        const auto opts = backend_->panel_options();
        if (!opts.empty()) synthPanel = q(opts[0].key);
    }
    rust::Vec<rust::String> paths;
    for (const QString &f : files) paths.push_back(r(f));
    QString method = qEnvironmentVariable("OT_QT_AUTOTEST_METHOD"), stdName = qEnvironmentVariable("OT_QT_AUTOTEST_STD");
    if (qEnvironmentVariableIsSet("OT_QT_AUTOTEST_METHOD_XML")) method = q(backend_->import_method(r(qEnvironmentVariable("OT_QT_AUTOTEST_METHOD_XML"))));
    if (qEnvironmentVariableIsSet("OT_QT_AUTOTEST_STD_XML")) stdName = q(backend_->import_size_standard(r(qEnvironmentVariable("OT_QT_AUTOTEST_STD_XML"))));
    if (method.isEmpty() && !backend_->method_list().empty()) method = q(backend_->method_list()[0]);
    if (stdName.isEmpty()) stdName = synthStd.isEmpty() ? QString("GS600_LIZ_(60-460)") : synthStd;
    backend_->add_sample_files(paths, r("autotest"), r(method), r(stdName), r(synthPanel));
    model_->refresh();
    refreshNavigator();
    analyze(false);
}

void MainWindow::showPlots() {
    QVector<int> rows = selectedRows();
    if (rows.isEmpty()) { QMessageBox::information(this, "OpenTrace", QString::fromUtf8("Выделите образцы в таблице.")); return; }
    QVector<int> ok;
    for (int i : rows) if (model_->row(i).status == "analyzed") ok << i;
    if (ok.isEmpty()) { QMessageBox::information(this, "OpenTrace", QString::fromUtf8("Выделенные образцы ещё не проанализированы.")); return; }
    auto *w = new PlotWindow(*backend_, ok, nullptr);
    w->onEdited = [this] { model_->refreshValues(); refreshGenotypes(); markDirty(); };
    w->onLabelEditViewer = [this] { labelEditViewer(); };
    w->show();
}

void MainWindow::exportTable() {
    QSettings st;
    const QString path = QFileDialog::getSaveFileName(this, QString::fromUtf8("Экспорт таблицы"), st.value("main/exportDir").toString() + (tabs_->currentWidget() == genoTable_ ? "/genotypes.txt" : "/samples.txt"),
                                                      QString::fromUtf8("Текст с табуляцией (*.txt);;CSV (*.csv)"));
    if (path.isEmpty()) return;
    st.setValue("main/exportDir", QFileInfo(path).absolutePath());
    QFile f(path);
    if (!f.open(QIODevice::WriteOnly | QIODevice::Text)) { QMessageBox::warning(this, "OpenTrace", f.errorString()); return; }
    const QChar sep = path.endsWith(".csv", Qt::CaseInsensitive) ? ',' : '\t';
    QTextStream out(&f);
    out.setEncoding(QStringConverter::Utf8);
    auto cell = [sep](QString v) {
        if (sep == ',' && (v.contains(',') || v.contains('"'))) v = '"' + QString(v).replace("\"", "\"\"") + '"';
        return v;
    };
    if (tabs_->currentWidget() == genoTable_) {   // Genotypes tab: the table as shown (GQ as text)
        QStringList head;
        for (int c = 0; c < genoTable_->columnCount(); ++c) head << cell(genoTable_->horizontalHeaderItem(c)->text());
        out << head.join(sep) << "\n";
        for (int i = 0; i < genoTable_->rowCount(); ++i) {
            QStringList row;
            for (int c = 0; c < genoTable_->columnCount(); ++c) {
                const auto *it = genoTable_->item(i, c);
                row << cell(it ? (it->text().isEmpty() ? it->data(Qt::UserRole).toString() : it->text()) : QString());
            }
            out << row.join(sep) << "\n";
        }
        status_->setText(QString::fromUtf8("Сохранено: ") + path);
        return;
    }
    QStringList head;   // Samples tab: the columns and rows as shown (quality columns as text)
    for (int c = 0; c < model_->columnCount(); ++c) head << cell(model_->headerData(c, Qt::Horizontal, Qt::DisplayRole).toString());
    out << head.join(sep) << "\n";
    for (int i = 0; i < proxy_->rowCount(); ++i) {
        QStringList row;
        const int src = proxy_->mapToSource(proxy_->index(i, 0)).row();
        for (int c = 0; c < model_->columnCount(); ++c) row << cell(model_->exportText(src, c));
        out << row.join(sep) << "\n";
    }
    status_->setText(QString::fromUtf8("Сохранено: ") + path);
}

void MainWindow::refreshNavigator() {
    nav_->clear();
    auto *root = new QTreeWidgetItem(nav_, {QString::fromUtf8("Проект")});
    root->setIcon(0, style()->standardIcon(QStyle::SP_DirHomeIcon));
    QMap<QString, QTreeWidgetItem *> runs;
    for (int i = 0; i < model_->rowCount(); ++i) {
        const RowView &rv = model_->row(i);
        auto *&run = runs[rv.runFolder];
        if (!run) { run = new QTreeWidgetItem(root, {rv.runFolder}); run->setIcon(0, style()->standardIcon(QStyle::SP_DirIcon)); }
        auto *it = new QTreeWidgetItem(run, {rv.name.isEmpty() ? rv.file : rv.name});
        it->setIcon(0, style()->standardIcon(QStyle::SP_FileIcon));
        it->setToolTip(0, rv.file);
    }
    nav_->expandToDepth(0);
}

void MainWindow::updateInfo() {
    const auto rows = selectedRows();
    if (rows.isEmpty()) { info_->clear(); return; }
    info_->setPlainText(q(backend_->sample_info_text(static_cast<std::size_t>(rows.first()))));
}

void MainWindow::about() {
    QMessageBox::about(this, QString::fromUtf8("О программе"),
                       QString::fromUtf8("<b>OpenTrace Desktop</b> (нативный интерфейс Qt %1)<br>Ядро анализа: %2<br><br>"
                                         "Исследовательское ПО: не валидировано для диагностического или судебно-экспертного применения.<br>"
                                         "Ресурсы: %3")
                           .arg(qVersion(), q(core_version()), q(backend_->resources_dir()).toHtmlEscaped()));
}


void MainWindow::setPanelForSelected() {
    const auto rows = selectedRows();
    if (rows.isEmpty()) { QMessageBox::information(this, "OpenTrace", QString::fromUtf8("Выделите образцы в таблице.")); return; }
    QStringList labels{QString::fromUtf8("(без панели)")};
    QStringList keys{QString()};
    for (const auto &o : backend_->panel_options()) { labels << q(o.label); keys << q(o.key); }
    bool ok = false;
    const QString pick = QInputDialog::getItem(this, QString::fromUtf8("Панель"), QString::fromUtf8("Панель для выделенных образцов (%1):").arg(rows.size()), labels, labels.size() > 1 ? 1 : 0, false, &ok);
    if (!ok) return;
    rust::Vec<std::size_t> v;
    for (int i : rows) v.push_back(static_cast<std::size_t>(i));
    backend_->set_samples_panel(v, r(keys[labels.indexOf(pick)]));
    model_->refreshValues();
    refreshGenotypes();
}

// Panel Manager (first version): kits imported from GeneMapper panels / bins / stutter files.
void MainWindow::panelManager() {
    QDialog dlg(this);
    dlg.setWindowTitle(QString::fromUtf8("Менеджер панелей"));
    dlg.resize(900, 420);
    auto *lay = new QVBoxLayout(&dlg);
    auto *tbl = new QTableWidget(0, 4);
    tbl->setHorizontalHeaderLabels({QString::fromUtf8("Набор реагентов"), QString::fromUtf8("Панели"), QString::fromUtf8("Наборы бинов"), QString::fromUtf8("Статтеры")});
    tbl->horizontalHeader()->setStretchLastSection(true);
    tbl->setEditTriggers(QAbstractItemView::NoEditTriggers);
    tbl->setSelectionBehavior(QAbstractItemView::SelectRows);
    tbl->setSelectionMode(QAbstractItemView::SingleSelection);
    auto fill = [this, tbl] {
        tbl->setRowCount(0);
        for (const auto &line : backend_->kit_summary()) {
            const QStringList c = q(line).split('\t');
            const int r0 = tbl->rowCount();
            tbl->insertRow(r0);
            for (int k = 0; k < 4 && k < c.size(); ++k) tbl->setItem(r0, k, new QTableWidgetItem(c[k]));
        }
        tbl->resizeColumnsToContents();
    };
    fill();
    auto *note = new QLabel(QString::fromUtf8("Импортируйте файлы GeneMapper: сначала панели (Panels), затем бины (Bins) и статтеры (Stutter) к выбранному набору."));
    note->setWordWrap(true);
    lay->addWidget(note);
    lay->addWidget(tbl, 1);
    auto *row = new QHBoxLayout;
    auto selectedKit = [tbl] { const auto it = tbl->currentRow() >= 0 ? tbl->item(tbl->currentRow(), 0) : nullptr; return it ? it->text() : QString(); };
    for (auto [kind, label] : {std::pair{"panels", "Импорт панелей..."}, std::pair{"bins", "Импорт бинов..."}, std::pair{"stutter", "Импорт статтеров..."}}) {
        auto *b = new QPushButton(QString::fromUtf8(label));
        const QString k = kind;
        connect(b, &QPushButton::clicked, &dlg, [this, k, fill, selectedKit, &dlg] {
            QSettings st;
            const QString path = QFileDialog::getOpenFileName(&dlg, QString::fromUtf8("Файл GeneMapper"), st.value("main/kitDir").toString(), "Text (*.txt);;All files (*)");
            if (path.isEmpty()) return;
            st.setValue("main/kitDir", QFileInfo(path).absolutePath());
            const QString res = q(backend_->import_kit(r(path), r(k), r(selectedKit())));
            if (res.isEmpty()) QMessageBox::warning(&dlg, "OpenTrace", QString::fromUtf8("Не удалось импортировать:\n") + q(backend_->last_import_error()));
            else status_->setText(QString::fromUtf8("Импортировано: ") + res);
            fill();
        });
        row->addWidget(b);
    }
    auto *del = new QPushButton(QString::fromUtf8("Удалить набор"));
    connect(del, &QPushButton::clicked, &dlg, [this, fill, selectedKit, &dlg] {
        const QString k = selectedKit();
        if (k.isEmpty()) return;
        if (QMessageBox::question(&dlg, "OpenTrace", QString::fromUtf8("Удалить набор «%1» из Менеджера панелей?").arg(k)) != QMessageBox::Yes) return;
        backend_->delete_kit(r(k));
        fill();
    });
    row->addWidget(del);
    row->addStretch(1);
    auto *close = new QPushButton(QString::fromUtf8("Закрыть"));
    connect(close, &QPushButton::clicked, &dlg, &QDialog::accept);
    row->addWidget(close);
    lay->addLayout(row);
    dlg.exec();
}

// Project Options (genotyping part): Allelic Ladder Analysis Summary, bin offsets, minimum call heights.
void MainWindow::projectOptions() {
    QDialog dlg(this);
    dlg.setWindowTitle(QString::fromUtf8("Параметры проекта"));
    auto *lay = new QVBoxLayout(&dlg);
    auto radio = [&](const QString &title, const QString &key, const QList<QPair<QString, QString>> &items) {
        auto *box = new QGroupBox(title);
        auto *bl = new QVBoxLayout(box);
        auto *grp = new QButtonGroup(box);
        const QString cur = q(backend_->option_get(r(key)));
        for (const auto &it : items) {
            auto *rb = new QRadioButton(it.second);
            rb->setProperty("v", it.first);
            rb->setChecked(it.first == cur);
            grp->addButton(rb);
            bl->addWidget(rb);
        }
        lay->addWidget(box);
        return grp;
    };
    auto *alas = radio(QString::fromUtf8("Если аллельная лестница не прошла проверку"), "alas",
                       {{"stop", QString::fromUtf8("Остановить и показать сводку по лестницам")}, {"passing", QString::fromUtf8("Продолжить с папками, где есть годная лестница")}, {"all", QString::fromUtf8("Генотипировать все папки")}});
    auto *bo = radio(QString::fromUtf8("Смещение бинов по лестницам"), "bin_offsets",
                     {{"green", QString::fromUtf8("Лестницы с зелёным SQ")}, {"greenYellow", QString::fromUtf8("Лестницы с зелёным и жёлтым SQ")}});
    auto *form = new QFormLayout;
    auto *homo = new QDoubleSpinBox;
    homo->setRange(0, 100000);
    homo->setDecimals(0);
    homo->setValue(q(backend_->option_get(r("homo_min_height"))).toDouble());
    auto *het = new QDoubleSpinBox;
    het->setRange(0, 100000);
    het->setDecimals(0);
    het->setValue(q(backend_->option_get(r("het_min_height"))).toDouble());
    auto *pu = new QCheckBox(QString::fromUtf8("Не вызывать аллели на пиках pull-up"));
    pu->setChecked(q(backend_->option_get(r("pullup_excludes_calls"))) == "true");
    form->addRow(QString::fromUtf8("Мин. высота вызова, гомозигота (RFU):"), homo);
    form->addRow(QString::fromUtf8("Мин. высота вызова, гетерозигота (RFU):"), het);
    lay->addLayout(form);
    lay->addWidget(pu);
    auto *bb = new QDialogButtonBox(QDialogButtonBox::Ok | QDialogButtonBox::Cancel);
    connect(bb, &QDialogButtonBox::accepted, &dlg, &QDialog::accept);
    connect(bb, &QDialogButtonBox::rejected, &dlg, &QDialog::reject);
    lay->addWidget(bb);
    if (dlg.exec() != QDialog::Accepted) return;
    if (alas->checkedButton()) backend_->option_set(r("alas"), r(alas->checkedButton()->property("v").toString()));
    if (bo->checkedButton()) backend_->option_set(r("bin_offsets"), r(bo->checkedButton()->property("v").toString()));
    backend_->option_set(r("homo_min_height"), r(QString::number(homo->value())));
    backend_->option_set(r("het_min_height"), r(QString::number(het->value())));
    backend_->option_set(r("pullup_excludes_calls"), r(pu->isChecked() ? "true" : "false"));
}

// Allelic Ladder Analysis Summary (ID-X): which run folders have no passing ladder; continue with the passing ones,
// genotype all folders anyway, or stop and look at the ladders.
QString MainWindow::alasDialog(const QString &summaryJson) {
    QDialog dlg(this);
    dlg.setWindowTitle(QString::fromUtf8("Сводка анализа аллельных лестниц"));
    dlg.resize(760, 380);
    auto *lay = new QVBoxLayout(&dlg);
    auto *intro = new QLabel(QString::fromUtf8("В одной или нескольких папках запуска нет аллельной лестницы, прошедшей требования калибровки (SQ) и генотипирования (CGQ)."));
    intro->setWordWrap(true);
    lay->addWidget(intro);
    auto *tbl = new QTableWidget(0, 5);
    tbl->setHorizontalHeaderLabels({QString::fromUtf8("Папка запуска"), QString::fromUtf8("Панель"), QString::fromUtf8("Лестница"), "SQ", QString::fromUtf8("Качество")});
    tbl->setEditTriggers(QAbstractItemView::NoEditTriggers);
    tbl->horizontalHeader()->setStretchLastSection(true);
    const QJsonArray groups = QJsonDocument::fromJson(summaryJson.toUtf8()).array();
    for (const auto &gv : groups) {
        const QJsonObject g = gv.toObject();
        const QJsonArray lad = g["ladders"].toArray();
        auto add = [&](const QString &file, const QString &sq, const QString &qual) {
            const int r0 = tbl->rowCount();
            tbl->insertRow(r0);
            tbl->setItem(r0, 0, new QTableWidgetItem(g["run_folder"].toString()));
            tbl->setItem(r0, 1, new QTableWidgetItem(g["panel"].toString()));
            tbl->setItem(r0, 2, new QTableWidgetItem(file));
            auto *sqi = new QTableWidgetItem;
            sqi->setIcon(qualityIcon(sq));
            sqi->setToolTip(sq);
            tbl->setItem(r0, 3, sqi);
            auto *qi = new QTableWidgetItem(qual);
            qi->setIcon(qualityIcon(g["pass"].toBool() ? "pass" : "low"));
            tbl->setItem(r0, 4, qi);
        };
        if (lad.isEmpty()) add(QString::fromUtf8("(нет лестницы)"), "na", g["status"].toString());
        for (const auto &lv : lad) {
            const QJsonObject l = lv.toObject();
            const QString qual = l["quality"].isNull() ? g["status"].toString() : (l["quality"].toInt() == 0 ? QString::fromUtf8("годна") : QString::fromUtf8("не годна (код %1)").arg(l["quality"].toInt()));
            add(l["file"].toString(), l["sq"].toString(), qual);
        }
    }
    tbl->resizeColumnsToContents();
    lay->addWidget(tbl, 1);
    QString answer = "passing";
    auto *bb = new QDialogButtonBox;
    auto *bPass = bb->addButton(QString::fromUtf8("Продолжить с годными папками"), QDialogButtonBox::AcceptRole);
    auto *bAll = bb->addButton(QString::fromUtf8("Генотипировать все"), QDialogButtonBox::ActionRole);
    auto *bView = bb->addButton(QString::fromUtf8("Остановить и просмотреть"), QDialogButtonBox::RejectRole);
    connect(bPass, &QPushButton::clicked, &dlg, [&] { answer = "passing"; dlg.accept(); });
    connect(bAll, &QPushButton::clicked, &dlg, [&] { answer = "all"; dlg.accept(); });
    connect(bView, &QPushButton::clicked, &dlg, [&] { answer = "view"; dlg.accept(); });
    lay->addWidget(bb);
    dlg.exec();
    return answer;
}

// A cell of the Genotypes table: numbers sort as numbers, quality icons by their value.
class GenoItem : public QTableWidgetItem {
public:
    using QTableWidgetItem::QTableWidgetItem;
    bool operator<(const QTableWidgetItem &o) const override {
        const QString a = text().isEmpty() ? data(Qt::UserRole).toString() : text(), b = o.text().isEmpty() ? o.data(Qt::UserRole).toString() : o.text();
        bool x = false, y = false;
        const double da = a.toDouble(&x), db = b.toDouble(&y);
        if (x && y) return da < db;
        return QString::localeAwareCompare(a, b) < 0;
    }
};

// Genotypes table: the columns, cells, filters and order of the active table setting (computed by the backend).
void MainWindow::refreshGenotypes() {
    const auto cols = backend_->table_columns("genotypes");
    const auto rows = backend_->genotypes_table();
    static const QHash<QString, QString> DYE_TAG{{"B", "DATA1"}, {"G", "DATA2"}, {"Y", "DATA3"}, {"R", "DATA4"}, {"O", "DATA105"}, {"P", "DATA106"}};
    genoTable_->setSortingEnabled(false);
    genoTable_->clear();
    genoTable_->setColumnCount(static_cast<int>(cols.size()));
    for (std::size_t c = 0; c < cols.size(); ++c) {
        auto *h = new QTableWidgetItem(q(cols[c].head));
        h->setToolTip(q(cols[c].title));
        genoTable_->setHorizontalHeaderItem(static_cast<int>(c), h);
    }
    genoTotal_ = static_cast<int>(rows.size());
    genoShown_ = 0;
    for (const auto &g : rows) if (g.shown) ++genoShown_;
    genoTable_->setRowCount(genoShown_);
    int i = 0;
    for (const auto &g : rows) {
        if (!g.shown) continue;
        for (std::size_t c = 0; c < cols.size() && c < g.cells.size(); ++c) {
            const QString kind = q(cols[c].kind), id = q(cols[c].id), v = q(g.cells[c]);
            auto *it = new GenoItem;
            if (kind == "pqv") {
                it->setIcon(qualityIcon(v));
                const QString tip = c < g.tips.size() ? q(g.tips[c]) : QString();
                it->setToolTip(id + ": " + v + (tip.isEmpty() ? QString() : " (" + tip + ")"));
                it->setData(Qt::UserRole, v);
            } else {
                it->setText(v);
                if (kind == "num") it->setTextAlignment(Qt::AlignRight | Qt::AlignVCenter);
                if (kind == "mark") it->setTextAlignment(Qt::AlignCenter);
                if (id == "dye" && DYE_TAG.contains(v)) it->setIcon(colorIcon(dyeSwatch(DYE_TAG.value(v))));
            }
            it->setData(Qt::UserRole + 1, static_cast<int>(g.source));
            genoTable_->setItem(i, static_cast<int>(c), it);
        }
        ++i;
    }
    genoTable_->horizontalHeader()->setSortIndicator(-1, Qt::AscendingOrder);   // order of the table setting until a header is clicked
    genoTable_->setSortingEnabled(true);
    genoTable_->resizeColumnsToContents();
    updateTableNote();
}

void MainWindow::fillTableSettings() {
    if (!tableSetting_) return;
    const QSignalBlocker block(tableSetting_);
    tableSetting_->clear();
    for (const auto &n : backend_->table_setting_names()) tableSetting_->addItem(q(n));
    tableSetting_->setCurrentIndex(tableSetting_->findText(q(backend_->table_setting_active())));
}

// The active table setting changed (or was edited): columns, widths, font, filters and order of both tables.
void MainWindow::applyTableSetting() {
    model_->refresh();
    auto font = [this](QTableView *t, const char *kind) {
        QFont f = QApplication::font();
        const int z = backend_->table_font_size(kind);
        if (z) f.setPointSize(z);
        t->setFont(f);
        t->verticalHeader()->setDefaultSectionSize(QFontMetrics(f).height() + 6);
    };
    font(table_, "samples");
    font(genoTable_, "genotypes");
    const QFontMetrics fm(table_->font());
    const auto &cols = model_->columns();
    for (int c = 0; c < cols.size(); ++c) {
        const int w = cols[c].kind == "status" ? 130 : cols[c].id == "sampleFile" ? 280 : cols[c].width * 5 / 4;
        table_->setColumnWidth(c, std::max(w, fm.horizontalAdvance(cols[c].head) + 26));
    }
    {
        const QSignalBlocker block(table_->horizontalHeader());
        table_->horizontalHeader()->setSortIndicatorShown(false);
        table_->horizontalHeader()->setSortIndicator(0, Qt::AscendingOrder);
    }
    proxy_->setSettingOrder(true);
    proxy_->sort(0, Qt::AscendingOrder);
    proxy_->reapply();
    refreshGenotypes();
}

// "Filtered by table setting «name» (columns): shown / all" for the table of the current tab
void MainWindow::updateTableNote() {
    if (!tableNote_) return;
    const bool geno = tabs_->currentWidget() == genoTable_;
    QStringList ids;
    for (const auto &id : backend_->table_filter_columns(geno ? "genotypes" : "samples")) ids << q(id);
    if (ids.isEmpty()) { tableNote_->clear(); return; }
    tableNote_->setText(QString::fromUtf8("Отфильтровано настройкой таблицы «%1» (%2): %3 / %4")
                            .arg(q(backend_->table_setting_active()), ids.join(", "))
                            .arg(geno ? genoShown_ : model_->shownCount())
                            .arg(geno ? genoTotal_ : model_->rowCount()));
}

void MainWindow::editTableSetting() {
    const QString name = q(backend_->table_setting_active());
    const QByteArray nb = name.toUtf8();
    TableSettingEditor ed(*backend_, parseJsonObject(q(backend_->table_setting_json(rust::Str(nb.constData(), nb.size())))), name, this);
    if (ed.exec() != QDialog::Accepted) return;
    const QByteArray sb = ed.savedName().toUtf8();
    backend_->table_setting_set_active(rust::Str(sb.constData(), sb.size()));
    fillTableSettings();
    applyTableSetting();
}

void MainWindow::updateTitle() {
    const QString name = projectPath_.isEmpty() ? QString::fromUtf8("без имени") : QFileInfo(projectPath_).completeBaseName();
    setWindowTitle(QString::fromUtf8("OpenTrace Desktop — ") + name + (dirty_ ? " *" : ""));
}

void MainWindow::markDirty() {
    dirty_ = true;
    updateTitle();
}

// true when the current project may be replaced (saved, discarded, or nothing to lose)
bool MainWindow::confirmDiscard() {
    if (!dirty_ || backend_->sample_count() == 0) return true;
    const auto b = QMessageBox::question(this, "OpenTrace", QString::fromUtf8("Сохранить изменения проекта?"), QMessageBox::Save | QMessageBox::Discard | QMessageBox::Cancel);
    if (b == QMessageBox::Cancel) return false;
    if (b == QMessageBox::Save) return saveProject(false);
    return true;
}

bool MainWindow::saveProject(bool as) {
    if (backend_->analysis_progress().running) { QMessageBox::information(this, "OpenTrace", QString::fromUtf8("Дождитесь окончания анализа.")); return false; }
    QString path = projectPath_;
    if (as || path.isEmpty()) {
        QSettings st;
        path = QFileDialog::getSaveFileName(this, QString::fromUtf8("Сохранить проект"), st.value("main/projectDir").toString() + "/project.otproj", QString::fromUtf8("Проект OpenTrace (*.otproj)"));
        if (path.isEmpty()) return false;
        if (!path.endsWith(".otproj", Qt::CaseInsensitive)) path += ".otproj";
        st.setValue("main/projectDir", QFileInfo(path).absolutePath());
    }
    QApplication::setOverrideCursor(Qt::WaitCursor);
    const QString err = q(backend_->save_project_file(r(path)));
    QApplication::restoreOverrideCursor();
    if (!err.isEmpty()) { QMessageBox::warning(this, "OpenTrace", QString::fromUtf8("Проект не сохранён:\n") + err); return false; }
    projectPath_ = path;
    dirty_ = false;
    updateTitle();
    status_->setText(QString::fromUtf8("Сохранено: ") + path);
    return true;
}

void MainWindow::openProject() {
    if (backend_->analysis_progress().running || !confirmDiscard()) return;
    QSettings st;
    const QString path = QFileDialog::getOpenFileName(this, QString::fromUtf8("Открыть проект"), st.value("main/projectDir").toString(), QString::fromUtf8("Проект OpenTrace (*.otproj)"));
    if (path.isEmpty()) return;
    st.setValue("main/projectDir", QFileInfo(path).absolutePath());
    QApplication::setOverrideCursor(Qt::WaitCursor);
    const QString err = q(backend_->open_project_file(r(path)));
    QApplication::restoreOverrideCursor();
    if (!err.isEmpty()) { QMessageBox::warning(this, "OpenTrace", QString::fromUtf8("Проект не открыт:\n") + err); return; }
    projectPath_ = path;
    dirty_ = false;
    model_->refresh();
    refreshNavigator();
    refreshGenotypes();
    refreshSummary();
    updateInfo();
    updateTitle();
    status_->setText(QString::fromUtf8("Открыт проект: %1 (образцов: %2)").arg(path).arg(backend_->sample_count()));
}


// Label Edit Viewer: every label edit of the project (newest first), undo of one edit, export.
void MainWindow::labelEditViewer() {
    QDialog dlg(this);
    dlg.setWindowTitle(QString::fromUtf8("Журнал правок обозначений"));
    dlg.resize(1100, 460);
    auto *lay = new QVBoxLayout(&dlg);
    auto *tbl = new QTableWidget(0, 11);
    tbl->setHorizontalHeaderLabels({QString::fromUtf8("Файл образца"), QString::fromUtf8("Имя образца"), QString::fromUtf8("Маркер"), QString::fromUtf8("Размер"),
                                    QString::fromUtf8("Было"), QString::fromUtf8("Стало"), QString::fromUtf8("Действие"), QString::fromUtf8("Причина изменения"),
                                    QString::fromUtf8("Пользователь"), QString::fromUtf8("Дата"), QString::fromUtf8("Отменено")});
    tbl->setEditTriggers(QAbstractItemView::NoEditTriggers);
    tbl->setSelectionBehavior(QAbstractItemView::SelectRows);
    tbl->setSelectionMode(QAbstractItemView::SingleSelection);
    tbl->horizontalHeader()->setStretchLastSection(true);
    static const QHash<QString, QString> act{{"rename", "переименование"}, {"delete", "удаление"}, {"add", "добавление аллели"}, {"move", "перенос"},
                                              {"to_artifact", "в артефакт"}, {"add_artifact", "добавление артефакта"}, {"rename_artifact", "переименование артефакта"},
                                              {"delete_artifact", "удаление артефакта"}, {"artifact_to_allele", "артефакт в аллель"}, {"override_gq", "GQ вручную"},
                                              {"override_cgq", "CGQ вручную"}, {"pli", "PLI"}};
    QVector<QPair<std::size_t, std::size_t>> keys;
    auto fill = [&] {
        tbl->setRowCount(0);
        keys.clear();
        for (const auto &e : backend_->edit_list()) {
            const int r0 = tbl->rowCount();
            tbl->insertRow(r0);
            const QStringList v{q(e.file), q(e.name), q(e.marker), q(e.size), q(e.old), q(e.new_label), act.value(q(e.action), q(e.action)), q(e.reason), q(e.user), q(e.at),
                                e.undone ? QString::fromUtf8("да") : QString()};
            for (int c = 0; c < v.size(); ++c) {
                auto *it = new QTableWidgetItem(v[c]);
                if (e.undone) it->setForeground(QColor(140, 140, 140));
                tbl->setItem(r0, c, it);
            }
            keys.push_back({e.sample, e.index});
        }
        tbl->resizeColumnsToContents();
    };
    fill();
    lay->addWidget(tbl, 1);
    auto *row = new QHBoxLayout;
    auto *undo = new QPushButton(QString::fromUtf8("Отменить правку"));
    connect(undo, &QPushButton::clicked, &dlg, [&] {
        const int r0 = tbl->currentRow();
        if (r0 < 0) return;
        if (backend_->undo_edit_at(keys[r0].first, keys[r0].second)) { fill(); refreshGenotypes(); markDirty(); }
    });
    auto *exp = new QPushButton(QString::fromUtf8("Экспорт..."));
    connect(exp, &QPushButton::clicked, &dlg, [&] {
        const QString path = QFileDialog::getSaveFileName(&dlg, QString::fromUtf8("Экспорт журнала правок"), "label_edits.txt", QString::fromUtf8("Текст с табуляцией (*.txt)"));
        if (path.isEmpty()) return;
        QFile f(path);
        if (!f.open(QIODevice::WriteOnly | QIODevice::Text)) return;
        QTextStream out(&f);
        out.setEncoding(QStringConverter::Utf8);
        QStringList head;
        for (int c = 0; c < tbl->columnCount(); ++c) head << tbl->horizontalHeaderItem(c)->text();
        out << head.join('\t') << "\n";
        for (int i = 0; i < tbl->rowCount(); ++i) {
            QStringList r1;
            for (int c = 0; c < tbl->columnCount(); ++c) r1 << tbl->item(i, c)->text();
            out << r1.join('\t') << "\n";
        }
    });
    row->addWidget(undo);
    row->addWidget(exp);
    row->addStretch(1);
    auto *close = new QPushButton(QString::fromUtf8("Закрыть"));
    connect(close, &QPushButton::clicked, &dlg, &QDialog::accept);
    row->addWidget(close);
    lay->addLayout(row);
    if (tbl->rowCount() == 0) lay->insertWidget(0, new QLabel(QString::fromUtf8("Правок меток в проекте нет.")));
    dlg.exec();
}


void MainWindow::filterRows(const QSet<int> &rows, const QString &label) {
    const bool on = !label.isEmpty();
    proxy_->setRows(rows, on);
    filterBar_->setVisible(on);
    filterText_->setText(QString::fromUtf8("Отфильтровано: %1 (%2)").arg(label).arg(rows.size()));
    if (on) tabs_->setCurrentIndex(0);
}

// Analysis Summary (ID-X): sample status, allelic ladders per run folder, controls, samples, genotyping notes,
// instruments. Every number is a link that filters the Samples table to those samples.
void MainWindow::refreshSummary() {
    QString run = summaryRun_ ? summaryRun_->currentData().toString() : QString();
    while (QLayoutItem *it = summaryLay_->takeAt(0)) {
        if (it->widget()) it->widget()->deleteLater();
        delete it;
    }
    summaryRun_ = nullptr;
    const QByteArray rb = run.toUtf8();
    const QJsonObject d = QJsonDocument::fromJson(q(backend_->summary_json(rust::Str(rb.constData(), rb.size()))).toUtf8()).object();
    auto h2 = [&](const QString &t, int size) {
        auto *l = new QLabel(t);
        QFont f = l->font();
        f.setBold(true);
        f.setPointSizeF(f.pointSizeF() + size);
        l->setFont(f);
        summaryLay_->addWidget(l);
    };
    auto ids = [](const QJsonValue &v) { QSet<int> s; for (const auto &x : v.toArray()) s.insert(x.toInt()); return s; };
    auto link = [this, ids](const QJsonValue &v, const QString &label) {
        const QSet<int> rows = ids(v);
        auto *l = new QLabel(rows.isEmpty() ? QString("0") : QString("<a href='x'>%1</a>").arg(rows.size()));
        l->setAlignment(Qt::AlignCenter);
        connect(l, &QLabel::linkActivated, this, [this, rows, label] { filterRows(rows, label); });
        return l;
    };
    auto table = [&](const QStringList &head, const QList<QList<QWidget *>> &rows) {
        auto *t = new QTableWidget(rows.size(), head.size());
        t->setHorizontalHeaderLabels(head);
        t->verticalHeader()->hide();
        t->setEditTriggers(QAbstractItemView::NoEditTriggers);
        t->setSelectionMode(QAbstractItemView::NoSelection);
        for (int r0 = 0; r0 < rows.size(); ++r0)
            for (int c = 0; c < rows[r0].size(); ++c) t->setCellWidget(r0, c, rows[r0][c]);
        t->resizeColumnsToContents();
        for (int c = 0; c < head.size(); ++c) t->setColumnWidth(c, std::max(t->columnWidth(c) + 24, 110));
        t->setFixedHeight(t->horizontalHeader()->height() + rows.size() * t->verticalHeader()->defaultSectionSize() + 4);
        t->setSizeAdjustPolicy(QAbstractScrollArea::AdjustToContents);
        auto *wrap = new QWidget;
        auto *hl = new QHBoxLayout(wrap);
        hl->setContentsMargins(0, 0, 0, 0);
        hl->addWidget(t);
        hl->addStretch(1);
        summaryLay_->addWidget(wrap);
    };
    auto text = [](const QString &s) { auto *l = new QLabel(s); l->setContentsMargins(6, 0, 6, 0); return l; };
    h2(QString::fromUtf8("Сводка анализа"), 3);
    {
        auto *row = new QWidget;
        auto *hl = new QHBoxLayout(row);
        hl->setContentsMargins(0, 0, 0, 0);
        hl->addWidget(new QLabel(QString::fromUtf8("Папка запуска:")));
        summaryRun_ = new QComboBox;
        summaryRun_->addItem(QString::fromUtf8("Все папки запуска"), QString());
        for (const auto &r0 : d["runs"].toArray()) summaryRun_->addItem(r0.toString(), r0.toString());
        summaryRun_->setCurrentIndex(std::max(0, summaryRun_->findData(run)));
        summaryRun_->setEnabled(summaryRun_->count() > 2);
        connect(summaryRun_, qOverload<int>(&QComboBox::currentIndexChanged), this, [this] { QTimer::singleShot(0, this, [this] { refreshSummary(); }); });
        hl->addWidget(summaryRun_);
        hl->addStretch(1);
        summaryLay_->addWidget(row);
    }
    const QJsonObject st = d["status"].toObject();
    table({QString::fromUtf8("Статус образца"), QString::fromUtf8("Всего образцов")},
          {{text(QString::fromUtf8("Не проанализирован")), link(st["unanalyzed"], QString::fromUtf8("не проанализированы"))},
           {text(QString::fromUtf8("Проанализирован")), link(st["analyzed"], QString::fromUtf8("проанализированы"))}});
    h2(QString::fromUtf8("Качество аллельных лестниц по папкам запуска (по SQ и CGQ)"), 1);
    summaryLay_->addWidget(new QLabel(QString::fromUtf8("Бины смещаются только по лестницам с зелёными SQ и CGQ.")));
    QList<QList<QWidget *>> lr;
    for (const auto &lv : d["ladders"].toArray()) {
        const QJsonObject l = lv.toObject();
        const QString rf = l["run"].toString();
        lr.push_back({text(rf), link(l["all"], rf), link(l["used"], rf), link(l["unused"], rf), link(l["low"], rf)});
    }
    table({QString::fromUtf8("Папка запуска"), QString::fromUtf8("Всего"), QString::fromUtf8("SQ и CGQ годны (смещение бинов)"), QString::fromUtf8("SQ и CGQ годны (не для смещения)"),
           QString::fromUtf8("SQ или CGQ не годны")}, lr);
    auto flagTable = [&](const QString &first, const QJsonArray &rowsJ) {
        QList<QList<QWidget *>> rr;
        for (const auto &rv : rowsJ) {
            const QJsonObject r1 = rv.toObject();
            const QString lbl = r1["label"].toString();
            rr.push_back({text(lbl), link(r1["all"], lbl), link(r1["ok"], lbl), link(r1["bad"], lbl)});
        }
        table({first, QString::fromUtf8("Всего"), QString::fromUtf8("Все пороги соблюдены"), QString::fromUtf8("Один или более порогов не соблюдены")}, rr);
    };
    h2(QString::fromUtf8("Качество контролей (по PQV образца: SOS, SSPK, SQ, CGQ)"), 1);
    flagTable(QString::fromUtf8("Тип контроля"), d["controls"].toArray());
    h2(QString::fromUtf8("Качество образцов (по PQV образца: SOS, SSPK, SQ, CGQ)"), 1);
    flagTable(QString(), d["samples"].toArray());
    if (!d["skipped"].toArray().isEmpty() || !d["reference_bins"].toArray().isEmpty()) {
        h2(QString::fromUtf8("Генотипирование"), 1);
        table({QString(), QString::fromUtf8("Всего")},
              {{text(QString::fromUtf8("Не генотипированы: в папке нет годной лестницы")), link(d["skipped"], QString::fromUtf8("пропущены"))},
               {text(QString::fromUtf8("Генотипированы по эталонным бинам (без смещения)")), link(d["reference_bins"], QString::fromUtf8("эталонные бины"))}});
    }
    h2(QString::fromUtf8("Приборы"), 1);
    QList<QList<QWidget *>> ir;
    for (const auto &iv : d["instruments"].toArray()) {
        const QJsonObject o = iv.toObject();
        ir.push_back({text(o["name"].toString()), link(o["samples"], o["name"].toString())});
    }
    table({QString::fromUtf8("Прибор (модель и серийный номер из файла)"), QString::fromUtf8("Образцов")}, ir);
    summaryLay_->addStretch(1);
}


// Sample Info / Raw Data / EPT Data of the first selected sample (only the tab that is shown is read from the file).
void MainWindow::refreshSampleViews() {
    QWidget *cur = tabs_->currentWidget();
    if (cur != infoTree_ && cur != rawPage_ && cur != eptPage_) return;
    const auto rows = selectedRows();
    const int i = rows.isEmpty() ? -1 : rows.first();
    const QString title = i < 0 ? QString::fromUtf8("Выделите образец в таблице «Образцы».") : model_->row(i).file + QString::fromUtf8(" — ") + model_->row(i).name;
    if (cur == infoTree_) {
        infoTree_->clear();
        if (i < 0) return;
        QHash<QString, QTreeWidgetItem *> secs;
        for (const auto &line : backend_->sample_info_rows(static_cast<std::size_t>(i))) {
            const QStringList c = q(line).split('\t');
            if (c.size() < 3) continue;
            auto *&sec = secs[c[0]];
            if (!sec) { sec = new QTreeWidgetItem(infoTree_, {c[0].isEmpty() ? QString::fromUtf8("Ошибка") : c[0]}); QFont f = sec->font(0); f.setBold(true); sec->setFont(0, f); }
            new QTreeWidgetItem(sec, {c[1], c[2]});
        }
        infoTree_->expandAll();
        infoTree_->resizeColumnToContents(0);
    } else if (cur == rawPage_) {
        rawTitle_->setText(title);
        while (QLayoutItem *it = rawLegend_->layout()->takeAt(0)) { if (it->widget()) it->widget()->deleteLater(); delete it; }
        QVector<SimplePlot::Series> ser;
        if (i >= 0)
            for (const auto &t : backend_->sample_raw(static_cast<std::size_t>(i))) {
                SimplePlot::Series sv{q(t.tag) + " " + q(t.dye), dyeColor(q(t.tag)), {}, true};
                if (q(t.tag) == "DATA3") sv.color = QColor(0x22, 0x22, 0x22);
                sv.ys.reserve(static_cast<int>(t.ys.size()));
                for (double y : t.ys) sv.ys << y;
                ser << sv;
            }
        for (int k = 0; k < ser.size(); ++k) {
            auto *cb = new QCheckBox(ser[k].name);
            cb->setChecked(true);
            cb->setStyleSheet(QString("color:%1").arg(ser[k].color.name()));
            connect(cb, &QCheckBox::toggled, this, [this, k](bool on) { rawPlot_->setVisible(k, on); });
            rawLegend_->layout()->addWidget(cb);
        }
        rawPlot_->setSeries(ser, QString::fromUtf8("точки данных"));
    } else {
        while (QLayoutItem *it = eptLay_->takeAt(0)) { if (it->widget()) it->widget()->deleteLater(); delete it; }
        auto *t0 = new QLabel(title);
        eptLay_->addWidget(t0);
        if (i < 0) { eptLay_->addStretch(1); return; }
        static const QHash<QString, QColor> col{{"voltage", QColor(0xd3, 0x20, 0x20)}, {"current", QColor(0x1a, 0x9b, 0x2e)}, {"power", QColor(0x1f, 0x4f, 0xd6)}, {"temperature", QColor(0xe6, 0x7e, 0x22)}};
        const auto ept = backend_->sample_ept(static_cast<std::size_t>(i));
        if (ept.empty()) eptLay_->addWidget(new QLabel(QString::fromUtf8("В файле нет данных EPT.")));
        for (const auto &t : ept) {
            double mn = 0, mx = 0;
            SimplePlot::Series sv{q(t.dye), col.value(q(t.tag), Qt::black), {}, true};
            for (double y : t.ys) { sv.ys << y; mn = std::min(mn, y); mx = std::max(mx, y); }
            auto *lab = new QLabel(QString::fromUtf8("%1 · мин %2 · макс %3").arg(sv.name).arg(mn).arg(mx));
            lab->setStyleSheet(QString("color:%1;font-weight:bold").arg(sv.color.name()));
            eptLay_->addWidget(lab);
            auto *pl = new SimplePlot;
            pl->setSeries({sv}, QString::fromUtf8("точки EPT"));
            eptLay_->addWidget(pl, 1);
        }
    }
}

} // namespace ot

