#include "opentrace-qt/cpp/app.h"
#include "opentrace-qt/cpp/mainwindow.h"
#include <QApplication>
#include <QIcon>

namespace ot {
std::int32_t run_app() {
    static int argc = 1;
    static char name[] = "opentrace-qt";
    static char *argv[] = {name, nullptr};
    QApplication app(argc, argv);
    QApplication::setApplicationName("OpenTrace Desktop");
    QApplication::setOrganizationName("OpenTrace");
    QApplication::setApplicationVersion("0.1.0");
    MainWindow w;
    w.show();
    return app.exec();
}
}
