use cxx_qt_build::CxxQtBuilder;

fn main() {
    let cpp = ["cpp/app.h", "cpp/app.cpp", "cpp/mainwindow.h", "cpp/mainwindow.cpp", "cpp/samplemodel.h", "cpp/samplemodel.cpp", "cpp/plotwindow.h", "cpp/plotwindow.cpp", "cpp/style.h", "cpp/style.cpp", "cpp/simpleplot.h", "cpp/simpleplot.cpp", "cpp/plotsettingsdialog.h", "cpp/plotsettingsdialog.cpp", "cpp/tablesettingsdialog.h", "cpp/tablesettingsdialog.cpp"];
    unsafe {
        CxxQtBuilder::new()
            .qt_module("Widgets")
            .qt_module("PrintSupport")
            .file("src/bridge.rs")
            .cpp_files(cpp)
            .cc_builder(|cc| {
                cc.flag_if_supported("-std=c++17");
                cc.flag_if_supported("/std:c++17");
                cc.flag_if_supported("/Zc:__cplusplus");
                cc.flag_if_supported("/permissive-");
            })
    }
    .build();
}
