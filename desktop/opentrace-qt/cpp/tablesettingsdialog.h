// Table Setting Editor (ID-X: name; Samples and Genotypes tabs with the column list Show / Column / Filtering / Content,
// Show / Hide buttons, three sort keys, number of alleles) and the Table Settings list of the GeneMapper Manager.
// The setting is the web JSON; the Rust side validates and stores it.
#pragma once
#include <QDialog>
#include <QJsonObject>
#include <QString>
#include <functional>

class QLineEdit;
class QTabWidget;
class QTableWidget;

namespace ot {
struct Backend;

class TableSettingEditor : public QDialog {
    Q_OBJECT
public:
    // previous: the stored name of the setting ("" for a new one)
    TableSettingEditor(const Backend &backend, const QJsonObject &setting, const QString &previous, QWidget *parent = nullptr);
    QString savedName() const { return saved_; }
    void accept() override;
private:
    const Backend &backend_;
    QJsonObject s_;
    QString previous_, saved_;
    QLineEdit *name_ = nullptr;
    QTabWidget *tabs_ = nullptr;
    QList<std::function<void()>> collect_;   // widgets -> s_
    QWidget *page(const QString &kind);
};

class TableSettingsManager : public QDialog {
    Q_OBJECT
public:
    explicit TableSettingsManager(const Backend &backend, QWidget *parent = nullptr);
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
}
