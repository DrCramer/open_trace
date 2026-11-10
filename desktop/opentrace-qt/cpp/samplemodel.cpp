#include "opentrace-qt/cpp/samplemodel.h"
#include "opentrace-qt/cpp/style.h"
#include "opentrace-qt/src/bridge.cxx.h"
#include <QIcon>
#include <QBrush>
#include <QColor>

namespace ot {

static QString q(const rust::String &s) { return QString::fromUtf8(s.data(), static_cast<int>(s.size())); }

SampleModel::SampleModel(const Backend &backend, QObject *parent) : QAbstractTableModel(parent), backend_(backend) { load(); }

static QString statusText(const QString &s) {
    if (s == "analyzed") return QString::fromUtf8("Проанализирован");
    if (s == "running") return QString::fromUtf8("Анализ…");
    if (s == "failed") return QString::fromUtf8("Ошибка");
    return QString::fromUtf8("Не проанализирован");
}

void SampleModel::load() {
    rows_.clear();
    cols_.clear();
    cells_.clear();
    tips_.clear();
    shown_.clear();
    rank_.clear();
    for (const auto &c : backend_.table_columns("samples")) cols_.push_back(ColView{q(c.id), q(c.head), q(c.title), q(c.kind), c.width});
    const std::size_t n = backend_.sample_count();
    rows_.reserve(static_cast<int>(n));
    for (std::size_t i = 0; i < n; ++i) {
        SampleRow r = backend_.sample_row(i);
        rows_.push_back(RowView{q(r.file), q(r.name), q(r.run_folder), q(r.status), q(r.sq), q(r.method), q(r.size_std), q(r.instrument), q(r.well), q(r.error), q(r.stype), q(r.panel), q(r.sos), q(r.sspk), q(r.cgq), q(r.mix), q(r.omr), q(r.se)});
    }
    for (const auto &t : backend_.samples_table()) {
        QStringList c, tip;
        for (const auto &v : t.cells) c << q(v);
        for (const auto &v : t.tips) tip << q(v);
        cells_.push_back(c);
        tips_.push_back(tip);
        shown_.push_back(t.shown);
        rank_.push_back(static_cast<int>(t.rank));
    }
    while (cells_.size() < rows_.size()) { cells_.push_back({}); tips_.push_back({}); shown_.push_back(true); rank_.push_back(static_cast<int>(rank_.size())); }
}

QStringList SampleModel::colIds() const {
    QStringList l;
    for (const ColView &c : cols_) l << c.id;
    return l;
}

void SampleModel::refresh() {
    beginResetModel();
    load();
    endResetModel();
}

void SampleModel::refreshValues() {
    const QStringList before = colIds();
    const int n = rows_.size();
    QStringList now;
    for (const auto &c : backend_.table_columns("samples")) now << q(c.id);
    if (static_cast<std::size_t>(n) != backend_.sample_count() || before != now) { refresh(); return; }
    load();
    if (!rows_.isEmpty() && !cols_.isEmpty()) emit dataChanged(index(0, 0), index(rows_.size() - 1, cols_.size() - 1));
}

int SampleModel::rowCount(const QModelIndex &parent) const { return parent.isValid() ? 0 : rows_.size(); }
int SampleModel::columnCount(const QModelIndex &parent) const { return parent.isValid() ? 0 : cols_.size(); }

QVariant SampleModel::headerData(int section, Qt::Orientation orientation, int role) const {
    if (orientation == Qt::Vertical) return role == Qt::DisplayRole ? QVariant(section + 1) : QVariant();
    if (section < 0 || section >= cols_.size()) return {};
    if (role == Qt::DisplayRole) return cols_[section].head;
    if (role == Qt::ToolTipRole) return cols_[section].title;
    return {};
}

QString SampleModel::exportText(int row, int col) const {
    if (col < 0 || col >= cols_.size() || row < 0 || row >= cells_.size()) return {};
    const QString v = cells_[row].value(col);
    return cols_[col].kind == "status" ? statusText(v) : v;
}

QVariant SampleModel::data(const QModelIndex &index, int role) const {
    if (!index.isValid() || index.row() >= rows_.size() || index.column() >= cols_.size()) return {};
    const ColView &c = cols_[index.column()];
    const QString v = cells_[index.row()].value(index.column());
    const bool pqv = c.kind == "pqv";
    switch (role) {
    case Qt::DisplayRole:
        if (pqv) return {};
        return c.kind == "status" ? statusText(v) : v;
    case Qt::DecorationRole:
        if (pqv) return qualityIcon(v);
        if (c.kind == "status" && v == "failed") return qualityIcon("low");
        return {};
    case Qt::ToolTipRole: {
        const QString tip = tips_[index.row()].value(index.column());
        if (!tip.isEmpty()) return tip;
        if (pqv) return c.id + ": " + v;
        return rows_[index.row()].error.isEmpty() ? QVariant(v) : QVariant(rows_[index.row()].error);
    }
    case Qt::TextAlignmentRole:
        if (pqv) return int(Qt::AlignCenter);
        if (c.kind == "num") return int(Qt::AlignRight | Qt::AlignVCenter);
        return {};
    case SortRole: {
        bool ok = false;
        const double d = v.toDouble(&ok);
        if (ok) return d;
        return c.kind == "status" ? statusText(v) : v;
    }
    }
    return {};
}

} // namespace ot
