// Simple trace plot of the Raw Data and EPT Data tabs: several series over data points, drag to zoom X,
// double click for the full view, cursor values in a label.
#pragma once
#include <QColor>
#include <QString>
#include <QVector>
#include <QWidget>

class QLabel;

namespace ot {
class SimplePlot : public QWidget {
    Q_OBJECT
public:
    struct Series { QString name; QColor color; QVector<double> ys; bool visible = true; };
    explicit SimplePlot(QWidget *parent = nullptr);
    void setSeries(const QVector<Series> &s, const QString &xTitle);
    void setVisible(int series, bool on);
    void setReadout(QLabel *l) { readout_ = l; }
    QSize sizeHint() const override { return QSize(800, 300); }
protected:
    void paintEvent(QPaintEvent *) override;
    void mousePressEvent(QMouseEvent *) override;
    void mouseMoveEvent(QMouseEvent *) override;
    void mouseReleaseEvent(QMouseEvent *) override;
    void mouseDoubleClickEvent(QMouseEvent *) override;
private:
    QVector<Series> series_;
    QString xTitle_;
    double x0_ = 0, x1_ = 0;   // x1 <= x0: full view
    int drag_ = -1, dragTo_ = -1;
    QLabel *readout_ = nullptr;
    double L() const { return 56; }
    double R() const { return width() - 10; }
    void range(double &a, double &b) const;
    double xAt(double px) const;
};
}
