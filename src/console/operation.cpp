#include <kotavel/console/runtime.hpp>
#include <iostream>

namespace kotavel {
// Presentación de una operación de consola; Application conserva el diagnóstico.
OperationResult execute_console(Application& application, std::string_view id,
                                const std::function<void()>& operation) {
    const auto result = application.execute(id, operation);
    if (const auto presentation = application.present_error())
        std::cerr << presentation->console_text();
    if (application.last_error() && application.last_error()->report_status == ReportStatus::failed)
        std::cerr << "Aviso: no se pudo guardar el diagnóstico.\n";
    return result;
}
}
