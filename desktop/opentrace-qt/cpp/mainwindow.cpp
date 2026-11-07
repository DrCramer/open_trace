#include "opentrace-qt/cpp/mainwindow.h"
#include "opentrace-qt/cpp/plotwindow.h"
#include "opentrace-qt/cpp/samplemodel.h"
#include "opentrace-qt/cpp/style.h"
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
#include <QEventLoop>
#include <QToolBar>
#include <QTreeWidget>
#include <QSet>
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
    proxy_ = new QSortFilterProxyModel(this);
    proxy_->setSourceModel(model_);
    table_ = new QTableView;
    table_->setModel(proxy_);
    table_->setSelectionBehavior(QAbstractItemView::SelectRows);
    table_->setSelectionMode(QAbstractItemView::ExtendedSelection);
    table_->setSortingEnabled(true);
    table_->horizontalHeader()->setSortIndicator(-1, Qt::AscendingOrder);   // project order until a column is clicked
    proxy_->sort(-1);
    table_->setAlternatingRowColors(true);
    table_->verticalHeader()->setDefaultSectionSize(table_->fontMetrics().height() + 6);
    table_->horizontalHeader()->setStretchLastSection(true);
    table_->setEditTriggers(QAbstractItemView::NoEditTriggers);
    table_->setColumnWidth(SampleModel::Status, 130);
    table_->setColumnWidth(SampleModel::SampleFile, 280);
    table_->setColumnWidth(SampleModel::SampleName, 140);
    table_->setColumnWidth(SampleModel::SQ, 40);
    connect(table_, &QTableView::doubleClicked, this, [this] { showPlots(); });
    connect(table_->selectionModel(), &QItemSelectionModel::selectionChanged, this, [this] { updateInfo(); });

    nav_ = new QTreeWidget;
    nav_->setHeaderHidden(true);
    nav_->setMinimumWidth(180);
    info_ = new QPlainTextEdit;
    info_->setReadOnly(true);
    tabs_ = new QTabWidget;
    tabs_->setDocumentMode(true);
    tabs_->addTab(table_, QString::fromUtf8("Образцы"));
    genoTable_ = new QTableWidget;
    genoTable_->setEditTriggers(QAbstractItemView::NoEditTriggers);
    genoTable_->setSelectionBehavior(QAbstractItemView::SelectRows);
    genoTable_->setAlternatingRowColors(true);
    genoTable_->verticalHeader()->setDefaultSectionSize(table_->fontMetrics().height() + 6);
    genoTable_->setSortingEnabled(true);
    tabs_->addTab(genoTable_, QString::fromUtf8("Генотипы"));
    tabs_->addTab(info_, QString::fromUtf8("Информация"));
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
    view->addAction(QString::fromUtf8("&Информация об образце"), QKeySequence("Ctrl+F1"), this, [this] { tabs_->setCurrentIndex(1); });
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
    markDirty();
    if (autotest_) QTimer::singleShot(300, this, [this] {
        const QString out = qEnvironmentVariable("OT_QT_AUTOTEST_OUT", ".");
        QDir().mkpath(out);
        table_->selectAll();
        grab().save(out + "/main.png");
        tabs_->setCurrentWidget(genoTable_);
        QApplication::processEvents();
        grab().save(out + "/genotypes.png");
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
        if (!ok.isEmpty()) {
            auto *w = new PlotWindow(*backend_, ok.mid(0, 3), nullptr);
            w->resize(1400, 900);
            w->show();
            QEventLoop wait;                        // let the window lay out its panes before the screenshot
            QTimer::singleShot(400, &wait, &QEventLoop::quit);
            wait.exec();
            w->grab().save(out + "/plot.png");
            pdf = w->savePdf(out + "/plots.pdf");
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
        const QString digest = q(backend_->genotype_digest());
        {
            QFile d(out + "/digest.txt");
            if (d.open(QIODevice::WriteOnly)) d.write(digest.toUtf8());
        }
        // project file round trip: save, reopen, same genotypes
        bool reopen = q(backend_->save_project_file(r(out + "/project.otproj"))).isEmpty() && q(backend_->open_project_file(r(out + "/project.otproj"))).isEmpty() &&
                      q(backend_->genotype_digest()) == digest;
        model_->refresh();
        QFile f(out + "/result.json");
        if (f.open(QIODevice::WriteOnly)) {
            f.write(QString("{\"samples\":%1,\"analyzed\":%2,\"failed\":%3,\"pdf\":%4,\"genotype_rows\":%5,\"reopen_identical\":%7,\"edit_ok\":%8,\"rows\":[%6]}\n")
                        .arg(model_->rowCount()).arg(ok.size()).arg(failed).arg(pdf ? "true" : "false").arg(genoTable_->rowCount()).arg(rows.join(",")).arg(reopen ? "true" : "false").arg(editOk ? "true" : "false").toUtf8());
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
                row << cell(it ? (it->text().isEmpty() ? it->toolTip() : it->text()) : QString());
            }
            out << row.join(sep) << "\n";
        }
        status_->setText(QString::fromUtf8("Сохранено: ") + path);
        return;
    }
    QStringList head;
    for (int c = 0; c < model_->columnCount(); ++c) head << model_->headerData(c, Qt::Horizontal, Qt::DisplayRole).toString();
    out << head.join(sep) << "\n";
    for (int i = 0; i < proxy_->rowCount(); ++i) {
        QStringList row;
        const RowView &rv = model_->row(proxy_->mapToSource(proxy_->index(i, 0)).row());
        for (int c = 0; c < model_->columnCount(); ++c) {
            QString v = c == SampleModel::SQ ? rv.sq : model_->data(model_->index(proxy_->mapToSource(proxy_->index(i, 0)).row(), c), Qt::DisplayRole).toString();
            if (sep == ',' && (v.contains(',') || v.contains('"'))) v = '"' + QString(v).replace("\"", "\"\"") + '"';
            row << v;
        }
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

void MainWindow::refreshGenotypes() {
    const auto rows = backend_->genotype_rows();
    std::size_t maxN = 2;
    for (const auto &g : rows) maxN = std::max<std::size_t>(maxN, g.alleles.size());
    maxN = std::min<std::size_t>(maxN, 8);
    QStringList head{QString::fromUtf8("Файл образца"), QString::fromUtf8("Имя образца"), QString::fromUtf8("Маркер"), QString::fromUtf8("Краситель")};
    for (std::size_t k = 1; k <= maxN; ++k) head << QString::fromUtf8("Аллель %1").arg(k);
    for (std::size_t k = 1; k <= maxN; ++k) head << QString::fromUtf8("Размер %1").arg(k);
    for (std::size_t k = 1; k <= maxN; ++k) head << QString::fromUtf8("Высота %1").arg(k);
    head << "GQ";
    genoTable_->setSortingEnabled(false);
    genoTable_->clear();
    genoTable_->setColumnCount(head.size());
    genoTable_->setHorizontalHeaderLabels(head);
    genoTable_->setRowCount(static_cast<int>(rows.size()));
    int i = 0;
    for (const auto &g : rows) {
        int c = 0;
        genoTable_->setItem(i, c++, new QTableWidgetItem(q(g.file)));
        genoTable_->setItem(i, c++, new QTableWidgetItem(q(g.name)));
        genoTable_->setItem(i, c++, new QTableWidgetItem(q(g.marker)));
        auto *dye = new QTableWidgetItem(dyeName(q(g.dye)));
        dye->setIcon(colorIcon(dyeSwatch(q(g.dye))));
        genoTable_->setItem(i, c++, dye);
        for (std::size_t k = 0; k < maxN; ++k) genoTable_->setItem(i, c++, new QTableWidgetItem(k < g.alleles.size() ? q(g.alleles[k]) : QString()));
        for (std::size_t k = 0; k < maxN; ++k) genoTable_->setItem(i, c++, new QTableWidgetItem(k < g.sizes.size() ? QString::number(g.sizes[k], 'f', 2) : QString()));
        for (std::size_t k = 0; k < maxN; ++k) genoTable_->setItem(i, c++, new QTableWidgetItem(k < g.heights.size() ? QString::number(std::lround(g.heights[k])) : QString()));
        auto *gq = new QTableWidgetItem;
        gq->setIcon(qualityIcon(q(g.gq)));
        gq->setToolTip(q(g.gq));
        genoTable_->setItem(i, c++, gq);
        ++i;
    }
    genoTable_->setSortingEnabled(true);
    genoTable_->resizeColumnsToContents();
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

} // namespace ot

