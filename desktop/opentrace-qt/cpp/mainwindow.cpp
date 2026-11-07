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
    QSettings().setValue("main/geometry", saveGeometry());
    e->accept();
}

void MainWindow::buildMenus() {
    QStyle *s = style();
    auto *file = menuBar()->addMenu(QString::fromUtf8("&Файл"));
    auto *aNew = file->addAction(s->standardIcon(QStyle::SP_FileIcon), QString::fromUtf8("&Новый проект"), this, [this] {
        if (backend_->analysis_progress().running) return;
        std::vector<std::size_t> all;
        rust::Vec<std::size_t> rows;
        for (std::size_t i = 0; i < backend_->sample_count(); ++i) rows.push_back(i);
        backend_->remove_samples(rows);
        model_->refresh();
        refreshNavigator();
        updateInfo();
    });
    aNew->setShortcut(QKeySequence::New);
    auto *aAdd = file->addAction(s->standardIcon(QStyle::SP_DialogOpenButton), QString::fromUtf8("&Добавить образцы в проект..."), this, [this] { addSamples(); });
    aAdd->setShortcut(QKeySequence("Ctrl+K"));
    auto *aExport = file->addAction(s->standardIcon(QStyle::SP_DialogSaveButton), QString::fromUtf8("&Экспорт таблицы..."), this, [this] { exportTable(); });
    aExport->setShortcut(QKeySequence("Ctrl+E"));
    file->addSeparator();
    file->addAction(QString::fromUtf8("В&ыход"), QKeySequence::Quit, this, &QWidget::close);

    auto *edit = menuBar()->addMenu(QString::fromUtf8("&Правка"));
    edit->addAction(QString::fromUtf8("Выделить &всё"), QKeySequence::SelectAll, table_, &QTableView::selectAll);
    auto *aDel = edit->addAction(s->standardIcon(QStyle::SP_TrashIcon), QString::fromUtf8("&Удалить из проекта"), this, [this] { removeSelected(); });
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
    auto *aNav = view->addAction(QString::fromUtf8("&Навигатор"));
    aNav->setCheckable(true);
    aNav->setChecked(true);
    aNav->setShortcut(QKeySequence("Ctrl+Shift+N"));
    connect(aNav, &QAction::toggled, nav_, &QWidget::setVisible);

    auto *plots = menuBar()->addMenu(QString::fromUtf8("&Графики"));
    auto *aPlots = plots->addAction(s->standardIcon(QStyle::SP_ComputerIcon), QString::fromUtf8("Показать &графики"), this, [this] { showPlots(); });
    aPlots->setShortcut(QKeySequence("Ctrl+L"));

    auto *tools = menuBar()->addMenu(QString::fromUtf8("&Инструменты"));
    tools->addAction(QString::fromUtf8("Импорт &метода анализа (GeneMapper XML)..."), this, [this] { importLibraryFile(true); });
    tools->addAction(QString::fromUtf8("Импорт &стандарта длин (GeneMapper XML)..."), this, [this] { importLibraryFile(false); });

    auto *help = menuBar()->addMenu(QString::fromUtf8("&Справка"));
    help->addAction(QString::fromUtf8("&О программе"), this, [this] { about(); });

    auto *tb = addToolBar(QString::fromUtf8("Основная"));
    tb->setObjectName("mainToolbar");
    tb->setIconSize(QSize(18, 18));
    tb->addAction(aNew);
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
    auto errors = backend_->add_sample_files(paths, r(run->text()), r(method->currentText()), r(stdBox->currentText()));
    QApplication::restoreOverrideCursor();
    model_->refresh();
    refreshNavigator();
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
        status_->setText(QString::fromUtf8("Анализ: ") + q(p.message));
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
    if (autotest_) QTimer::singleShot(300, this, [this] {
        const QString out = qEnvironmentVariable("OT_QT_AUTOTEST_OUT", ".");
        QDir().mkpath(out);
        table_->selectAll();
        grab().save(out + "/main.png");
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
        QFile f(out + "/result.json");
        if (f.open(QIODevice::WriteOnly)) {
            f.write(QString("{\"samples\":%1,\"analyzed\":%2,\"failed\":%3,\"pdf\":%4,\"rows\":[%5]}\n")
                        .arg(model_->rowCount()).arg(ok.size()).arg(failed).arg(pdf ? "true" : "false").arg(rows.join(",")).toUtf8());
        }
        QApplication::exit(ok.isEmpty() ? 1 : 0);
    });
}

// Scripted run (CI, screenshots): add OT_QT_AUTOTEST_FILES (separated by ';'), analyse with OT_QT_AUTOTEST_METHOD /
// OT_QT_AUTOTEST_STD, then save main.png, plot.png, plots.pdf and result.json into OT_QT_AUTOTEST_OUT and quit.
void MainWindow::autotest() {
    autotest_ = true;
    QStringList files = qEnvironmentVariable("OT_QT_AUTOTEST_FILES").split(';', Qt::SkipEmptyParts);
    QString synthStd;
    if (files.size() == 1 && files.first().startsWith("synthetic:")) {   // invented run files of the core (CI)
        const int n = files.first().mid(10).toInt();
        files.clear();
        for (const auto &p : backend_->autotest_synthetic(r(qEnvironmentVariable("OT_QT_AUTOTEST_OUT", ".") + "/fsa"), static_cast<std::size_t>(std::max(1, n)))) files << q(p);
        synthStd = q(synthetic_standard_name());
    }
    rust::Vec<rust::String> paths;
    for (const QString &f : files) paths.push_back(r(f));
    QString method = qEnvironmentVariable("OT_QT_AUTOTEST_METHOD"), stdName = qEnvironmentVariable("OT_QT_AUTOTEST_STD");
    if (qEnvironmentVariableIsSet("OT_QT_AUTOTEST_METHOD_XML")) method = q(backend_->import_method(r(qEnvironmentVariable("OT_QT_AUTOTEST_METHOD_XML"))));
    if (qEnvironmentVariableIsSet("OT_QT_AUTOTEST_STD_XML")) stdName = q(backend_->import_size_standard(r(qEnvironmentVariable("OT_QT_AUTOTEST_STD_XML"))));
    if (method.isEmpty() && !backend_->method_list().empty()) method = q(backend_->method_list()[0]);
    if (stdName.isEmpty()) stdName = synthStd.isEmpty() ? QString("GS600_LIZ_(60-460)") : synthStd;
    backend_->add_sample_files(paths, r("autotest"), r(method), r(stdName));
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
    w->show();
}

void MainWindow::exportTable() {
    QSettings st;
    const QString path = QFileDialog::getSaveFileName(this, QString::fromUtf8("Экспорт таблицы"), st.value("main/exportDir").toString() + "/samples.txt",
                                                      QString::fromUtf8("Текст с табуляцией (*.txt);;CSV (*.csv)"));
    if (path.isEmpty()) return;
    st.setValue("main/exportDir", QFileInfo(path).absolutePath());
    QFile f(path);
    if (!f.open(QIODevice::WriteOnly | QIODevice::Text)) { QMessageBox::warning(this, "OpenTrace", f.errorString()); return; }
    const QChar sep = path.endsWith(".csv", Qt::CaseInsensitive) ? ',' : '\t';
    QTextStream out(&f);
    out.setEncoding(QStringConverter::Utf8);
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

} // namespace ot
