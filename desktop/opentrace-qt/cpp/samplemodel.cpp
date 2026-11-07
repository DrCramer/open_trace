#include "opentrace-qt/cpp/samplemodel.h"
#include "opentrace-qt/cpp/style.h"
#include "opentrace-qt/src/bridge.cxx.h"
#include <QIcon>
#include <QBrush>
#include <QColor>

namespace ot {

static QString q(const rust::String &s) { return QString::fromUtf8(s.data(), static_cast<int>(s.size())); }

SampleModel::SampleModel(const Backend &backend, QObject *parent) : QAbstractTableModel(parent), backend_(backend) { load(); }

void SampleModel::load() {
    rows_.clear();
    const std::size_t n = backend_.sample_count();
    rows_.reserve(static_cast<int>(n));
    for (std::size_t i = 0; i < n; ++i) {
        SampleRow r = backend_.sample_row(i);
        rows_.push_back(RowView{q(r.file), q(r.name), q(r.run_folder), q(r.status), q(r.sq), q(r.method), q(r.size_std), q(r.instrument), q(r.well), q(r.error), q(r.stype), q(r.panel), q(r.sos), q(r.sspk), q(r.cgq), q(r.mix), q(r.omr), q(r.se)});
    }
}

void SampleModel::refresh() {
    beginResetModel();
    load();
    endResetModel();
}

void SampleModel::refreshValues() {
    if (static_cast<std::size_t>(rows_.size()) != backend_.sample_count()) { refresh(); return; }
    load();
    if (!rows_.isEmpty()) emit dataChanged(index(0, 0), index(rows_.size() - 1, ColumnCount - 1));
}

int SampleModel::rowCount(const QModelIndex &parent) const { return parent.isValid() ? 0 : rows_.size(); }
int SampleModel::columnCount(const QModelIndex &parent) const { return parent.isValid() ? 0 : ColumnCount; }

QVariant SampleModel::headerData(int section, Qt::Orientation orientation, int role) const {
    if (role != Qt::DisplayRole) return {};
    if (orientation == Qt::Vertical) return section + 1;
    static const char *h[] = {"Статус", "Файл образца", "Имя образца", "Тип образца", "Папка запуска", "Лунка", "Панель", "Метод анализа", "Стандарт длин", "SOS", "SQ", "SSPK", "MIX", "OMR", "CGQ", "Прибор"};
    return QString::fromUtf8(h[section]);
}

QVariant SampleModel::data(const QModelIndex &index, int role) const {
    if (!index.isValid() || index.row() >= rows_.size()) return {};
    const RowView &r = rows_[index.row()];
    const int c = index.column();
    if (role == Qt::DisplayRole) {
        switch (c) {
        case Status:
            if (r.status == "analyzed") return QString::fromUtf8("Проанализирован");
            if (r.status == "running") return QString::fromUtf8("Анализ…");
            if (r.status == "failed") return QString::fromUtf8("Ошибка");
            return QString::fromUtf8("Не проанализирован");
        case SampleFile: return r.file;
        case SampleName: return r.name;
        case SampleType:
            if (r.stype == "ladder") return QString::fromUtf8("Аллельная лестница");
            if (r.stype == "positive") return QString::fromUtf8("Положительный контроль");
            if (r.stype == "negative") return QString::fromUtf8("Отрицательный контроль");
            return QString::fromUtf8("Образец");
        case Panel: return r.panel;
        case SOS: case SSPK: case MIX: case OMR: case CGQ: return {};
        case RunFolder: return r.runFolder;
        case Well: return r.well;
        case Method: return r.method;
        case SizeStd: return r.sizeStd;
        case SQ: return {};
        case Instrument: return r.instrument;
        }
    }
    if (role == Qt::DecorationRole) {
        if (c == SQ) return qualityIcon(r.sq);
        if (c == SOS) return qualityIcon(r.sos);
        if (c == SSPK) return qualityIcon(r.sspk);
        if (c == CGQ) return qualityIcon(r.cgq);
        if (c == MIX) return qualityIcon(r.mix);
        if (c == OMR) return qualityIcon(r.omr);
        if (c == Status && r.status == "failed") return qualityIcon("low");
    }
    if (role == Qt::ToolTipRole) {
        if (c == SQ) return r.sq;
        if (c == SOS) return r.sos;
        if (c == SSPK) return r.sspk;
        if (c == CGQ) return r.cgq;
        if (c == MIX) return r.mix;
        if (c == OMR) return r.omr;
        if (!r.error.isEmpty()) return r.error;
    }
    if (role == Qt::TextAlignmentRole && (c == SQ || c == SOS || c == SSPK || c == MIX || c == OMR || c == CGQ)) return int(Qt::AlignCenter);
    return {};
}

} // namespace ot
