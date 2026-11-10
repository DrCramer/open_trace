// Project window: native menu bar, tool bar, navigator, Samples table, sample information, status bar.
#pragma once
#include <QMainWindow>
#include "rust/cxx.h"

class QTableView;
class QTreeWidget;
class QTabWidget;
class QPlainTextEdit;
class QLabel;
class QProgressBar;
class QPushButton;
class QTimer;
class QSortFilterProxyModel;
class QScrollArea;
class QComboBox;
class QVBoxLayout;
class QTableWidget;

namespace ot {
struct Backend;
class SampleModel;
class RowFilterProxy;
class SimplePlot;

class MainWindow : public QMainWindow {
    Q_OBJECT
public:
    explicit MainWindow(QWidget *parent = nullptr);
    ~MainWindow() override;
protected:
    void closeEvent(QCloseEvent *) override;
private:
    rust::Box<Backend> backend_;
    SampleModel *model_ = nullptr;
    class RowFilterProxy *proxy_ = nullptr;
    QWidget *filterBar_ = nullptr;
    QLabel *filterText_ = nullptr;
    QWidget *summary_ = nullptr;
    QVBoxLayout *summaryLay_ = nullptr;
    QComboBox *summaryRun_ = nullptr;
    void refreshSummary();
    QWidget *rawPage_ = nullptr, *eptPage_ = nullptr;
    class QTreeWidget *infoTree_ = nullptr;
    class SimplePlot *rawPlot_ = nullptr;
    QLabel *rawTitle_ = nullptr, *rawReadout_ = nullptr;
    QWidget *rawLegend_ = nullptr;
    QVBoxLayout *eptLay_ = nullptr;
    int shownSample_ = -1;
    void refreshSampleViews();
    void filterRows(const QSet<int> &rows, const QString &label);
    QTableView *table_ = nullptr;
    QTreeWidget *nav_ = nullptr;
    QTabWidget *tabs_ = nullptr;
    QPlainTextEdit *info_ = nullptr;
    QLabel *status_ = nullptr;
    QProgressBar *progress_ = nullptr;
    QPushButton *stop_ = nullptr;
    QTimer *poll_ = nullptr;
    QAction *analyzeAct_ = nullptr;
    QAction *analyzeAllAct_ = nullptr;
    void buildMenus();
    void addSamples();
    void removeSelected();
    void analyze(bool all);
    void pollProgress();
    void showPlots();
    void exportTable();
    void refreshNavigator();
    void updateInfo();
    void about();
    void importLibraryFile(bool method);
    void setPanelForSelected();
    void panelManager();
    void projectOptions();
    QString alasDialog(const QString &summaryJson);
    void refreshGenotypes();
    // Table Settings: columns, filters and order of the Samples and Genotypes tables
    QComboBox *tableSetting_ = nullptr;
    QLabel *tableNote_ = nullptr;
    int genoShown_ = 0, genoTotal_ = 0;
    void fillTableSettings();
    void applyTableSetting();
    void updateTableNote();
    void editTableSetting();
    QTableWidget *genoTable_ = nullptr;
    QString projectPath_;
    bool dirty_ = false;
    void updateTitle();
    void markDirty();
    bool confirmDiscard();
    bool saveProject(bool as);
    void openProject();
    void labelEditViewer();
    bool asking_ = false;
    QVector<int> selectedRows() const;
    void autotest();                // OT_QT_AUTOTEST_*: scripted run for CI and screenshots
    bool autotest_ = false;
};
}
