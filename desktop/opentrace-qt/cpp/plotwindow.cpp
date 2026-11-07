#include "opentrace-qt/cpp/plotwindow.h"
#include "opentrace-qt/cpp/style.h"
#include "opentrace-qt/src/bridge.cxx.h"

#include <QAction>
#include <QActionGroup>
#include <QApplication>
#include <QDateTime>
#include <QFileDialog>
#include <QFileInfo>
#include <QKeySequence>
#include <QLabel>
#include <QMenuBar>
#include <QMouseEvent>
#include <QPageLayout>
#include <QPageSize>
#include <QPainter>
#include <QPainterPath>
#include <QPdfWriter>
#include <QPrintDialog>
#include <QPrintPreviewDialog>
#include <QPrinter>
#include <QScrollArea>
#include <QScrollBar>
#include <QSettings>
#include <QSpinBox>
#include <QStatusBar>
#include <QStyle>
#include <QToolBar>
#include <QVBoxLayout>
#include <QWheelEvent>
#include <QLayoutItem>
#include <QTimer>
#include <cmath>
#include <algorithm>

namespace ot {

static const char *DYES[] = {"DATA1", "DATA2", "DATA3", "DATA4", "DATA105", "DATA106"};
static QString q(const rust::String &s) { return QString::fromUtf8(s.data(), static_cast<int>(s.size())); }

static double niceStep(double span) {
    if (span <= 0) return 1;
    const double raw = span, mag = std::pow(10.0, std::floor(std::log10(raw)));
    const double f = raw / mag;
    return (f < 1.5 ? 1 : f < 3.5 ? 2 : f < 7.5 ? 5 : 10) * mag;
}

// ------------------------------------------------------------------ drawing (screen and printer)
static QColor gqFill(const QString &q) {
    if (q == "pass") return QColor(0x00, 0x99, 0x00);
    if (q == "check") return QColor(0xff, 0x99, 0x00);
    if (q == "low") return QColor(0xcc, 0x00, 0x00);
    return QColor(0x7f, 0x7f, 0x7f);
}

// label text colour per dye (DATA3 dark yellow, readable on white)
static QColor labelColor(const QString &tag) { return tag == "DATA3" ? QColor(0x7a, 0x6a, 0x00) : dyeColor(tag); }

// Geometry of a pane shared by the painter and the mouse handlers: plot rectangle and label rows.
PaneGeometry paneGeometry(const QRectF &rect, const QFontMetricsF &fm, double gap, bool markers, int labelRows, double rowH) {
    PaneGeometry g;
    const double headH = fm.height() + gap;
    g.head = QRectF(rect.left(), rect.top(), rect.width(), headH);
    g.L = rect.left() + fm.horizontalAdvance("000000") + 2 * gap;
    g.R = rect.right() - 2 * gap;
    g.markerTop = rect.top() + headH + gap * 0.5;
    g.markerH = markers ? fm.height() + gap * 0.6 : 0;
    g.top = g.markerTop + g.markerH + fm.height() + gap;
    g.bottom = rect.bottom() - gap - labelRows * rowH;
    return g;
}

void paintPane(QPainter &p, const QRectF &rect, const PaneData &d, const ViewState &v, double x0, double x1, bool print) {
    p.save();
    p.setClipRect(rect);
    p.fillRect(rect, Qt::white);
    const QFont base = QApplication::font();
    QFont small = base;
    small.setPointSizeF(print ? 7.0 : std::max(7.5, base.pointSizeF() - 0.5));
    p.setFont(small);
    const QFontMetricsF fm(small, p.device());                  // metrics of the target device (screen or printer)
    const double mm = print ? p.device()->logicalDpiY() / 25.4 : 0;   // printer units per millimetre
    const double gap = print ? 1.2 * mm : 6;
    const bool showMarkers = v.markerRange && !d.markers.isEmpty() && v.bp;
    // allele labels: boxes in up to 4 rows under the axis, each label in the first row where it does not overlap
    QFont lf = small;
    lf.setPointSizeF(small.pointSizeF() + (print ? 0.5 : 1.0));
    const QFontMetricsF lfm(lf, p.device());
    const double rowH = lfm.height() + gap * 0.9;
    struct Placed { double x, w; int row; const LabelData *l; };
    QVector<Placed> placed;
    int rows = 0;
    auto layoutLabels = [&](double L, double R, int maxRows) {
        placed.clear();
        rows = 0;
        if (!v.labels) return;
        for (const auto &l : d.labels) {
            if (!v.dyes.contains(l.tag) || l.x < x0 || l.x > x1) continue;
            const double x = L + (l.x - x0) / (x1 - x0) * (R - L);
            const double w = std::max(lfm.height() * 1.2, lfm.horizontalAdvance(l.text) + gap * 1.2);
            int row = 0;
            while (row < maxRows - 1 && std::any_of(placed.begin(), placed.end(), [&](const Placed &q) { return q.row == row && std::abs(q.x - x) < (q.w + w) / 2 + 2; })) ++row;
            placed.push_back({x, w, row, &l});
            rows = std::max(rows, row + 1);
        }
    };
    PaneGeometry g0 = paneGeometry(rect, fm, gap, showMarkers, 0, rowH);
    layoutLabels(g0.L, g0.R, 4);
    // as many label rows as fit while the trace keeps a usable height (35 mm-ish printed / 50 px on screen)
    const double minTrace = print ? 12 * mm : 50;
    int maxRows = std::max(0, std::min(4, int((g0.bottom - g0.top - minTrace) / rowH)));
    if (rows > maxRows) layoutLabels(g0.L, g0.R, std::max(1, maxRows));
    const PaneGeometry g = paneGeometry(rect, fm, gap, showMarkers, rows, rowH);
    const double L = g.L, R = g.R, top = g.top, bottom = g.bottom;
    // header: sample name and file (sample name is what ID-X prints as the title)
    p.fillRect(g.head, print ? QColor(255, 255, 255) : QColor(236, 239, 244));
    p.setPen(QColor(40, 40, 40));
    QString title = d.name.isEmpty() ? d.file : d.name + QStringLiteral("   ") + d.file;
    p.drawText(g.head.adjusted(gap, 0, -gap, 0), Qt::AlignVCenter | Qt::AlignLeft, title);
    if (bottom - top < 10 || R - L < 20) { p.restore(); return; }
    auto X = [&](double x) { return L + (x - x0) / (x1 - x0) * (R - L); };
    // y range: maximum of the shown dyes in the visible range
    double ymax = 1;
    for (const auto &t : d.traces) {
        if (!v.dyes.contains(t.tag)) continue;
        for (const auto &pt : t.pts) if (pt.x() >= x0 && pt.x() <= x1 && pt.y() > ymax) ymax = pt.y();
    }
    ymax *= 1.08;
    auto Y = [&](double y) { return bottom - std::min(y, ymax * 1.2) / ymax * (bottom - top); };
    QPen axis(Qt::black);
    axis.setWidthF(print ? 0.2 * mm : 1.0);
    axis.setCosmetic(!print);
    // marker headers (filled with the GQ colour) and bins of the shown dyes
    for (const auto &m : d.markers) {
        if (!v.dyes.contains(m.tag)) continue;
        const double xa = X(m.lo), xb = X(m.hi);
        if (xb < L || xa > R) continue;
        if (showMarkers) {
            const QRectF hr(xa, g.markerTop, xb - xa, g.markerH);
            p.fillRect(hr, gqFill(m.gq));
            p.setPen(axis);
            p.drawRect(hr);
            p.setPen(Qt::black);
            const QString name = fm.horizontalAdvance(m.name) <= xb - xa - 4 ? m.name : QStringLiteral("...");
            p.drawText(hr, Qt::AlignCenter, name);
        }
        if (v.bins && v.bp)
            for (const auto &b : m.bins) {
                const double ba = X(b.lo), bb = X(b.hi);
                p.fillRect(QRectF(ba, top, std::max(print ? 0.1 * mm : 1.0, bb - ba), bottom - top), b.virt ? QColor(0xfe, 0xcb, 0xcb) : QColor(0xd8, 0xd8, 0xd8));
            }
    }
    // axes
    p.setPen(axis);
    p.setBrush(Qt::NoBrush);
    p.drawRect(QRectF(L, top, R - L, bottom - top));
    const double sx = niceStep((x1 - x0) / 7);
    for (double x = std::ceil(x0 / sx) * sx; x <= x1; x += sx) {
        const double px = X(x);
        const double t = print ? 0.8 * mm : 3;
        p.drawLine(QPointF(px, top - t), QPointF(px, top + t));
        const QString lab = QString::number(x, 'g', 6);
        const double w = fm.horizontalAdvance(lab) + 2 * gap;
        p.drawText(QRectF(px - w / 2, top - fm.height() - t, w, fm.height()), Qt::AlignHCenter | Qt::AlignBottom, lab);
    }
    const double sy = niceStep(ymax / 2.2);
    for (double y = 0; y <= ymax; y += sy) {
        const double py = Y(y);
        p.drawLine(QPointF(L - (print ? mm : 4), py), QPointF(L, py));
        p.drawText(QRectF(rect.left(), py - fm.height() / 2, L - rect.left() - gap * 1.4, fm.height()), Qt::AlignRight | Qt::AlignVCenter, QString::number(std::lround(y)));
    }
    // traces (clipped to the plot rectangle)
    p.save();
    p.setClipRect(QRectF(L, top, R - L, bottom - top), Qt::IntersectClip);
    p.setRenderHint(QPainter::Antialiasing, true);
    for (const auto &t : d.traces) {
        if (!v.dyes.contains(t.tag)) continue;
        QPen pen(dyeColor(t.tag));
        pen.setWidthF(print ? 0.15 * mm : 1.0);
        pen.setCosmetic(!print);
        p.setPen(pen);
        QPainterPath path;
        bool started = false;
        // points per pixel column are reduced to their min and max (keeps every peak top, far fewer segments)
        const int n = t.pts.size();
        int i = 0;
        while (i < n && t.pts[i].x() < x0) ++i;
        if (i > 0) --i;
        double col = -1e300, mn = 0, mx = 0, firstY = 0, lastY = 0;
        auto flush = [&](double cx) {
            if (col < -1e299) return;
            if (!started) { path.moveTo(cx, Y(firstY)); started = true; } else path.lineTo(cx, Y(firstY));
            path.lineTo(cx, Y(mn)); path.lineTo(cx, Y(mx)); path.lineTo(cx, Y(lastY));
        };
        const double res = print ? 0.1 * mm : 1.0;   // printer: 0.1 mm columns (vector output)
        for (; i < n; ++i) {
            const double px = X(t.pts[i].x()), y = t.pts[i].y();
            const double c = std::floor(px / res) * res;
            if (c != col) { flush(col); col = c; mn = mx = firstY = lastY = y; }
            else { mn = std::min(mn, y); mx = std::max(mx, y); lastY = y; }
            if (t.pts[i].x() > x1) break;
        }
        flush(col);
        p.drawPath(path);
    }
    // marker indicators: red triangles at the start and end of each marker window (after bin offsets)
    if (v.indicators && v.bp) {
        p.setPen(Qt::NoPen);
        p.setBrush(QColor(255, 0, 0));
        const double tw = print ? 1.0 * mm : 4, th = print ? 2.4 * mm : 10;
        for (const auto &m : d.markers) {
            if (!v.dyes.contains(m.tag)) continue;
            const QVector<double> w = m.window.size() == 2 ? m.window : QVector<double>{m.lo, m.hi};
            for (double xv : w) {
                const double x = X(xv);
                if (x < L - 1 || x > R + 1) continue;
                p.drawPolygon(QPolygonF{QPointF(x - tw, bottom), QPointF(x, bottom - th), QPointF(x + tw, bottom)});
            }
        }
        p.setBrush(Qt::NoBrush);
    }
    p.restore();
    // allele label boxes with a lead line to the axis
    p.setFont(lf);
    for (const auto &pl : placed) {
        const double y = bottom + gap * 0.6 + pl.row * rowH, h = lfm.height() + gap * 0.3;
        const QColor col = labelColor(pl.l->tag);
        QPen border(pl.l->flagged ? QColor(0xe0, 0x40, 0xa0) : col);
        border.setWidthF(print ? 0.2 * mm : 1.0);
        border.setCosmetic(!print);
        QPen lead(col);
        lead.setWidthF(print ? 0.15 * mm : 1.0);
        lead.setCosmetic(!print);
        p.setPen(lead);
        p.drawLine(QPointF(pl.x, bottom), QPointF(pl.x, y));
        const QRectF box(pl.x - pl.w / 2, y, pl.w, h);
        p.fillRect(box, Qt::white);
        p.setPen(border);
        p.drawRect(box);
        p.setPen(Qt::black);
        p.drawText(box, Qt::AlignCenter, pl.l->text);
    }
    p.restore();
}

// ------------------------------------------------------------------ pane widget
PaneWidget::PaneWidget(PlotWindow *win, int index) : QWidget(), win_(win), index_(index) {
    setMouseTracking(true);
    setMinimumHeight(120);
    setSizePolicy(QSizePolicy::Expanding, QSizePolicy::Fixed);
}

QSize PaneWidget::sizeHint() const { return QSize(800, 220); }

void PaneWidget::paintEvent(QPaintEvent *) {
    QPainter p(this);
    double x0, x1;
    win_->visibleRange(x0, x1);
    paintPane(p, QRectF(rect()).adjusted(0, 0, -1, -1), win_->paneData(index_), win_->paneView(index_), x0, x1, false);
    if (dragFrom_ >= 0 && dragTo_ >= 0) {
        p.fillRect(QRect(QPoint(std::min(dragFrom_, dragTo_), 0), QPoint(std::max(dragFrom_, dragTo_), height())), QColor(10, 36, 106, 40));
    }
    p.setPen(QColor(180, 186, 196));
    p.drawLine(0, height() - 1, width(), height() - 1);
}

double PaneWidget::xAt(double px) const {
    double x0, x1;
    win_->visibleRange(x0, x1);
    QFont small = QApplication::font();
    small.setPointSizeF(std::max(7.5, small.pointSizeF() - 0.5));
    const QFontMetricsF fm(small, this);
    const double L = fm.horizontalAdvance("000000") + 12, R = width() - 12;
    return x0 + (px - L) / (R - L) * (x1 - x0);
}

void PaneWidget::mousePressEvent(QMouseEvent *e) {
    if (e->button() == Qt::LeftButton) { dragFrom_ = dragTo_ = int(e->position().x()); update(); }
}
void PaneWidget::mouseMoveEvent(QMouseEvent *e) {
    win_->showCursor(xAt(e->position().x()), 0);
    if (dragFrom_ >= 0) { dragTo_ = int(e->position().x()); update(); }
}
void PaneWidget::mouseReleaseEvent(QMouseEvent *e) {
    if (dragFrom_ >= 0 && std::abs(dragTo_ - dragFrom_) > 4) {
        const double a = xAt(std::min(dragFrom_, dragTo_)), b = xAt(std::max(dragFrom_, dragTo_));
        win_->zoomTo(a, b);
    }
    dragFrom_ = dragTo_ = -1;
    update();
    Q_UNUSED(e);
}
void PaneWidget::mouseDoubleClickEvent(QMouseEvent *) { win_->fullView(); }
void PaneWidget::wheelEvent(QWheelEvent *e) {
    if (!(e->modifiers() & Qt::ControlModifier)) { e->ignore(); return; }   // plain wheel scrolls the panes
    double x0, x1;
    win_->visibleRange(x0, x1);
    const double c = xAt(e->position().x()), f = e->angleDelta().y() > 0 ? 0.8 : 1.25;
    win_->zoomTo(c - (c - x0) * f, c + (x1 - c) * f);
}

// ------------------------------------------------------------------ window
PlotWindow::PlotWindow(const Backend &backend, const QVector<int> &rows, QWidget *parent)
    : QMainWindow(parent), backend_(backend), rows_(rows) {
    setAttribute(Qt::WA_DeleteOnClose);
    setWindowTitle(QStringLiteral("OpenTrace — Графики"));
    for (const char *d : DYES) view_.dyes.insert(d);
    view_.dyes.remove("DATA105");   // size standard hidden by default (ID-X "Data Interpretation" plot setting)
    QSettings st;
    view_.panes = std::clamp(st.value("plot/panes", 2).toInt(), 1, 8);
    load();
    scroll_ = new QScrollArea(this);
    scroll_->setWidgetResizable(true);
    scroll_->setFrameShape(QFrame::NoFrame);
    host_ = new QWidget;
    layout_ = new QVBoxLayout(host_);
    layout_->setContentsMargins(0, 0, 0, 0);
    layout_->setSpacing(0);
    scroll_->setWidget(host_);
    rebuildPanes();
    setCentralWidget(scroll_);
    xy_ = new QLabel;
    xy_->setMinimumWidth(160);
    statusBar()->addWidget(xy_);
    statusBar()->addPermanentWidget(new QLabel(QString::fromUtf8("Образцов: %1").arg(panes_.size())));
    buildMenus();
    resize(1200, 820);
}

void PlotWindow::load() {
    panes_.clear();
    for (int r : rows_) {
        PaneData d;
        SampleRow row = backend_.sample_row(static_cast<std::size_t>(r));
        d.file = q(row.file);
        d.name = q(row.name);
        d.sq = q(row.sq);
        bool sized = !backend_.sized_range(static_cast<std::size_t>(r)).empty();
        const bool bp = view_.bp && sized;
        auto traces = backend_.sample_traces(static_cast<std::size_t>(r), bp);
        d.lo = 1e300; d.hi = -1e300;
        for (const auto &t : traces) {
            TraceData td;
            td.tag = q(t.tag);
            td.pts.reserve(static_cast<int>(t.xs.size()));
            for (std::size_t k = 0; k < t.xs.size(); ++k) td.pts.push_back(QPointF(t.xs[k], t.ys[k]));
            d.traces.push_back(std::move(td));
        }
        if (bp) {
            for (const auto &m : backend_.plot_markers(static_cast<std::size_t>(r))) {
                MarkerData md{q(m.name), q(m.tag), q(m.gq), m.min_bp, m.max_bp, {}, {}};
                for (double w : m.window) md.window << w;
                for (std::size_t k = 0; k + 2 < m.bins.size(); k += 3) md.bins << BinData{m.bins[k], m.bins[k + 1], m.bins[k + 2] > 0.5};
                d.markers << md;
            }
        }
        for (const auto &l : backend_.plot_labels(static_cast<std::size_t>(r), bp)) d.labels << LabelData{q(l.tag), q(l.text), l.x, l.height, l.flagged};
        if (bp) {
            auto range = backend_.sized_range(static_cast<std::size_t>(r));
            d.lo = range[0] - 10; d.hi = range[1] + 10;
        } else {
            for (const auto &t : d.traces) if (!t.pts.isEmpty()) { d.lo = std::min(d.lo, t.pts.first().x()); d.hi = std::max(d.hi, t.pts.last().x()); }
        }
        if (d.hi <= d.lo) { d.lo = 0; d.hi = 1; }
        panes_.push_back(std::move(d));
    }
}

ViewState PlotWindow::paneView(int item) const {
    ViewState v = view_;
    if (!items_[item].dye.isEmpty()) v.dyes = {items_[item].dye};
    return v;
}

// Panes: per sample one pane (combined dyes) or one pane per shown dye that the sample has (separate).
void PlotWindow::rebuildPanes() {
    items_.clear();
    for (int i = 0; i < panes_.size(); ++i) {
        if (!view_.separate) { items_.push_back({i, QString()}); continue; }
        for (const char *d : DYES) {
            const QString tag = d;
            if (!view_.dyes.contains(tag)) continue;
            if (std::any_of(panes_[i].traces.begin(), panes_[i].traces.end(), [&](const TraceData &t) { return t.tag == tag; })) items_.push_back({i, tag});
        }
    }
    if (!layout_) return;
    while (QLayoutItem *it = layout_->takeAt(0)) {
        if (it->widget()) it->widget()->deleteLater();
        delete it;
    }
    for (int k = 0; k < items_.size(); ++k) layout_->addWidget(new PaneWidget(this, k));
    layout_->addStretch(1);
    relayout();
}

void PlotWindow::visibleRange(double &x0, double &x1) const {
    if (view_.x1 > view_.x0) { x0 = view_.x0; x1 = view_.x1; return; }
    x0 = 1e300; x1 = -1e300;
    for (const auto &d : panes_) { x0 = std::min(x0, d.lo); x1 = std::max(x1, d.hi); }
    if (x1 <= x0) { x0 = 0; x1 = 1; }
}

void PlotWindow::zoomTo(double a, double b) {
    if (b - a < 1e-3) return;
    view_.x0 = a; view_.x1 = b;
    repaintPanes();
}
void PlotWindow::fullView() { view_.x0 = view_.x1 = 0; repaintPanes(); }
void PlotWindow::showCursor(double x, double) {
    xy_->setText(view_.bp ? QString::fromUtf8("X: %1 п.н.").arg(x, 0, 'f', 2) : QString::fromUtf8("X: %1 точка").arg(std::lround(x)));
}

void PlotWindow::repaintPanes() {
    for (auto *w : host_->findChildren<PaneWidget *>()) w->update();
}

void PlotWindow::relayout() {
    const int h = std::max(120, scroll_->viewport()->height() / std::max(1, view_.panes));
    for (auto *w : host_->findChildren<PaneWidget *>()) w->setFixedHeight(h);
    scroll_->verticalScrollBar()->setSingleStep(h);
    scroll_->verticalScrollBar()->setPageStep(h * view_.panes);
}

void PlotWindow::resizeEvent(QResizeEvent *e) {
    QMainWindow::resizeEvent(e);
    relayout();
}

void PlotWindow::showEvent(QShowEvent *e) {
    QMainWindow::showEvent(e);
    QTimer::singleShot(0, this, [this] { relayout(); });
}

void PlotWindow::buildMenus() {
    auto *file = menuBar()->addMenu(QString::fromUtf8("&Файл"));
    auto *aPrint = file->addAction(style()->standardIcon(QStyle::SP_FileDialogDetailedView), QString::fromUtf8("&Печать..."), this, [this] { print(false); });
    aPrint->setShortcut(QKeySequence::Print);
    auto *aPreview = file->addAction(QString::fromUtf8("Предварительный &просмотр..."), this, [this] { print(true); });
    auto *aPdf = file->addAction(style()->standardIcon(QStyle::SP_DialogSaveButton), QString::fromUtf8("Сохранить в PDF..."), this, [this] { exportPdf(); });
    file->addSeparator();
    file->addAction(QString::fromUtf8("&Закрыть"), QKeySequence::Close, this, &QWidget::close);

    auto *view = menuBar()->addMenu(QString::fromUtf8("&Вид"));
    auto *tb = addToolBar(QString::fromUtf8("Графики"));
    tb->setObjectName("plotToolbar");
    tb->setIconSize(QSize(16, 16));
    tb->addAction(aPrint);
    tb->addAction(aPreview);
    tb->addAction(aPdf);
    tb->addSeparator();
    const char *keys[] = {"Ctrl+1", "Ctrl+2", "Ctrl+3", "Ctrl+4", "Ctrl+5", "Ctrl+0"};
    for (int k = 0; k < 6; ++k) {
        const QString tag = DYES[k];
        auto *a = view->addAction(colorIcon(dyeSwatch(tag)), QString::fromUtf8("Краситель: ") + dyeName(tag));
        a->setCheckable(true);
        a->setChecked(view_.dyes.contains(tag));
        a->setShortcut(QKeySequence(keys[k]));
        connect(a, &QAction::toggled, this, [this, tag](bool on) { if (on) view_.dyes.insert(tag); else view_.dyes.remove(tag); if (view_.separate) rebuildPanes(); else repaintPanes(); });
        tb->addAction(a);
    }
    tb->addSeparator();
    auto *plots = menuBar()->addMenu(QString::fromUtf8("&Графики"));
    auto *aFull = plots->addAction(style()->standardIcon(QStyle::SP_BrowserReload), QString::fromUtf8("Полный вид"), this, [this] { fullView(); });
    aFull->setShortcut(QKeySequence("Ctrl+]"));
    tb->addAction(aFull);
    plots->addSeparator();
    auto toggle = [this, plots, tb](const char *label, const char *key, bool ViewState::*field) {
        auto *a = plots->addAction(QString::fromUtf8(label));
        a->setCheckable(true);
        a->setChecked(view_.*field);
        a->setShortcut(QKeySequence(key));
        connect(a, &QAction::toggled, this, [this, field](bool on) { view_.*field = on; repaintPanes(); });
        Q_UNUSED(tb);
        return a;
    };
    {
        auto *lay = plots->addMenu(QString::fromUtf8("Компоновка"));
        auto *lg = new QActionGroup(this);
        auto *aSep = lay->addAction(QString::fromUtf8("Раздельно (краситель на панель)"));
        auto *aComb = lay->addAction(QString::fromUtf8("Совмещённо (все красители образца)"));
        for (auto *a : {aSep, aComb}) { a->setCheckable(true); lg->addAction(a); }
        aSep->setChecked(view_.separate);
        aComb->setChecked(!view_.separate);
        aSep->setShortcut(QKeySequence("Ctrl+8"));
        aComb->setShortcut(QKeySequence("Ctrl+7"));
        connect(aSep, &QAction::triggered, this, [this] { view_.separate = true; rebuildPanes(); });
        connect(aComb, &QAction::triggered, this, [this] { view_.separate = false; rebuildPanes(); });
    }
    toggle("Бины", "Ctrl+Shift+B", &ViewState::bins);
    toggle("Диапазоны маркеров", "Ctrl+Shift+R", &ViewState::markerRange);
    toggle("Метки аллелей", "Ctrl+Shift+H", &ViewState::labels);
    toggle("Индикаторы маркеров", "Ctrl+K", &ViewState::indicators);
    plots->addSeparator();
    auto *axis = plots->addMenu(QString::fromUtf8("Ось X"));
    auto *grp = new QActionGroup(this);
    auto *aBp = axis->addAction(QString::fromUtf8("Пары нуклеотидов"));
    auto *aDp = axis->addAction(QString::fromUtf8("Точки данных"));
    for (auto *a : {aBp, aDp}) { a->setCheckable(true); grp->addAction(a); }
    aBp->setChecked(true);
    aBp->setShortcut(QKeySequence("Ctrl+Y"));
    aDp->setShortcut(QKeySequence("Ctrl+N"));
    connect(aBp, &QAction::triggered, this, [this] { view_.bp = true; view_.x0 = view_.x1 = 0; load(); rebuildPanes(); });
    connect(aDp, &QAction::triggered, this, [this] { view_.bp = false; view_.x0 = view_.x1 = 0; load(); rebuildPanes(); });
    tb->addSeparator();
    tb->addWidget(new QLabel(QString::fromUtf8(" Панели: ")));
    panesBox_ = new QSpinBox;
    panesBox_->setRange(1, 8);
    panesBox_->setValue(view_.panes);
    connect(panesBox_, qOverload<int>(&QSpinBox::valueChanged), this, [this](int n) { view_.panes = n; QSettings().setValue("plot/panes", n); relayout(); });
    tb->addWidget(panesBox_);
    auto *help = menuBar()->addMenu(QString::fromUtf8("&Справка"));
    help->addAction(QString::fromUtf8("Управление графиком"), this, [this] {
        statusBar()->showMessage(QString::fromUtf8("Протяжка мышью — увеличить участок; двойной щелчок — полный вид; Ctrl+колесо — масштаб; Ctrl+1…5, Ctrl+0 — красители"), 10000);
    });
}

// ------------------------------------------------------------------ printing (vector)
void PlotWindow::renderPages(QPagedPaintDevice *dev, QPainter &p) {
    const QRectF page = QRectF(0, 0, dev->width(), dev->height());
    const double unit = dev->logicalDpiY() / 25.4;                 // device units per mm
    const double footH = 6 * unit;
    const int per = std::max(1, std::min(view_.panes, 6));
    const double paneH = (page.height() - footH) / per;
    const QString date = QDateTime::currentDateTime().toString("dd.MM.yyyy HH:mm");   // 24-hour Russian format
    double x0, x1;
    visibleRange(x0, x1);
    const int pages = (items_.size() + per - 1) / per;
    QFont f = QApplication::font();
    f.setPointSizeF(7);
    for (int pg = 0; pg < pages; ++pg) {
        if (pg) dev->newPage();
        for (int k = 0; k < per; ++k) {
            const int i = pg * per + k;
            if (i >= items_.size()) break;
            paintPane(p, QRectF(page.left(), page.top() + k * paneH, page.width(), paneH - 2 * unit), paneData(i), paneView(i), x0, x1, true);
        }
        p.setFont(f);
        p.setPen(Qt::black);
        p.drawText(QRectF(page.left(), page.bottom() - footH, page.width() / 2, footH), Qt::AlignLeft | Qt::AlignVCenter, date);
        p.drawText(QRectF(page.left() + page.width() / 2, page.bottom() - footH, page.width() / 2, footH), Qt::AlignRight | Qt::AlignVCenter,
                   QString::fromUtf8("OpenTrace — стр. %1 из %2 — только для исследований").arg(pg + 1).arg(pages));
    }
}

void PlotWindow::print(bool preview) {
    QPrinter printer(QPrinter::HighResolution);
    printer.setPageOrientation(QPageLayout::Landscape);
    printer.setPageMargins(QMarginsF(10, 10, 10, 10), QPageLayout::Millimeter);
    if (preview) {
        QPrintPreviewDialog dlg(&printer, this);
        connect(&dlg, &QPrintPreviewDialog::paintRequested, this, [this](QPrinter *pr) { QPainter p(pr); renderPages(pr, p); });
        dlg.exec();
        return;
    }
    QPrintDialog dlg(&printer, this);
    if (dlg.exec() != QDialog::Accepted) return;
    QPainter p(&printer);
    renderPages(&printer, p);
}

void PlotWindow::exportPdf() {
    QSettings st;
    QString path = QFileDialog::getSaveFileName(this, QString::fromUtf8("Сохранить графики в PDF"), st.value("plot/pdfDir").toString() + "/plots.pdf", "PDF (*.pdf)");
    if (path.isEmpty()) return;
    st.setValue("plot/pdfDir", QFileInfo(path).absolutePath());
    if (savePdf(path)) statusBar()->showMessage(QString::fromUtf8("Сохранено: ") + path, 8000);
}

bool PlotWindow::savePdf(const QString &path) {
    QPdfWriter w(path);
    w.setPageSize(QPageSize(QPageSize::A4));
    w.setPageOrientation(QPageLayout::Landscape);
    w.setPageMargins(QMarginsF(10, 10, 10, 10), QPageLayout::Millimeter);
    w.setResolution(600);
    w.setCreator("OpenTrace Desktop");
    w.setTitle(QString::fromUtf8("Электрофореграммы"));
    QPainter p(&w);
    if (!p.isActive()) return false;
    renderPages(&w, p);
    p.end();
    return true;
}

} // namespace ot
