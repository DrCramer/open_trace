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
#include <QTableWidget>
#include <QHeaderView>
#include <QSplitter>
#include <QContextMenuEvent>
#include <QInputDialog>
#include <QMessageBox>
#include <QMenu>
#include <QJsonDocument>
#include <QJsonObject>
#include <QLayoutItem>
#include <QTimer>
#include <QRegularExpression>
#include <QComboBox>
#include <QJsonArray>
#include <QCloseEvent>
#include <QSignalBlocker>
#include "opentrace-qt/cpp/plotsettingsdialog.h"
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

// Java logical fonts of ID-X plot settings -> installed families
static QFont labelFontOf(const QString &name) {
    QFont f;
    if (name == "Serif") { f.setFamily("Times New Roman"); f.setStyleHint(QFont::Serif); }
    else if (name == "SansSerif" || name == "Dialog") { f.setFamily("Arial"); f.setStyleHint(QFont::SansSerif); }
    else if (name == "Monospaced" || name == "DialogInput") { f.setFamily("Courier New"); f.setStyleHint(QFont::Monospace); }
    else f.setFamily(name);
    return f;
}

static const QStringList PQV_KEYS{"SOS", "SQ", "SSPK", "MIX", "OMR", "CGQ"};

// Geometry of a pane shared by the painter and the mouse handlers: plot rectangle and label rows.
PaneGeometry paneGeometry(const QRectF &rect, const QFontMetricsF &fm, double gap, bool header, bool markers, int labelRows, double rowH) {
    PaneGeometry g;
    const double headH = header ? fm.height() + gap : 0;
    g.head = QRectF(rect.left(), rect.top(), rect.width(), headH);
    g.L = rect.left() + fm.horizontalAdvance("000000") + 2 * gap;
    g.R = rect.right() - 2 * gap;
    g.markerTop = rect.top() + headH + gap * 0.5;
    g.markerH = markers ? fm.height() + gap * 0.6 : 0;
    g.top = g.markerTop + g.markerH + fm.height() + gap;
    g.bottom = rect.bottom() - gap - labelRows * rowH;
    return g;
}

