#include "opentrace-qt/cpp/simpleplot.h"
#include <QLabel>
#include <QMouseEvent>
#include <QPainter>
#include <QPainterPath>
#include <cmath>
#include <algorithm>

namespace ot {

static double nice(double span) {
    if (span <= 0) return 1;
    const double mag = std::pow(10.0, std::floor(std::log10(span))), f = span / mag;
    return (f < 1.5 ? 1 : f < 3.5 ? 2 : f < 7.5 ? 5 : 10) * mag;
}

SimplePlot::SimplePlot(QWidget *parent) : QWidget(parent) {
    setMouseTracking(true);
    setMinimumHeight(110);
}

void SimplePlot::setSeries(const QVector<Series> &s, const QString &xTitle) {
    series_ = s;
    xTitle_ = xTitle;
    x0_ = x1_ = 0;
    update();
}

void SimplePlot::setVisible(int i, bool on) {
    if (i >= 0 && i < series_.size()) { series_[i].visible = on; update(); }
}

void SimplePlot::range(double &a, double &b) const {
    if (x1_ > x0_) { a = x0_; b = x1_; return; }
    int n = 1;
    for (const auto &s : series_) n = std::max(n, int(s.ys.size()));
    a = 0;
    b = n - 1;
}

double SimplePlot::xAt(double px) const {
    double a, b;
    range(a, b);
    return a + (px - L()) / (R() - L()) * (b - a);
}

void SimplePlot::paintEvent(QPaintEvent *) {
    QPainter p(this);
    p.fillRect(rect(), Qt::white);
    double a, b;
    range(a, b);
    if (b <= a) return;
    const double top = 8, bottom = height() - 22, l = L(), r = R();
    double ymin = 0, ymax = 1;
    for (const auto &s : series_) {
        if (!s.visible) continue;
        for (int i = std::max(0, int(a)); i <= std::min(int(s.ys.size()) - 1, int(std::ceil(b))); ++i) { ymax = std::max(ymax, s.ys[i]); ymin = std::min(ymin, s.ys[i]); }
    }
    ymax *= 1.05;
    auto X = [&](double x) { return l + (x - a) / (b - a) * (r - l); };
    auto Y = [&](double y) { return bottom - (y - ymin) / (ymax - ymin) * (bottom - top); };
    p.setPen(Qt::black);
    p.drawRect(QRectF(l, top, r - l, bottom - top));
    const double sx = nice((b - a) / 8);
    for (double x = std::ceil(a / sx) * sx; x <= b; x += sx) {
        p.drawLine(QPointF(X(x), bottom), QPointF(X(x), bottom + 4));
        p.drawText(QRectF(X(x) - 40, bottom + 4, 80, 16), Qt::AlignHCenter | Qt::AlignTop, QString::number(x, 'g', 7));
    }
    const double sy = nice((ymax - ymin) / 3);
    for (double y = std::ceil(ymin / sy) * sy; y <= ymax; y += sy) {
        p.drawLine(QPointF(l - 4, Y(y)), QPointF(l, Y(y)));
        p.drawText(QRectF(0, Y(y) - 8, l - 6, 16), Qt::AlignRight | Qt::AlignVCenter, QString::number(std::lround(y)));
    }
    p.drawText(QRectF(r - 200, bottom + 4, 200, 16), Qt::AlignRight | Qt::AlignTop, xTitle_);
    p.save();
    p.setClipRect(QRectF(l, top, r - l, bottom - top));
    p.setRenderHint(QPainter::Antialiasing);
    for (const auto &s : series_) {
        if (!s.visible || s.ys.isEmpty()) continue;
        p.setPen(QPen(s.color, 1));
        QPainterPath path;
        bool started = false;
        double col = -1e300, mn = 0, mx = 0;
        auto flush = [&] {
            if (col < -1e299) return;
            if (!started) { path.moveTo(col, Y(mn)); started = true; } else path.lineTo(col, Y(mn));
            path.lineTo(col, Y(mx));
        };
        for (int i = std::max(0, int(a) - 1); i < s.ys.size() && i <= int(b) + 1; ++i) {
            const double c = std::floor(X(i));
            if (c != col) { flush(); col = c; mn = mx = s.ys[i]; } else { mn = std::min(mn, s.ys[i]); mx = std::max(mx, s.ys[i]); }
        }
        flush();
        p.drawPath(path);
    }
    p.restore();
    if (drag_ >= 0) p.fillRect(QRect(QPoint(std::min(drag_, dragTo_), int(top)), QPoint(std::max(drag_, dragTo_), int(bottom))), QColor(10, 36, 106, 40));
}

void SimplePlot::mousePressEvent(QMouseEvent *e) { if (e->button() == Qt::LeftButton) drag_ = dragTo_ = int(e->position().x()); }
void SimplePlot::mouseMoveEvent(QMouseEvent *e) {
    if (drag_ >= 0) { dragTo_ = int(e->position().x()); update(); }
    if (readout_) {
        const int x = int(std::lround(xAt(e->position().x())));
        QStringList parts{QString::fromUtf8("Точка %1").arg(x)};
        for (const auto &s : series_) if (s.visible && x >= 0 && x < s.ys.size()) parts << QString("%1: %2").arg(s.name).arg(s.ys[x]);
        readout_->setText(parts.join("    "));
    }
}
void SimplePlot::mouseReleaseEvent(QMouseEvent *) {
    if (drag_ >= 0 && std::abs(dragTo_ - drag_) > 4) {
        const double a = xAt(std::min(drag_, dragTo_)), b = xAt(std::max(drag_, dragTo_));
        x0_ = std::max(0.0, a);
        x1_ = b;
    }
    drag_ = dragTo_ = -1;
    update();
}
void SimplePlot::mouseDoubleClickEvent(QMouseEvent *) { x0_ = x1_ = 0; update(); }

} // namespace ot
