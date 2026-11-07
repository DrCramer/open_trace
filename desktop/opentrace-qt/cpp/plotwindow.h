// Plot window: electropherograms of the selected samples drawn natively with QPainter (screen and vector printing).
#pragma once
#include <QMainWindow>
#include <QFontMetricsF>
#include <QRectF>
#include <QWidget>
#include <QVector>
#include <QPointF>
#include <QSet>
#include <QStringList>
#include <memory>
#include <functional>

class QScrollArea;
class QVBoxLayout;
class QLabel;
class QSpinBox;
class QPainter;
class QPagedPaintDevice;
class QTableWidget;
class QSplitter;

namespace ot {
struct Backend;

struct TraceData { QString tag; QVector<QPointF> pts; };
struct BinData { double lo, hi; bool virt; };
struct MarkerData { QString name, tag, gq; double lo, hi; QVector<double> window; QVector<BinData> bins; };
struct LabelData { QString tag, text; double x, height; bool flagged; QString id, marker, kind; bool edited; };
struct PeakData { QString tag; double x, height, scan, size, area; };
struct PaneData {
    QString file, name, sq;
    QVector<TraceData> traces;   // x in bp (or data points)
    QVector<MarkerData> markers; // kit markers (bp only)
    QVector<LabelData> labels;   // allele labels
    QVector<PeakData> peaks;     // detected peaks (adding labels)
    int sample = -1;             // row of the sample in the project
    double lo = 0, hi = 0;        // full x range of the sample
};

struct ViewState {
    bool bp = true;               // X axis in basepairs (else data points)
    double x0 = 0, x1 = 0;        // visible X range; x1 <= x0: full view
    QSet<QString> dyes;           // shown dyes
    bool bins = true, markerRange = true, labels = true, indicators = true;
    int panes = 2;                // panes per window height
    bool separate = true;         // one pane per dye (ID-X "Separate"), else all dyes of a sample in one pane
};

struct PaneGeometry { QRectF head; double L = 0, R = 0, markerTop = 0, markerH = 0, top = 0, bottom = 0; };
PaneGeometry paneGeometry(const QRectF &rect, const QFontMetricsF &fm, double gap, bool markers, int labelRows, double rowH);

// Draws one pane (header, axes, traces) into rect; the same code paints the screen and the printer (vector).
// Where things were drawn (mouse hit tests of the screen panes).
struct PaneHits {
    QVector<QPair<QRectF, int>> labels;   // label box -> index in PaneData::labels
    QVector<QPair<QRectF, QString>> markers;   // marker header -> marker name
    QRectF head;                          // sample header
    double L = 0, R = 0, top = 0, bottom = 0, ymax = 1;
};
// selection drawn by the pane: label index (-1 none), peak index (-1 none)
struct PaneSelection { int label = -1; int peak = -1; };
void paintPane(QPainter &p, const QRectF &rect, const PaneData &d, const ViewState &v, double x0, double x1, bool print,
               PaneHits *hits = nullptr, const PaneSelection *sel = nullptr);

class PlotWindow;
class PaneWidget : public QWidget {
    Q_OBJECT
public:
    PaneWidget(PlotWindow *win, int index);
    QSize sizeHint() const override;
protected:
    void paintEvent(QPaintEvent *) override;
    void mousePressEvent(QMouseEvent *) override;
    void contextMenuEvent(QContextMenuEvent *) override;
    void mouseMoveEvent(QMouseEvent *) override;
    void mouseReleaseEvent(QMouseEvent *) override;
    void mouseDoubleClickEvent(QMouseEvent *) override;
    void wheelEvent(QWheelEvent *) override;
private:
    PlotWindow *win_;
    int index_;
    int dragFrom_ = -1, dragTo_ = -1;
    PaneHits hits_;
    bool hitTest(const QPointF &pt, int &label, int &peak) const;
    double xAt(double px) const;
};

class PlotWindow : public QMainWindow {
    Q_OBJECT
public:
    PlotWindow(const Backend &backend, const QVector<int> &rows, QWidget *parent = nullptr);
    // the project window refreshes its tables after a label edit made here
    std::function<void()> onEdited;
    PaneSelection selection(int item) const { return selItem_ == item ? sel_ : PaneSelection{}; }
    void select(int item, int label, int peak);
    void editMenu(int item, const QPoint &globalPos);
    void editSelected(const QString &action);
    void headerMenu(int item, const QString &marker, const QPoint &globalPos);
    // a pane on screen / paper: one sample, all its shown dyes or one dye
    struct PaneItem { int data; QString dye; };
    const PaneData &paneData(int item) const { return panes_[items_[item].data]; }
    ViewState paneView(int item) const;
    int paneCount() const { return items_.size(); }
    const ViewState &view() const { return view_; }
    void visibleRange(double &x0, double &x1) const;
    void zoomTo(double x0, double x1);
    void fullView();
    void showCursor(double x, double y);
    bool savePdf(const QString &path);
protected:
    void resizeEvent(QResizeEvent *) override;
    void showEvent(QShowEvent *) override;
private:
    const Backend &backend_;
    QVector<int> rows_;
    QVector<PaneData> panes_;
    QVector<PaneItem> items_;
    void rebuildPanes();
    ViewState view_;
    QScrollArea *scroll_ = nullptr;
    QWidget *host_ = nullptr;
    QVBoxLayout *layout_ = nullptr;
    QLabel *xy_ = nullptr;
    QSpinBox *panesBox_ = nullptr;
    void load();
    QTableWidget *lower_ = nullptr;
    QSplitter *split_ = nullptr;
    QString table_ = "genotypes";   // none | sizing | genotypes (ID-X Ctrl+Q / Ctrl+A / Ctrl+W)
    void fillLower();
    int selItem_ = -1;
    PaneSelection sel_;
    bool editLabel(int item, int label, const QString &action);
    bool addAtPeak(int item, int peak, bool artifact);
    void afterEdit();
    QString askReason(const QString &what);
    void buildMenus();
    void relayout();
    void repaintPanes();
    void print(bool preview);
    void exportPdf();
    void renderPages(QPagedPaintDevice *dev, QPainter &p);
};
}