void paintPane(QPainter &p, const QRectF &rect, const PaneData &d, const ViewState &v, double x0, double x1, bool print, PaneHits *hits, const PaneSelection *sel) {
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
    // allele labels (plot setting Labels tab: font, size, up to 4 lines): boxes in up to 4 rows under the axis,
    // each label in the first row where it does not overlap
    QFont lf = labelFontOf(v.labelFont);
    if (print) lf.setPointSizeF(std::max(4.0, (v.labelSize + 2) * 0.62));
    else lf.setPixelSize(std::max(6, v.labelSize + 2));
    const QFontMetricsF lfm(lf, p.device());
    const double lineH = lfm.height();
    int nl = 1;
    for (const auto &l : d.labels) if (v.dyes.contains(l.tag)) nl = std::max(nl, int(l.lines.size()));
    const double rowH = nl * lineH + gap * 0.9;
    struct Placed { double x, w, h; int row; const LabelData *l; int index; };
    QVector<Placed> placed;
    int rows = 0;
    auto layoutLabels = [&](double L, double R, int maxRows) {
        placed.clear();
        rows = 0;
        if (v.labels == "none") return;
        for (int li = 0; li < d.labels.size(); ++li) {
            const LabelData &l = d.labels[li];
            if (!v.dyes.contains(l.tag) || l.x < x0 || l.x > x1) continue;
            const double x = L + (l.x - x0) / (x1 - x0) * (R - L);
            const QStringList lines = l.lines.isEmpty() ? QStringList{l.text} : l.lines;
            double w = lfm.height() * 1.2;
            for (const auto &t : lines) w = std::max(w, lfm.horizontalAdvance(t) + gap * 1.2);
            int row = 0;
            while (row < maxRows - 1 && std::any_of(placed.begin(), placed.end(), [&](const Placed &q) { return q.row == row && std::abs(q.x - x) < (q.w + w) / 2 + 2; })) ++row;
            placed.push_back({x, w, lines.size() * lineH + gap * 0.3, row, &l, li});
            rows = std::max(rows, row + 1);
        }
    };
    PaneGeometry g0 = paneGeometry(rect, fm, gap, v.header, showMarkers, 0, rowH);
    layoutLabels(g0.L, g0.R, 4);
    // as many label rows as fit while the trace keeps a usable height (12 mm printed / 50 px on screen)
    const double minTrace = print ? 12 * mm : 50;
    int maxRows = std::max(0, std::min(4, int((g0.bottom - g0.top - minTrace) / rowH)));
    if (rows > maxRows) layoutLabels(g0.L, g0.R, std::max(1, maxRows));
    const PaneGeometry g = paneGeometry(rect, fm, gap, v.header, showMarkers, rows, rowH);
    const double L = g.L, R = g.R, top = g.top, bottom = g.bottom;
    // sample header: the fields chosen on the Sample Header tab (file, name, panel, sample PQVs)
    if (v.header) {
        p.fillRect(g.head, print ? QColor(255, 255, 255) : QColor(236, 239, 244));
        p.setPen(QColor(40, 40, 40));
        double hx = g.head.left() + gap;
        auto text = [&](const QString &t, double minW) {
            if (t.isEmpty() && minW <= 0) return;
            const double w = std::max(minW, fm.horizontalAdvance(t) + 2 * gap);
            p.drawText(QRectF(hx, g.head.top(), w, g.head.height()), Qt::AlignVCenter | Qt::AlignLeft, t);
            hx += w;
        };
        if (d.overlay) text(QString::fromUtf8("Наложение: %1").arg(d.name), 0);
        else {
            if (v.sampleHeader.contains("sampleFileName")) text(d.file, 0);
            if (v.sampleHeader.contains("sampleName")) text(d.name, 0);
            if (v.sampleHeader.contains("panel")) text(d.panel, 0);
            const double sym = fm.height() * 0.85;
            for (const QString &k : PQV_KEYS) {
                if (!v.sampleHeader.contains(k)) continue;
                const QString val = d.pqv.value(k);
                const double kw = fm.horizontalAdvance(k) + gap * 0.5;
                p.setPen(QColor(90, 90, 90));
                p.drawText(QRectF(hx, g.head.top(), kw, g.head.height()), Qt::AlignVCenter | Qt::AlignLeft, k);
                paintQuality(p, QRectF(hx + kw, g.head.center().y() - sym / 2, sym, sym), val);
                if (val == "na" || val.isEmpty()) p.drawText(QRectF(hx + kw, g.head.top(), sym, g.head.height()), Qt::AlignCenter, "–");
                hx += kw + sym + gap * 1.5;
            }
            p.setPen(QColor(40, 40, 40));
        }
    }
    if (bottom - top < 10 || R - L < 20) { p.restore(); return; }
    auto X = [&](double x) { return L + (x - x0) / (x1 - x0) * (R - L); };
    // y range: scaled individually (maximum of the shown dyes in the visible range), to the maximum of all panes,
    // or to the entered value (Display Settings "Y axis"); result dye scales multiply the data
    double ymax = 1;
    for (const auto &t : d.traces) {
        if (!v.dyes.contains(t.tag)) continue;
        const double k = v.scaleOf(t.tag);
        for (const auto &pt : t.pts) if (pt.x() >= x0 && pt.x() <= x1 && pt.y() * k > ymax) ymax = pt.y() * k;
    }
    if (v.yAxis == "entered") ymax = std::max(1.0, v.yValue);
    else if (v.yAxis == "max" && v.yGlobal > 0) ymax = v.yGlobal * 1.08;
    else ymax *= 1.08;
    auto Y = [&](double y) { return bottom - std::min(y, ymax * 1.2) / ymax * (bottom - top); };
    if (hits) { hits->labels.clear(); hits->markers.clear(); hits->head = g.head; hits->L = L; hits->R = R; hits->top = top; hits->bottom = bottom; hits->ymax = ymax; }
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
            if (hits) hits->markers.push_back({hr, m.name});
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
    // off-scale data: pink band (Display Settings "Off-scale data")
    if (v.offscale)
        for (const auto &pk : d.peaks) {
            if (!pk.offscale || !v.dyes.contains(pk.tag)) continue;
            const double xa = v.bp ? X(pk.x) - 3 : X(pk.x - 3), xb = v.bp ? X(pk.x) + 3 : X(pk.x + 3);
            p.fillRect(QRectF(std::min(xa, xb), top, std::max(2.0, std::abs(xb - xa)), bottom - top), QColor(255, 105, 180, 77));
        }
    p.setRenderHint(QPainter::Antialiasing, true);
    int ti = 0;
    for (const auto &t : d.traces) {
        if (!v.dyes.contains(t.tag)) continue;
        const double k = v.scaleOf(t.tag);
        const QColor col = v.customColors && !v.colors.isEmpty() ? QColor(v.colors[ti % v.colors.size()]) : dyeColor(t.tag);
        ++ti;
        QPen pen(col);
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
        double col0 = -1e300, mn = 0, mx = 0, firstY = 0, lastY = 0;
        auto flush = [&](double cx) {
            if (col0 < -1e299) return;
            if (!started) { path.moveTo(cx, Y(firstY)); started = true; } else path.lineTo(cx, Y(firstY));
            path.lineTo(cx, Y(mn)); path.lineTo(cx, Y(mx)); path.lineTo(cx, Y(lastY));
        };
        const double res = print ? 0.1 * mm : 1.0;   // printer: 0.1 mm columns (vector output)
        for (; i < n; ++i) {
            const double px = X(t.pts[i].x()), y = t.pts[i].y() * k;
            const double c = std::floor(px / res) * res;
            if (c != col0) { flush(col0); col0 = c; mn = mx = firstY = lastY = y; }
            else { mn = std::min(mn, y); mx = std::max(mx, y); lastY = y; }
            if (t.pts[i].x() > x1) break;
        }
        flush(col0);
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
    // peak position indicators: a tick under each detected peak
    if (v.peakPositions)
        for (const auto &pk : d.peaks) {
            if (!v.dyes.contains(pk.tag)) continue;
            const double x = X(pk.x);
            if (x < L || x > R) continue;
            QPen tick(dyeColor(pk.tag));
            tick.setWidthF(print ? 0.15 * mm : 1.0);
            tick.setCosmetic(!print);
            p.setPen(tick);
            p.drawLine(QPointF(x, bottom - (print ? 0.3 * mm : 1)), QPointF(x, bottom - (print ? 1.8 * mm : 7)));
        }
    // size-standard labels on the reference dye
    if (v.sizeStdLabels) {
        p.setFont(small);
        p.setPen(Qt::black);
        for (const auto &m : d.stdMarks) {
            if (!v.dyes.contains(m.tag) || m.x < x0 || m.x > x1) continue;
            const QString t = QString::number(m.size, 'f', 1).remove(QRegularExpression("\\.0$"));
            const double y = std::max(top + fm.height(), Y(m.height * v.scaleOf(m.tag)) - (print ? mm : 4));
            const double w = fm.horizontalAdvance(t) + gap;
            p.drawText(QRectF(X(m.x) - w / 2, y - fm.height(), w, fm.height()), Qt::AlignHCenter | Qt::AlignBottom, t);
        }
    }
    if (sel && sel->peak >= 0 && sel->peak < d.peaks.size()) {   // selected peak: filled dot at its top
        const PeakData &pk = d.peaks[sel->peak];
        p.setPen(QPen(Qt::black, 1));
        p.setBrush(dyeSwatch(pk.tag));
        p.drawEllipse(QPointF(X(pk.x), Y(pk.height * v.scaleOf(pk.tag))), 4.5, 4.5);
        p.setBrush(Qt::NoBrush);
    }
    p.restore();
    // allele label boxes with a lead line to the axis; colours per the setting's label colour scheme
    static const QColor ART(0xe0, 0x40, 0xa0), ART_TEXT(0xb0, 0x00, 0x6a), EDITED(0xb0, 0x00, 0x60), LEAD(0x99, 0x99, 0x99), OMR(0xd0, 0x70, 0x0a), OMR_BG(0xff, 0xf3, 0xe0), OMR_TEXT(0xa0, 0x4f, 0x00);
    p.setFont(lf);
    for (const auto &pl : placed) {
        const double y = bottom + gap * 0.6 + pl.row * rowH;
        const LabelData &l = *pl.l;
        const QColor dyeCol = labelColor(l.tag);
        QColor border, text, lead, fill = Qt::white;
        double bw = print ? 0.2 * mm : 1.0;
        if (l.kind == "omr") { border = OMR; text = OMR_TEXT; lead = OMR; fill = OMR_BG; }
        else if (l.kind == "art" || l.kind == "spk") { border = ART; text = ART_TEXT; lead = ART; bw *= 1.5; }
        else {
            border = l.flagged ? ART : v.colorScheme == "bw" ? QColor(Qt::black) : dyeCol;
            text = v.alleleChanges && l.edited ? EDITED : v.colorScheme == "dyeAll" ? dyeCol : QColor(Qt::black);
            lead = v.colorScheme == "bw" ? LEAD : dyeCol;
        }
        QPen lp(lead);
        lp.setWidthF(print ? 0.15 * mm : 1.0);
        lp.setCosmetic(!print);
        p.setPen(lp);
        p.drawLine(QPointF(pl.x, bottom), QPointF(pl.x, y));
        const QRectF box(pl.x - pl.w / 2, y, pl.w, pl.h);
        const bool selected = sel && sel->label == pl.index;
        p.fillRect(box, selected ? QColor(255, 247, 150) : fill);
        QPen bp(border);
        bp.setWidthF(selected ? (print ? 0.4 * mm : 2.0) : bw);
        bp.setCosmetic(!print);
        p.setPen(bp);
        p.drawRect(box);
        p.setPen(text);
        const QStringList lines = l.lines.isEmpty() ? QStringList{l.text} : l.lines;
        for (int k = 0; k < lines.size(); ++k)
            p.drawText(QRectF(box.left(), box.top() + gap * 0.15 + k * lineH, box.width(), lineH), Qt::AlignCenter, lines[k]);
        if (hits && (l.kind == "call" || l.kind == "art")) hits->labels.push_back({box, pl.index});
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
    if (index_ >= win_->paneCount()) return;
    QPainter p(this);
    double x0, x1;
    win_->visibleRange(x0, x1);
    const PaneSelection sel = win_->selection(index_);
    paintPane(p, QRectF(rect()).adjusted(0, 0, -1, -1), win_->paneData(index_), win_->paneView(index_), x0, x1, false, &hits_, &sel);
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

// label box under the point, else the detected peak nearest in x (within 6 px, below its top) on a shown dye
bool PaneWidget::hitTest(const QPointF &pt, int &label, int &peak) const {
    label = peak = -1;
    if (win_->paneData(index_).overlay) return false;   // labels are edited in the panes of one sample
    for (const auto &h : hits_.labels) if (h.first.contains(pt)) { label = h.second; return true; }
    if (pt.y() < hits_.top || pt.y() > hits_.bottom) return false;
    double x0, x1;
    win_->visibleRange(x0, x1);
    const PaneData &d = win_->paneData(index_);
    const ViewState v = win_->paneView(index_);
    double best = 7;
    for (int k = 0; k < d.peaks.size(); ++k) {
        const PeakData &pk = d.peaks[k];
        if (!v.dyes.contains(pk.tag) || pk.x < x0 || pk.x > x1) continue;
        const double px = hits_.L + (pk.x - x0) / (x1 - x0) * (hits_.R - hits_.L);
        const double top = hits_.bottom - std::min(pk.height * v.scaleOf(pk.tag), hits_.ymax * 1.2) / hits_.ymax * (hits_.bottom - hits_.top);
        if (std::abs(px - pt.x()) < best && pt.y() >= top - 8) { best = std::abs(px - pt.x()); peak = k; }
    }
    return peak >= 0;
}

void PaneWidget::mousePressEvent(QMouseEvent *e) {
    if (e->button() != Qt::LeftButton) return;
    int label, peak;
    if (hitTest(e->position(), label, peak)) { win_->select(index_, label, peak); return; }
    win_->select(-1, -1, -1);
    dragFrom_ = dragTo_ = int(e->position().x());
    update();
}

void PaneWidget::contextMenuEvent(QContextMenuEvent *e) {
    int label, peak;
    if (!hitTest(e->pos(), label, peak)) {
        for (const auto &m : hits_.markers)
            if (m.first.contains(e->pos())) { win_->headerMenu(index_, m.second, e->globalPos()); return; }
        if (hits_.head.contains(e->pos())) win_->headerMenu(index_, QString(), e->globalPos());
        return;
    }
    win_->select(index_, label, peak);
    win_->editMenu(index_, e->globalPos());
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
    applySetting(q(backend_.plot_setting_active()));
    load();
    scroll_ = new QScrollArea(this);
    scroll_->setWidgetResizable(true);
    scroll_->setFrameShape(QFrame::NoFrame);
    host_ = new QWidget;
    layout_ = new QVBoxLayout(host_);
    layout_->setContentsMargins(0, 0, 0, 0);
    layout_->setSpacing(0);
    scroll_->setWidget(host_);
    scroll_->viewport()->installEventFilter(this);   // pane heights follow the viewport (lower table shown / hidden, splitter)
    rebuildPanes();
    lower_ = new QTableWidget;
    lower_->setEditTriggers(QAbstractItemView::NoEditTriggers);
    lower_->setSelectionBehavior(QAbstractItemView::SelectRows);
    lower_->setAlternatingRowColors(true);
    lower_->verticalHeader()->setDefaultSectionSize(lower_->fontMetrics().height() + 4);
    connect(lower_, &QTableWidget::cellClicked, this, [this](int row, int) {   // sizing row -> select the peak in its pane
        const auto *it = lower_->item(row, 0);
        if (!it || table_ != "sizing") return;
        const int data = it->data(Qt::UserRole).toInt(), peak = it->data(Qt::UserRole + 1).toInt();
        const QString tag = panes_[data].peaks[peak].tag;
        for (int k = 0; k < items_.size(); ++k)
            if (items_[k].data == data && (items_[k].dye.isEmpty() || items_[k].dye == tag)) {
                select(k, -1, peak);
                if (auto *w = host_->findChildren<PaneWidget *>().value(k)) scroll_->ensureWidgetVisible(w);
                break;
            }
    });
    split_ = new QSplitter(Qt::Vertical);
    split_->addWidget(scroll_);
    split_->addWidget(lower_);
    split_->setStretchFactor(0, 3);
    split_->setStretchFactor(1, 1);
    setCentralWidget(split_);
    fillLower();
    xy_ = new QLabel;
    xy_->setMinimumWidth(160);
    statusBar()->addWidget(xy_);
    statusBar()->addPermanentWidget(new QLabel(QString::fromUtf8("Образцов: %1").arg(panes_.size())));
    buildMenus();
    resize(1200, 820);
    QTimer::singleShot(0, this, [this] {   // setting with the Label Edit Viewer as its table
        if (setting_.value("display").toObject().value("table").toString() == "lev" && onLabelEditViewer) onLabelEditViewer();
    });
}

void PlotWindow::load() {
    panes_.clear();
    for (int r : rows_) {
        PaneData d;
        const std::size_t ri = static_cast<std::size_t>(r);
        SampleRow row = backend_.sample_row(ri);
        d.file = q(row.file);
        d.name = q(row.name);
        d.sq = q(row.sq);
        d.stype = q(row.stype);
        d.panel = q(row.panel);
        d.pqv = {{"SOS", q(row.sos)}, {"SQ", q(row.sq)}, {"SSPK", q(row.sspk)}, {"MIX", q(row.mix)}, {"OMR", q(row.omr)}, {"CGQ", q(row.cgq)}};
        bool sized = !backend_.sized_range(ri).empty();
        const bool bp = view_.bp && sized;
        auto traces = backend_.sample_traces(ri, bp);
        d.lo = 1e300; d.hi = -1e300;
        for (const auto &t : traces) {
            TraceData td;
            td.tag = q(t.tag);
            td.pts.reserve(static_cast<int>(t.xs.size()));
            for (std::size_t k = 0; k < t.xs.size(); ++k) td.pts.push_back(QPointF(t.xs[k], t.ys[k]));
            d.traces.push_back(std::move(td));
        }
        if (bp) {
            for (const auto &m : backend_.plot_markers(ri)) {
                MarkerData md{q(m.name), q(m.tag), q(m.gq), m.min_bp, m.max_bp, {}, {}};
                for (double w : m.window) md.window << w;
                for (std::size_t k = 0; k + 2 < m.bins.size(); k += 3) md.bins << BinData{m.bins[k], m.bins[k + 1], m.bins[k + 2] > 0.5};
                d.markers << md;
            }
        }
        for (const auto &l : backend_.plot_labels(ri, bp)) {
            QStringList lines;
            for (const auto &x : l.lines) lines << q(x);
            d.labels << LabelData{q(l.tag), q(l.text), l.x, l.height, l.flagged, q(l.id), q(l.marker), q(l.kind), l.edited, lines};
        }
        for (const auto &pk : backend_.sample_peaks(ri, bp)) d.peaks << PeakData{q(pk.tag), pk.x, pk.height, pk.scan, pk.size_bp, pk.area, pk.offscale};
        for (const auto &pk : backend_.size_std_marks(ri, bp)) d.stdMarks << PeakData{q(pk.tag), pk.x, pk.height, pk.scan, pk.size_bp, 0, false};
        d.sample = r;
        if (bp) {
            auto range = backend_.sized_range(ri);
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

// Panes: per sample one pane (combined dyes) or one pane per shown dye that the sample has (separate), or one pane
// with every shown sample (overlay). Controls / allelic ladders first when the setting brings them to the top.
void PlotWindow::rebuildPanes() {
    items_.clear();
    QVector<int> order;
    for (int i = 0; i < panes_.size(); ++i) order << i;
    auto rank = [this](int i) {
        const QString t = panes_[i].stype;
        return (view_.laddersTop && t == "ladder") || (view_.controlsTop && (t == "positive" || t == "negative")) ? 0 : 1;
    };
    std::stable_sort(order.begin(), order.end(), [&](int a, int b) { return rank(a) < rank(b); });
    if (view_.layout == "overlay") {
        overlay_ = PaneData{};
        overlay_.overlay = true;
        overlay_.lo = 1e300; overlay_.hi = -1e300;
        QStringList names;
        for (int i : order) {
            const PaneData &d = panes_[i];
            for (const auto &t : d.traces) if (view_.dyes.contains(t.tag)) overlay_.traces << t;
            overlay_.labels << d.labels;
            overlay_.peaks << d.peaks;
            overlay_.stdMarks << d.stdMarks;
            if (overlay_.markers.isEmpty()) overlay_.markers = d.markers;
            overlay_.lo = std::min(overlay_.lo, d.lo);
            overlay_.hi = std::max(overlay_.hi, d.hi);
            names << (d.name.isEmpty() ? d.file : d.name);
        }
        overlay_.name = QString::fromUtf8("%1 образцов").arg(names.size());
        if (!panes_.isEmpty()) items_.push_back({-1, QString()});
    } else {
        for (int i : order) {
            if (view_.layout != "separate") { items_.push_back({i, QString()}); continue; }
            for (const char *d : DYES) {
                const QString tag = d;
                if (!view_.dyes.contains(tag)) continue;
                if (std::any_of(panes_[i].traces.begin(), panes_[i].traces.end(), [&](const TraceData &t) { return t.tag == tag; })) items_.push_back({i, tag});
            }
        }
    }
    selItem_ = -1;
    sel_ = PaneSelection{};
    updateYGlobal();
    if (!layout_) return;
    while (QLayoutItem *it = layout_->takeAt(0)) {
        if (QWidget *w = it->widget()) {   // out of the window at once (a pending delete must not paint a stale index)
            w->hide();
            w->setParent(nullptr);
            w->deleteLater();
        }
        delete it;
    }
    for (int k = 0; k < items_.size(); ++k) layout_->addWidget(new PaneWidget(this, k));
    layout_->addStretch(1);
    relayout();
}

// "Scale to the maximum Y of all panes": the highest shown (scaled) data point in the visible X range.
void PlotWindow::updateYGlobal() {
    view_.yGlobal = 0;
    if (view_.yAxis != "max") return;
    double x0, x1;
    visibleRange(x0, x1);
    for (int k = 0; k < items_.size(); ++k) {
        const PaneData &d = paneData(k);
        const ViewState v = paneView(k);
        for (const auto &t : d.traces) {
            if (!v.dyes.contains(t.tag)) continue;
            const double f = v.scaleOf(t.tag);
            for (const auto &pt : t.pts) if (pt.x() >= x0 && pt.x() <= x1) view_.yGlobal = std::max(view_.yGlobal, pt.y() * f);
        }
    }
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
    if (!host_) return;
    updateYGlobal();
    for (auto *w : host_->findChildren<PaneWidget *>()) w->update();
}

void PlotWindow::relayout() {
    if (!scroll_ || !host_) return;
    // multi-line labels (Labels tab) get extra room so the trace keeps its height
    int nl = 1;
    const QJsonObject L = setting_.value("labels").toObject();
    for (const char *k : {"assigned", "custom", "ladder", "artifact"}) {
        const QJsonArray a = L.value(k).toArray();
        int n = 0;
        while (n < a.size() && a.at(n).toString() != "none") ++n;
        nl = std::max(nl, n);
    }
    const int extra = view_.labels == "none" ? 0 : 2 * (nl - 1) * (view_.labelSize + 3);
    const int h = std::max(120 + extra, scroll_->viewport()->height() / std::max(1, view_.panes));
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

bool PlotWindow::eventFilter(QObject *o, QEvent *e) {
    if (scroll_ && o == scroll_->viewport() && e->type() == QEvent::Resize) {
        const int h = static_cast<QResizeEvent *>(e)->size().height();
        if (h != lastViewportH_) { lastViewportH_ = h; QTimer::singleShot(0, this, [this] { relayout(); }); }
    }
    return QMainWindow::eventFilter(o, e);
}

void PlotWindow::closeEvent(QCloseEvent *e) {
    saveLastView();
    QMainWindow::closeEvent(e);
}

// ------------------------------------------------------------------ plot settings
static const QList<QPair<QString, QString>> SETTING_DYES{{"blue", "DATA1"}, {"green", "DATA2"}, {"yellow", "DATA3"}, {"red", "DATA4"}, {"purple", "DATA106"}, {"orange", "DATA105"}};

// Display Settings, Labels, Sample Header and Sizing Table of the setting; "use last" settings bring back the
// interactive state of the last plot window that used them.
void PlotWindow::applySetting(const QString &name) {
    const QByteArray nb = name.toUtf8();
    setting_ = parseJsonObject(q(backend_.plot_setting_json(rust::Str(nb.constData(), nb.size()))));
    if (setting_.isEmpty()) return;
    settingName_ = name;
    backend_.plot_setting_set_active(rust::Str(nb.constData(), nb.size()));
    const QJsonObject D = parseJsonObject(q(backend_.plot_setting_display(rust::Str(nb.constData(), nb.size()))));
    view_.panes = std::clamp(D.value("panes").toInt(2), 1, 16);
    view_.layout = D.value("layout").toString("separate");
    view_.labels = D.value("labels").toString("h");
    view_.bins = D.value("bins").toBool(true);
    view_.header = D.value("header").toBool(true);
    view_.markerRange = D.value("markerRange").toBool(true);
    view_.indicators = D.value("markerIndicator").toBool(true);
    view_.bp = D.value("xAxis").toString("bp") != "dp";
    view_.yAxis = D.value("yAxis").toString("individual");
    view_.yValue = D.value("yValue").toDouble(1000);
    view_.peakPositions = D.value("peakPositions").toBool();
    view_.offscale = D.value("offscale").toBool(true);
    view_.alleleChanges = D.value("alleleChanges").toBool(true);
    view_.sizeStdLabels = D.value("sizeStdLabels").toBool();
    view_.controlsTop = D.value("controlsTop").toBool();
    view_.laddersTop = D.value("laddersTop").toBool();
    const QJsonObject dyes = D.value("dyes").toObject();
    view_.dyes.clear();
    for (const auto &d : SETTING_DYES) if (dyes.value(d.first).toBool(true)) view_.dyes.insert(d.second);
    view_.colors.clear();
    for (const auto &c : D.value("customColors").toArray()) view_.colors << c.toString();
    view_.scale.clear();
    const QJsonObject sc = D.value("dyeScales").toObject();
    for (const auto &d : SETTING_DYES) view_.scale[d.second] = sc.value(d.first).toDouble(1.0);
    view_.x0 = view_.x1 = 0;
    if (!D.value("allDyes").toBool(true) && view_.bp && D.value("end").toDouble() > D.value("start").toDouble()) {
        view_.x0 = D.value("start").toDouble();
        view_.x1 = D.value("end").toDouble();
    }
    const QJsonObject L = setting_.value("labels").toObject();
    view_.labelFont = L.value("font").toString("Serif");
    view_.labelSize = L.value("size").toInt(10);
    view_.colorScheme = L.value("color").toString("bw");
    view_.sampleHeader.clear();
    const QJsonObject H = setting_.value("sampleHeader").toObject();
    for (auto it = H.begin(); it != H.end(); ++it) if (it.value().toBool()) view_.sampleHeader.insert(it.key());
    const QJsonObject ST = setting_.value("sizingTable").toObject();
    sizingCols_.clear();
    const QJsonObject cols = ST.value("cols").toObject();
    for (auto it = cols.begin(); it != cols.end(); ++it) if (it.value().toBool()) sizingCols_.insert(it.key());
    sizingFont_ = ST.value("font").toString("Dialog");
    sizingSize_ = ST.value("size").toInt(11);
    const QString table = D.value("table").toString("genotypes");
    table_ = table == "lev" ? "none" : table;
    if (toolbar_) toolbar_->setVisible(D.value("toolbar").toBool(true));
    if (!host_) return;   // constructor: the panes are built next
    load();   // label lines follow the setting's Labels tab
    rebuildPanes();
    fillLower();
    syncActions();
    if (table == "lev" && onLabelEditViewer) QTimer::singleShot(0, this, [this] { if (onLabelEditViewer) onLabelEditViewer(); });
}

// "Use the display settings last used for this plot": the interactive state is kept when the window closes.
void PlotWindow::saveLastView() {
    if (settingName_.isEmpty() || setting_.value("display").toObject().value("persistence").toString() != "last") return;
    QJsonObject dyes;
    for (const auto &d : SETTING_DYES) dyes[d.first] = view_.dyes.contains(d.second);
    const QJsonObject v{{"panes", view_.panes}, {"layout", view_.layout}, {"labels", view_.labels}, {"bins", view_.bins}, {"header", view_.header},
                        {"markerRange", view_.markerRange}, {"table", table_}, {"xAxis", view_.bp ? "bp" : "dp"}, {"yAxis", view_.yAxis}, {"dyes", dyes}};
    const QByteArray nb = settingName_.toUtf8(), js = QJsonDocument(v).toJson(QJsonDocument::Compact);
    backend_.plot_setting_save_view(rust::Str(nb.constData(), nb.size()), rust::Str(js.constData(), js.size()));
}

void PlotWindow::fillSettingBox() {
    if (!settingBox_) return;
    QSignalBlocker block(settingBox_);
    settingBox_->clear();
    for (const auto &n : backend_.plot_setting_names()) settingBox_->addItem(q(n));
    settingBox_->setCurrentText(settingName_);
}

// check marks of the menu after a setting changed the view
void PlotWindow::syncActions() {
    auto set = [this](const QString &k, bool on) { if (auto *a = acts_.value(k)) { QSignalBlocker b(a); a->setChecked(on); } };
    for (const char *d : DYES) set(QString("dye:") + d, view_.dyes.contains(d));
    for (const char *l : {"separate", "combine", "overlay"}) set(QString("layout:") + l, view_.layout == l);
    for (const char *t : {"none", "sizing", "genotypes"}) set(QString("table:") + t, table_ == t);
    for (const char *l : {"none", "h", "v"}) set(QString("labels:") + l, view_.labels == l);
    for (const char *y : {"individual", "max", "entered"}) set(QString("y:") + y, view_.yAxis == y);
    set("x:bp", view_.bp);
    set("x:dp", !view_.bp);
    set("bins", view_.bins);
    set("markerRange", view_.markerRange);
    set("indicators", view_.indicators);
    set("header", view_.header);
    set("peakPositions", view_.peakPositions);
    set("offscale", view_.offscale);
    set("sizeStdLabels", view_.sizeStdLabels);
    set("alleleChanges", view_.alleleChanges);
    set("customColors", view_.customColors);
    set("controlsTop", view_.controlsTop);
    set("laddersTop", view_.laddersTop);
    set("toolbar", toolbar_ && toolbar_->isVisible());
    if (auto *a = acts_.value("y:entered")) a->setText(QString::fromUtf8("По введённому значению (%1)...").arg(view_.yValue));
    if (panesBox_) { QSignalBlocker b(panesBox_); panesBox_->setValue(view_.panes); }
    fillSettingBox();
}

void PlotWindow::buildMenus() {
    auto *file = menuBar()->addMenu(QString::fromUtf8("&Файл"));
    auto *aPrint = file->addAction(style()->standardIcon(QStyle::SP_FileDialogDetailedView), QString::fromUtf8("&Печать..."), this, [this] { print(false); });
    aPrint->setShortcut(QKeySequence::Print);
    auto *aPreview = file->addAction(QString::fromUtf8("Предварительный &просмотр..."), this, [this] { print(true); });
    auto *aPdf = file->addAction(style()->standardIcon(QStyle::SP_DialogSaveButton), QString::fromUtf8("Сохранить в PDF..."), this, [this] { exportPdf(); });
    file->addSeparator();
    file->addAction(QString::fromUtf8("&Закрыть"), QKeySequence::Close, this, &QWidget::close);

    auto *edit = menuBar()->addMenu(QString::fromUtf8("&Правка"));
    edit->addAction(QString::fromUtf8("&Отменить последнюю правку"), QKeySequence::Undo, this, [this] {
        rust::Vec<std::size_t> v;
        for (int r : rows_) v.push_back(static_cast<std::size_t>(r));
        const QString what = q(backend_.undo_last_edit(v));
        if (what.isEmpty()) { statusBar()->showMessage(QString::fromUtf8("Нет правок для отмены"), 4000); return; }
        statusBar()->showMessage(QString::fromUtf8("Отменено: ") + what, 6000);
        afterEdit();
    });
    edit->addSeparator();
    edit->addAction(QString::fromUtf8("&Удалить выделенную метку"), QKeySequence::Delete, this, [this] { editSelected("delete"); });
    edit->addAction(QString::fromUtf8("&Переименовать метку..."), QKeySequence("F2"), this, [this] { editSelected("rename"); });
    edit->addAction(QString::fromUtf8("Добавить &аллель на выделенный пик..."), QKeySequence("Insert"), this, [this] { editSelected("add"); });

    auto *view = menuBar()->addMenu(QString::fromUtf8("&Вид"));
    toolbar_ = addToolBar(QString::fromUtf8("Графики"));
    toolbar_->setObjectName("plotToolbar");
    toolbar_->setIconSize(QSize(16, 16));
    auto *tb = toolbar_;
    // plot setting of the window (ID-X toolbar "Plot Setting") and its editor
    tb->addWidget(new QLabel(QString::fromUtf8(" Настройка: ")));
    settingBox_ = new QComboBox;
    settingBox_->setMinimumContentsLength(18);
    settingBox_->setSizeAdjustPolicy(QComboBox::AdjustToMinimumContentsLengthWithIcon);
    connect(settingBox_, &QComboBox::currentTextChanged, this, [this](const QString &n) { if (!n.isEmpty() && n != settingName_) { saveLastView(); applySetting(n); } });
    tb->addWidget(settingBox_);
    auto *aEditSetting = tb->addAction(QString::fromUtf8("Изменить..."), this, [this] {
        PlotSettingsEditor ed(backend_, setting_, settingName_, false, this);
        if (ed.exec() == QDialog::Accepted) applySetting(ed.savedName());
    });
    aEditSetting->setToolTip(QString::fromUtf8("Редактор настроек графика"));
    tb->addSeparator();
    tb->addAction(aPrint);
    tb->addAction(aPreview);
    tb->addAction(aPdf);
    tb->addSeparator();
    auto check = [this](QMenu *m, const QString &key, const QString &label, const char *sc, bool on, std::function<void(bool)> f) {
        auto *a = m->addAction(label);
        a->setCheckable(true);
        a->setChecked(on);
        if (sc && *sc) a->setShortcut(QKeySequence(sc));
        connect(a, &QAction::toggled, this, f);
        acts_[key] = a;
        return a;
    };
    const char *keys[] = {"Ctrl+1", "Ctrl+2", "Ctrl+3", "Ctrl+4", "Ctrl+5", "Ctrl+0"};
    for (int k = 0; k < 6; ++k) {
        const QString tag = DYES[k];
        auto *a = check(view, "dye:" + tag, QString::fromUtf8("Краситель: ") + dyeName(tag), keys[k], view_.dyes.contains(tag), [this, tag](bool on) {
            if (on) view_.dyes.insert(tag); else view_.dyes.remove(tag);
            if (view_.layout != "combine") rebuildPanes(); else repaintPanes();
            if (table_ == "sizing") fillLower();
        });
        a->setIcon(colorIcon(dyeSwatch(tag)));
        tb->addAction(a);
    }
    view->addSeparator();
    check(view, "toolbar", QString::fromUtf8("Панель инструментов"), "", true, [this](bool on) { toolbar_->setVisible(on); });
    check(view, "header", QString::fromUtf8("Заголовок графика"), "", view_.header, [this](bool on) { view_.header = on; repaintPanes(); });
    tb->addSeparator();
    auto *plots = menuBar()->addMenu(QString::fromUtf8("&Графики"));
    auto *aFull = plots->addAction(style()->standardIcon(QStyle::SP_BrowserReload), QString::fromUtf8("Полный вид"), this, [this] { fullView(); });
    aFull->setShortcut(QKeySequence("Ctrl+]"));
    tb->addAction(aFull);
    plots->addSeparator();
    auto radio = [this](QMenu *m, const QString &group, const QList<std::tuple<QString, QString, const char *>> &items, const QString &cur, std::function<void(const QString &)> f) {
        auto *g = new QActionGroup(this);
        for (const auto &[key, label, sc] : items) {
            auto *a = m->addAction(label);
            a->setCheckable(true);
            a->setChecked(key == cur);
            if (sc && *sc) a->setShortcut(QKeySequence(sc));
            g->addAction(a);
            acts_[group + ":" + key] = a;
            const QString k = key;
            connect(a, &QAction::triggered, this, [f, k] { f(k); });
        }
    };
    radio(plots->addMenu(QString::fromUtf8("Компоновка")), "layout",
          {{"combine", QString::fromUtf8("Объединить красители"), "Ctrl+7"}, {"separate", QString::fromUtf8("Разделить красители"), "Ctrl+8"}, {"overlay", QString::fromUtf8("Наложить все"), "Ctrl+9"}},
          view_.layout, [this](const QString &k) { view_.layout = k; rebuildPanes(); });
    radio(plots->addMenu(QString::fromUtf8("Таблица")), "table",
          {{"none", QString::fromUtf8("Без таблицы"), "Ctrl+Q"}, {"sizing", QString::fromUtf8("Таблица размеров (Sizing)"), "Ctrl+A"}, {"genotypes", QString::fromUtf8("Таблица генотипов"), "Ctrl+W"}},
          table_, [this](const QString &k) { table_ = k; fillLower(); });
    radio(plots->addMenu(QString::fromUtf8("Обозначения")), "labels",
          {{"none", QString::fromUtf8("Без обозначений"), "Ctrl+Shift+N"}, {"h", QString::fromUtf8("Горизонтальные обозначения"), "Ctrl+Shift+H"}, {"v", QString::fromUtf8("Вертикальные обозначения"), "Ctrl+Shift+V"}},
          view_.labels, [this](const QString &k) { view_.labels = k; relayout(); repaintPanes(); });
    check(plots, "bins", QString::fromUtf8("Бины"), "Ctrl+Shift+B", view_.bins, [this](bool on) { view_.bins = on; repaintPanes(); });
    check(plots, "markerRange", QString::fromUtf8("Диапазоны маркеров"), "Ctrl+R", view_.markerRange, [this](bool on) { view_.markerRange = on; repaintPanes(); });
    check(plots, "indicators", QString::fromUtf8("Индикаторы маркеров"), "Ctrl+K", view_.indicators, [this](bool on) { view_.indicators = on; repaintPanes(); });
    check(plots, "peakPositions", QString::fromUtf8("Положения пиков"), "", view_.peakPositions, [this](bool on) { view_.peakPositions = on; repaintPanes(); });
    check(plots, "offscale", QString::fromUtf8("Индикатор зашкаливающих пиков"), "", view_.offscale, [this](bool on) { view_.offscale = on; repaintPanes(); });
    check(plots, "sizeStdLabels", QString::fromUtf8("Обозначения размерного стандарта"), "", view_.sizeStdLabels, [this](bool on) { view_.sizeStdLabels = on; repaintPanes(); });
    check(plots, "alleleChanges", QString::fromUtf8("Изменения аллелей"), "", view_.alleleChanges, [this](bool on) { view_.alleleChanges = on; repaintPanes(); });
    check(plots, "customColors", QString::fromUtf8("Свои цвета кривых"), "", view_.customColors, [this](bool on) { view_.customColors = on; repaintPanes(); });
    check(plots, "controlsTop", QString::fromUtf8("Контроли наверх"), "", view_.controlsTop, [this](bool on) { view_.controlsTop = on; rebuildPanes(); });
    check(plots, "laddersTop", QString::fromUtf8("Леддеры наверх"), "", view_.laddersTop, [this](bool on) { view_.laddersTop = on; rebuildPanes(); });
    plots->addSeparator();
    radio(plots->addMenu(QString::fromUtf8("Ось X")), "x", {{"bp", QString::fromUtf8("Пары нуклеотидов"), "Ctrl+Y"}, {"dp", QString::fromUtf8("Точки данных"), "Ctrl+N"}},
          view_.bp ? "bp" : "dp", [this](const QString &k) { view_.bp = k == "bp"; view_.x0 = view_.x1 = 0; load(); rebuildPanes(); });
    radio(plots->addMenu(QString::fromUtf8("Масштаб по оси Y")), "y",
          {{"max", QString::fromUtf8("Масштаб по максимуму Y"), "Ctrl+Shift+M"}, {"individual", QString::fromUtf8("Масштабировать индивидуально"), "Ctrl+Shift+D"},
           {"entered", QString::fromUtf8("По введённому значению (%1)...").arg(view_.yValue), "Ctrl+Shift+Y"}},
          view_.yAxis, [this](const QString &k) {
              if (k == "entered") {
                  bool ok = false;
                  const double v = QInputDialog::getDouble(this, QString::fromUtf8("Масштаб по оси Y"), QString::fromUtf8("Максимум оси Y (RFU):"), view_.yValue, 1, 1e7, 0, &ok);
                  if (!ok) { syncActions(); return; }
                  view_.yValue = v;
              }
              view_.yAxis = k;
              syncActions();
              repaintPanes();
          });
    plots->addSeparator();
    plots->addAction(QString::fromUtf8("Настройки графиков..."), this, [this] {
        PlotSettingsManager m(backend_, this);
        m.exec();
        if (q(backend_.plot_setting_json(rust::Str(settingName_.toUtf8().constData(), settingName_.toUtf8().size()))).isEmpty()) applySetting(q(backend_.plot_setting_active()));
        else fillSettingBox();
    });
    plots->addAction(aEditSetting);
    tb->addSeparator();
    tb->addWidget(new QLabel(QString::fromUtf8(" Панели: ")));
    panesBox_ = new QSpinBox;
    panesBox_->setRange(1, 16);
    panesBox_->setValue(view_.panes);
    connect(panesBox_, qOverload<int>(&QSpinBox::valueChanged), this, [this](int n) { view_.panes = n; relayout(); });
    tb->addWidget(panesBox_);
    auto *help = menuBar()->addMenu(QString::fromUtf8("&Справка"));
    help->addAction(QString::fromUtf8("Управление графиком"), this, [this] {
        statusBar()->showMessage(QString::fromUtf8("Протяжка мышью — увеличить участок; двойной щелчок — полный вид; Ctrl+колесо — масштаб; Ctrl+1…5, Ctrl+0 — красители"), 10000);
    });
    fillSettingBox();
    toolbar_->setVisible(setting_.value("display").toObject().value("toolbar").toBool(true));
    syncActions();
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


// ------------------------------------------------------------------ label editing
void PlotWindow::select(int item, int label, int peak) {
    selItem_ = item;
    sel_ = PaneSelection{label, label >= 0 ? -1 : peak};
    repaintPanes();
    if (item >= 0) {
        const PaneData &d = paneData(item);
        if (label >= 0) {
            const LabelData &l = d.labels[label];
            xy_->setText(QString::fromUtf8("%1 · %2 · %3").arg(l.marker, l.text, QString::number(l.x, 'f', 2)));
        } else if (peak >= 0) {
            const PeakData &pk = d.peaks[peak];
            xy_->setText(QString::fromUtf8("Пик: %1 п.н., %2 RFU").arg(pk.size, 0, 'f', 2).arg(std::lround(pk.height)));
        }
    }
}

QString PlotWindow::askReason(const QString &what) {
    bool ok = false;
    const QString r = QInputDialog::getText(this, QString::fromUtf8("Причина изменения"), what + QString::fromUtf8("\nПричина изменения (сохраняется в журнале правок):"), QLineEdit::Normal, QString(), &ok);
    if (!ok) return QString();
    return r.isEmpty() ? QString(" ") : r;   // a blank reason is allowed (one space keeps "cancelled" apart)
}

static QString jsonText(const QJsonObject &o) { return QString::fromUtf8(QJsonDocument(o).toJson(QJsonDocument::Compact)); }

bool PlotWindow::editLabel(int item, int label, const QString &action) {
    const PaneData &d = paneData(item);
    if (label < 0 || label >= d.labels.size()) return false;
    const LabelData &l = d.labels[label];
    const bool art = l.kind == "art";
    QJsonObject spec;
    spec["marker"] = l.marker;
    if (art) spec["artifactId"] = l.id; else spec["callId"] = l.id;
    QString what;
    if (action == "delete") {
        spec["action"] = art ? "delete_artifact" : "delete";
        what = QString::fromUtf8("Удалить метку «%1» (%2)").arg(l.text, l.marker);
    } else {
        bool ok = false;
        const QString title = action == "rename" ? QString::fromUtf8("Переименовать") : action == "to_artifact" ? QString::fromUtf8("Заменить на артефакт") : QString::fromUtf8("Сделать аллелью");
        const QString v = QInputDialog::getText(this, title, QString::fromUtf8("Новая метка (%1):").arg(l.marker), QLineEdit::Normal, action == "to_artifact" ? QString("SPK") : l.text, &ok).trimmed();
        if (!ok || v.isEmpty()) return false;
        if (v.size() > 40) { QMessageBox::warning(this, "OpenTrace", QString::fromUtf8("Метка длиннее 40 символов.")); return false; }
        spec["new"] = v;
        spec["action"] = action == "rename" ? (art ? "rename_artifact" : "rename") : action == "to_artifact" ? "to_artifact" : "artifact_to_allele";
        what = QString::fromUtf8("%1: «%2» → «%3» (%4)").arg(title, l.text, v, l.marker);
    }
    const QString reason = askReason(what);
    if (reason.isEmpty()) return false;
    const QByteArray sp = jsonText(spec).toUtf8(), rs = reason.trimmed().toUtf8();
    const auto err = backend_.apply_edit_json(static_cast<std::size_t>(d.sample), rust::Str(sp.constData(), sp.size()), rust::Str(rs.constData(), rs.size()));
    if (!err.empty()) { QMessageBox::warning(this, "OpenTrace", q(err)); return false; }
    afterEdit();
    return true;
}

bool PlotWindow::addAtPeak(int item, int peak, bool artifact) {
    const PaneData &d = paneData(item);
    if (peak < 0 || peak >= d.peaks.size()) return false;
    const PeakData &pk = d.peaks[peak];
    const QByteArray tag = pk.tag.toUtf8();
    const QString marker = q(backend_.marker_at(static_cast<std::size_t>(d.sample), rust::Str(tag.constData(), tag.size()), pk.size));
    if (marker.isEmpty() && !artifact) { QMessageBox::information(this, "OpenTrace", QString::fromUtf8("Пик вне диапазонов маркеров.")); return false; }
    bool ok = false;
    const QString v = QInputDialog::getText(this, artifact ? QString::fromUtf8("Добавить артефакт") : QString::fromUtf8("Добавить аллель"),
                                            QString::fromUtf8("Метка для пика %1 п.н. (%2):").arg(pk.size, 0, 'f', 2).arg(marker.isEmpty() ? pk.tag : marker),
                                            QLineEdit::Normal, artifact ? QString("SPK") : QString(), &ok).trimmed();
    if (!ok || v.isEmpty() || v.size() > 40) return false;
    QJsonObject peakObj{{"channel", pk.tag}, {"size_bp", pk.size}, {"height", pk.height}, {"scan", pk.scan}};
    QJsonObject spec{{"action", artifact ? "add_artifact" : "add"}, {"marker", marker}, {"peak", peakObj}, {"new", v}};
    const QString reason = askReason(QString::fromUtf8("%1 «%2» (%3, %4 п.н.)").arg(artifact ? QString::fromUtf8("Добавить артефакт") : QString::fromUtf8("Добавить аллель"), v, marker).arg(pk.size, 0, 'f', 2));
    if (reason.isEmpty()) return false;
    const QByteArray sp = jsonText(spec).toUtf8(), rs = reason.trimmed().toUtf8();
    const auto err = backend_.apply_edit_json(static_cast<std::size_t>(d.sample), rust::Str(sp.constData(), sp.size()), rust::Str(rs.constData(), rs.size()));
    if (!err.empty()) { QMessageBox::warning(this, "OpenTrace", q(err)); return false; }
    afterEdit();
    return true;
}

void PlotWindow::afterEdit() {
    load();
    fillLower();
    sel_ = PaneSelection{};
    selItem_ = -1;
    repaintPanes();
    if (onEdited) onEdited();
}

void PlotWindow::editSelected(const QString &action) {
    if (selItem_ < 0) return;
    if (sel_.label >= 0) editLabel(selItem_, sel_.label, action);
    else if (sel_.peak >= 0 && (action == "add" || action == "add_artifact")) addAtPeak(selItem_, sel_.peak, action == "add_artifact");
}

void PlotWindow::editMenu(int item, const QPoint &globalPos) {
    QMenu m(this);
    const PaneData &d = paneData(item);
    if (sel_.label >= 0) {
        const LabelData &l = d.labels[sel_.label];
        const bool art = l.kind == "art";
        m.addSection(QString::fromUtf8("%1 · %2").arg(l.marker, l.text));
        m.addAction(QString::fromUtf8("Переименовать..."), this, [this] { editSelected("rename"); })->setShortcut(QKeySequence("F2"));
        if (art) m.addAction(QString::fromUtf8("Сделать аллелью..."), this, [this] { editSelected("artifact_to_allele"); });
        else m.addAction(QString::fromUtf8("Заменить на артефакт..."), this, [this] { editSelected("to_artifact"); });
        m.addAction(QString::fromUtf8("Удалить метку"), this, [this] { editSelected("delete"); })->setShortcut(QKeySequence::Delete);
    } else if (sel_.peak >= 0) {
        m.addSection(QString::fromUtf8("Пик %1 п.н.").arg(d.peaks[sel_.peak].size, 0, 'f', 2));
        m.addAction(QString::fromUtf8("Добавить аллель..."), this, [this] { editSelected("add"); });
        m.addAction(QString::fromUtf8("Добавить артефакт..."), this, [this] { editSelected("add_artifact"); });
    }
    m.exec(globalPos);
}


// Marker header: Override GQ, Partial Locus Indicator; sample header: Override CGQ (ID-X plot popups).
void PlotWindow::headerMenu(int item, const QString &marker, const QPoint &globalPos) {
    const PaneData &d = paneData(item);
    if (!backend_.sample_editable(static_cast<std::size_t>(d.sample))) return;
    QMenu m(this);
    auto apply = [this, &d](const QJsonObject &spec, const QString &what) {
        const QString reason = askReason(what);
        if (reason.isEmpty()) return;
        const QByteArray sp = jsonText(spec).toUtf8(), rs = reason.trimmed().toUtf8();
        const auto err = backend_.apply_edit_json(static_cast<std::size_t>(d.sample), rust::Str(sp.constData(), sp.size()), rust::Str(rs.constData(), rs.size()));
        if (!err.empty()) { QMessageBox::warning(this, "OpenTrace", q(err)); return; }
        afterEdit();
    };
    if (!marker.isEmpty()) {
        const QByteArray mk = marker.toUtf8();
        const bool pli = backend_.pli_on(static_cast<std::size_t>(d.sample), rust::Str(mk.constData(), mk.size()));
        m.addSection(marker);
        m.addAction(QString::fromUtf8("Переопределить GQ"), this, [=] { apply(QJsonObject{{"action", "override_gq"}, {"marker", marker}}, QString::fromUtf8("Переопределить GQ (%1)").arg(marker)); });
        auto *a = m.addAction(QString::fromUtf8("Частичный локус (PLI)"), this, [=] {
            apply(QJsonObject{{"action", "pli"}, {"marker", marker}, {"new", pli ? "off" : "on"}}, QString::fromUtf8("PLI %1 (%2)").arg(pli ? QString::fromUtf8("снять") : QString::fromUtf8("отметить"), marker));
        });
        a->setCheckable(true);
        a->setChecked(pli);
    } else {
        m.addSection(d.name.isEmpty() ? d.file : d.name);
        m.addAction(QString::fromUtf8("Переопределить CGQ"), this, [=] { apply(QJsonObject{{"action", "override_cgq"}}, QString::fromUtf8("Переопределить CGQ")); });
    }
    m.exec(globalPos);
}


// Lower table of the plot window: Sizing (every sized peak of the shown dyes: dye/peak, file, marker, allele, size,
// height, area, data point) or Genotypes (the samples' markers) or none.
void PlotWindow::fillLower() {
    lower_->setVisible(table_ != "none");
    lower_->clear();
    lower_->setRowCount(0);
    if (table_ == "none") return;
    static const QHash<QString, QString> letter{{"DATA1", "B"}, {"DATA2", "G"}, {"DATA3", "Y"}, {"DATA4", "R"}, {"DATA105", "O"}, {"DATA106", "P"}};
    if (table_ == "sizing") {
        // columns of the plot setting's Sizing Table tab (ID-X order), its font
        static const QList<QPair<QString, QString>> COLS{{"sampleFileName", "Файл образца"}, {"marker", "Маркер"}, {"dyeSamplePeak", "Краситель/пик"}, {"minutes", "Минуты"},
                                                         {"size", "Размер"}, {"height", "Высота"}, {"area", "Площадь"}, {"dataPoint", "Точка данных"}, {"alleleCall", "Аллель"}};
        QStringList keys, head;
        for (const auto &c : COLS) if (sizingCols_.isEmpty() || sizingCols_.contains(c.first)) { keys << c.first; head << c.second; }
        QFont tf = lower_->font();
        tf.setFamily(sizingFont_ == "Dialog" || sizingFont_ == "SansSerif" ? QString("Arial") : sizingFont_ == "Serif" ? QString("Times New Roman") : sizingFont_ == "Monospaced" || sizingFont_ == "DialogInput" ? QString("Courier New") : sizingFont_);
        tf.setPixelSize(std::max(8, sizingSize_ + 2));
        lower_->setFont(tf);
        lower_->setColumnCount(head.size());
        lower_->setHorizontalHeaderLabels(head);
        int row = 0;
        for (int di = 0; di < panes_.size(); ++di) {
            const PaneData &d = panes_[di];
            for (const char *dye : DYES) {
                const QString tag = dye;
                if (!view_.dyes.contains(tag)) continue;
                QVector<int> idx;
                for (int k = 0; k < d.peaks.size(); ++k) if (d.peaks[k].tag == tag) idx << k;
                std::sort(idx.begin(), idx.end(), [&](int a, int b) { return d.peaks[a].scan < d.peaks[b].scan; });
                int n = 0;
                for (int k : idx) {
                    const PeakData &pk = d.peaks[k];
                    ++n;
                    const LabelData *lab = nullptr;
                    for (const auto &l : d.labels) if (l.kind == "call" && l.tag == tag && std::abs(l.x - pk.x) < 0.05) { lab = &l; break; }
                    lower_->insertRow(row);
                    const QHash<QString, QString> v{{"sampleFileName", d.file}, {"marker", lab ? lab->marker : QString()}, {"dyeSamplePeak", QString("%1,%2").arg(letter.value(tag, tag)).arg(n)},
                                                    {"minutes", QString()}, {"size", QString::number(pk.size, 'f', 2)}, {"height", QString::number(std::lround(pk.height))},
                                                    {"area", QString::number(std::lround(pk.area))}, {"dataPoint", QString::number(std::lround(pk.scan))}, {"alleleCall", lab ? lab->text : QString()}};
                    for (int c = 0; c < keys.size(); ++c) {
                        auto *it = new QTableWidgetItem(v.value(keys[c]));
                        if (keys[c] == "dyeSamplePeak") it->setIcon(colorIcon(dyeSwatch(tag)));
                        it->setData(Qt::UserRole, di);
                        it->setData(Qt::UserRole + 1, k);
                        lower_->setItem(row, c, it);
                    }
                    ++row;
                }
            }
        }
    } else {
        lower_->setFont(QFont());
        QStringList head{QString::fromUtf8("Имя образца"), QString::fromUtf8("Маркер"), QString::fromUtf8("Аллель 1"), QString::fromUtf8("Аллель 2"),
                         QString::fromUtf8("Аллель 3"), QString::fromUtf8("Аллель 4"), "GQ"};
        lower_->setColumnCount(head.size());
        lower_->setHorizontalHeaderLabels(head);
        QSet<int> shown;
        for (const auto &d : panes_) shown.insert(d.sample);
        int row = 0;
        for (const auto &g : backend_.genotype_rows()) {
            if (!shown.contains(static_cast<int>(g.sample))) continue;
            lower_->insertRow(row);
            lower_->setItem(row, 0, new QTableWidgetItem(q(g.name)));
            lower_->setItem(row, 1, new QTableWidgetItem(q(g.marker)));
            for (int k = 0; k < 4; ++k) lower_->setItem(row, 2 + k, new QTableWidgetItem(static_cast<std::size_t>(k) < g.alleles.size() ? q(g.alleles[k]) : QString()));
            auto *gq = new QTableWidgetItem;
            gq->setIcon(qualityIcon(q(g.gq)));
            gq->setToolTip(q(g.gq));
            lower_->setItem(row, 6, gq);
            ++row;
        }
    }
    lower_->resizeColumnsToContents();
}

} // namespace ot

