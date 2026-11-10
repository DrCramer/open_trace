// Samples table model: columns, cells, filter and order come from the Rust backend as the active Table Setting
// describes them (one snapshot per refresh). Rows stay in project order; the proxy filters and sorts.
#pragma once
#include <QAbstractTableModel>
#include <QVector>
#include <QStringList>
#include "rust/cxx.h"

#include <QSortFilterProxyModel>
#include <QSet>

namespace ot {
struct Backend;

struct RowView {
    QString file, name, runFolder, status, sq, method, sizeStd, instrument, well, error, stype, panel, sos, sspk, cgq, mix, omr, se;
};

struct ColView {
    QString id, head, title, kind;   // kind: text | num | pqv | status | mark
    int width = 80;
};

class SampleModel : public QAbstractTableModel {
    Q_OBJECT
public:
    enum { SortRole = Qt::UserRole + 1 };
    explicit SampleModel(const Backend &backend, QObject *parent = nullptr);
    int rowCount(const QModelIndex &parent = QModelIndex()) const override;
    int columnCount(const QModelIndex &parent = QModelIndex()) const override;
    QVariant data(const QModelIndex &index, int role) const override;
    QVariant headerData(int section, Qt::Orientation orientation, int role) const override;
    void refresh();                 // reread everything (rows or columns may have changed)
    void refreshValues();           // same rows and columns, new values
    const RowView &row(int i) const { return rows_[i]; }
    const QVector<ColView> &columns() const { return cols_; }
    QString cell(int row, int col) const { return cells_[row].value(col); }
    QString exportText(int row, int col) const;
    bool shown(int row) const { return row < shown_.size() ? shown_[row] : true; }   // column filters of the table setting
    int rank(int row) const { return row < rank_.size() ? rank_[row] : row; }         // order of its sort keys
    int shownCount() const { return static_cast<int>(shown_.count(true)); }

private:
    const Backend &backend_;
    QVector<RowView> rows_;
    QVector<ColView> cols_;
    QVector<QStringList> cells_, tips_;
    QVector<bool> shown_;
    QVector<int> rank_;
    QStringList colIds() const;
    void load();
};

// Samples table proxy: the rows of an Analysis Summary link (empty = all), the column filters of the table setting, and
// the order of its sort keys until a column header is clicked.
class RowFilterProxy : public QSortFilterProxyModel {
    Q_OBJECT
public:
    using QSortFilterProxyModel::QSortFilterProxyModel;
    void setRows(const QSet<int> &rows, bool on) { rows_ = rows; on_ = on; invalidateFilter(); }
    bool active() const { return on_; }
    // order of the table setting (true) or of the clicked column (false)
    void setSettingOrder(bool on) { settingOrder_ = on; }
    bool settingOrder() const { return settingOrder_; }
    void reapply() { invalidate(); }
    // row numbers follow the shown order (the source model numbers its own rows)
    QVariant headerData(int section, Qt::Orientation o, int role) const override {
        if (o == Qt::Vertical && role == Qt::DisplayRole) return section + 1;
        return QSortFilterProxyModel::headerData(section, o, role);
    }
protected:
    bool filterAcceptsRow(int row, const QModelIndex &) const override {
        const auto *m = qobject_cast<const SampleModel *>(sourceModel());
        return (!on_ || rows_.contains(row)) && (!m || m->shown(row));
    }
    bool lessThan(const QModelIndex &a, const QModelIndex &b) const override {
        const auto *m = qobject_cast<const SampleModel *>(sourceModel());
        if (settingOrder_ && m) return m->rank(a.row()) < m->rank(b.row());
        const QVariant x = a.data(SampleModel::SortRole), y = b.data(SampleModel::SortRole);
        if (x.userType() == QMetaType::Double && y.userType() == QMetaType::Double) return x.toDouble() < y.toDouble();
        return QString::localeAwareCompare(x.toString(), y.toString()) < 0;
    }
private:
    QSet<int> rows_;
    bool on_ = false;
    bool settingOrder_ = true;
};
}
