// Plot Settings Editor (ID-X: General, Sample Header, Genotype Header, Sizing Table, Labels, Display Settings) and the
// Plot Settings list of the GeneMapper Manager. The setting is the web JSON; the Rust side validates and stores it.
#pragma once
#include <QDialog>
#include <QJsonObject>
#include <QString>
#include <functional>

class QTabWidget;
class QTableWidget;

namespace ot {
struct Backend;

class PlotSettingsEditor : public QDialog {
    Q_OBJECT
public:
    // previous: the stored name of the setting ("" for a new one); readOnly shows it without saving
    PlotSettingsEditor(const Backend &backend, const QJsonObject &setting, const QString &previous, bool readOnly, QWidget *parent = nullptr);
    QString savedName() const { return saved_; }
    void accept() override;
private:
    const Backend &backend_;
    QJsonObject s_;
    QString previous_, saved_;
    bool ro_;
    QTabWidget *tabs_ = nullptr;
    std::function<void()> collect_;   // widgets -> s_
    void build();
};

class PlotSettingsManager : public QDialog {
    Q_OBJECT
public:
    explicit PlotSettingsManager(const Backend &backend, QWidget *parent = nullptr);
    std::function<void()> onChanged;
private:
    const Backend &backend_;
    QTableWidget *table_ = nullptr;
    void fill(const QString &select = QString());
    QString current() const;
    void edit(const QString &name);
    void create();
    void saveAs();
    void remove();
    void importFile();
    void exportFile();
};

QJsonObject parseJsonObject(const QString &text);
}
