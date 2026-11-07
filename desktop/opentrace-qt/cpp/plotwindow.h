// Plot window: electropherograms of the selected samples drawn natively with QPainter (screen and vector printing).
#pragma once
#include <QMainWindow>
#include <QWidget>
#include <QVector>
#include <QPointF>
#include <QSet>
#include <QStringList>
#include <memory>

class QScrollArea;
class QVBoxLayout;
class QLabel;
class QSpinBox;
class QPainter;
class QPagedPaintDevice;

namespace ot {
struct Backend;

struct TraceData { QString tag; QVector<QPointF> pts; };
struct PaneData {
    QString file, name, sq;
    QVector<TraceData> traces;   // x in bp (or data points)
    double lo = 0, hi = 0;        // full x range of the sample
};

struct ViewState {
    bool bp = true;               // X axis in basepairs (else data points)
    double x0 = 0, x1 = 0;        // visible X range; x1 <= x0: full view
    QSet<QString> dyes;           // shown dyes
    int panes = 2;                // panes per window height
};

// Draws one pane (header, axes, traces) into rect; the same code paints the screen and the printer (vector).
void paintPane(QPainter &p, const QRectF &rect, const PaneData &d, const ViewState &v, double x0, double x1, bool print);

class PlotWindow;
class PaneWidget : public QWidget {
    Q_OBJECT
public:
    PaneWidget(PlotWindow *win, int index);
    QSize sizeHint() const override;
protected:
    void paintEvent(QPaintEvent *) override;
    void mousePressEvent(QMouseEvent *) override;
    void mouseMoveEvent(QMouseEvent *) override;
    void mouseReleaseEvent(QMouseEvent *) override;
    void mouseDoubleClickEvent(QMouseEvent *) override;
    void wheelEvent(QWheelEvent *) override;
private:
    PlotWindow *win_;
    int index_;
    int dragFrom_ = -1, dragTo_ = -1;
    double xAt(double px) const;
};

class PlotWindow : public QMainWindow {
    Q_OBJECT
public:
    PlotWindow(const Backend &backend, const QVector<int> &rows, QWidget *parent = nullptr);
    const PaneData &pane(int i) const { return panes_[i]; }
    int paneCount() const { return panes_.size(); }
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
    ViewState view_;
    QScrollArea *scroll_ = nullptr;
    QWidget *host_ = nullptr;
    QVBoxLayout *layout_ = nullptr;
    QLabel *xy_ = nullptr;
    QSpinBox *panesBox_ = nullptr;
    void load();
    void buildMenus();
    void relayout();
    void repaintPanes();
    void print(bool preview);
    void exportPdf();
    void renderPages(QPagedPaintDevice *dev, QPainter &p);
};
}
