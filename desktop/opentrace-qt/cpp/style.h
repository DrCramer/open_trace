// Colours of the dyes (ID-X palette) and of the quality symbols, shared by the tables and the plot window.
#pragma once
#include <QColor>
#include <QIcon>
#include <QString>
#include <QRectF>
class QPainter;

namespace ot {
QColor dyeColor(const QString &tag);            // trace colour on the white (ID-X) plot field
QColor dyeSwatch(const QString &tag);           // button / legend swatch (DATA3 shown yellow, as in ID-X)
QString dyeName(const QString &tag);            // Blue, Green, Yellow, Red, Orange, Purple
QIcon qualityIcon(const QString &q);            // pass / check / low / override / na
void paintQuality(QPainter &p, const QRectF &r, const QString &q);   // the same symbol as vector drawing (headers, printing)
QIcon colorIcon(const QColor &c);
QString fromRust(const char *data, std::size_t len);
}
