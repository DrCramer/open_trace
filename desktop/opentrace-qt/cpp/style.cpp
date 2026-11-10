#include "opentrace-qt/cpp/style.h"
#include <QPainter>
#include <QPixmap>
#include <QHash>

namespace ot {

QColor dyeColor(const QString &tag) {
    static const QHash<QString, QColor> c{{"DATA1", QColor(0, 0, 255)}, {"DATA2", QColor(0, 160, 0)}, {"DATA3", QColor(0, 0, 0)},
                                          {"DATA4", QColor(255, 0, 0)}, {"DATA105", QColor(255, 128, 0)}, {"DATA106", QColor(176, 0, 208)}};
    return c.value(tag, QColor(51, 51, 51));
}

QColor dyeSwatch(const QString &tag) { return tag == "DATA3" ? QColor(229, 211, 0) : dyeColor(tag); }

QString dyeName(const QString &tag) {
    static const QHash<QString, QString> n{{"DATA1", "Blue"}, {"DATA2", "Green"}, {"DATA3", "Yellow"}, {"DATA4", "Red"}, {"DATA105", "Orange"}, {"DATA106", "Purple"}};
    return n.value(tag, tag);
}

QIcon colorIcon(const QColor &c) {
    QPixmap pm(16, 16);
    pm.fill(Qt::transparent);
    QPainter p(&pm);
    p.setPen(QColor(60, 60, 60));
    p.setBrush(c);
    p.drawRect(2, 2, 11, 11);
    return QIcon(pm);
}

// Quality symbol in a 16-unit box scaled to r: pass square, check triangle, low circle with a bar, override square with a bar.
void paintQuality(QPainter &p, const QRectF &r, const QString &q) {
    if (q != "pass" && q != "check" && q != "low" && q != "override") return;
    p.save();
    p.translate(r.topLeft());
    p.scale(r.width() / 16.0, r.height() / 16.0);
    p.setRenderHint(QPainter::Antialiasing);
    QPen edge;
    edge.setCosmetic(false);
    edge.setWidthF(1.0);
    if (q == "pass" || q == "override") {
        edge.setColor(QColor(0, 110, 0));
        p.setPen(edge);
        p.setBrush(QColor(60, 190, 60));
        p.drawRoundedRect(QRectF(2.5, 3.5, 11, 9), 2, 2);
        if (q == "override") {
            p.setPen(QPen(Qt::white, 1.6));
            p.drawLine(QPointF(5, 8), QPointF(11, 8));
        }
    } else if (q == "check") {
        edge.setColor(QColor(140, 100, 0));
        p.setPen(edge);
        p.setBrush(QColor(255, 200, 0));
        p.drawPolygon(QPolygonF{QPointF(8, 2), QPointF(14.5, 14), QPointF(1.5, 14)});
    } else {
        edge.setColor(QColor(150, 0, 0));
        p.setPen(edge);
        p.setBrush(QColor(220, 30, 30));
        p.drawEllipse(QRectF(2, 2, 12, 12));
        p.setPen(QPen(Qt::white, 2));
        p.drawLine(QPointF(5, 8), QPointF(11, 8));
    }
    p.restore();
}

QIcon qualityIcon(const QString &q) {
    if (q != "pass" && q != "check" && q != "low" && q != "override") return QIcon();
    QPixmap pm(16, 16);
    pm.fill(Qt::transparent);
    QPainter p(&pm);
    paintQuality(p, QRectF(0, 0, 16, 16), q);
    return QIcon(pm);
}

QString fromRust(const char *data, std::size_t len) { return QString::fromUtf8(data, static_cast<int>(len)); }

} // namespace ot
