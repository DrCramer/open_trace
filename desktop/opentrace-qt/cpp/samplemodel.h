// Samples table model: rows are read from the Rust backend (one snapshot per refresh).
#pragma once
#include <QAbstractTableModel>
#include <QVector>
#include <QStringList>
#include "rust/cxx.h"

namespace ot {
struct Backend;

struct RowView {
    QString file, name, runFolder, status, sq, method, sizeStd, instrument, well, error, stype, panel, sos, sspk, cgq;
};

class SampleModel : public QAbstractTableModel {
    Q_OBJECT
public:
    enum Column { Status, SampleFile, SampleName, SampleType, RunFolder, Well, Panel, Method, SizeStd, SOS, SQ, SSPK, CGQ, Instrument, ColumnCount };
    explicit SampleModel(const Backend &backend, QObject *parent = nullptr);
    int rowCount(const QModelIndex &parent = QModelIndex()) const override;
    int columnCount(const QModelIndex &parent = QModelIndex()) const override;
    QVariant data(const QModelIndex &index, int role) const override;
    QVariant headerData(int section, Qt::Orientation orientation, int role) const override;
    void refresh();                 // reread all rows (structure may have changed)
    void refreshValues();           // same rows, new values
    const RowView &row(int i) const { return rows_[i]; }

private:
    const Backend &backend_;
    QVector<RowView> rows_;
    void load();
};
}
