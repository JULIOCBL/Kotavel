#include "kotavel/src/desktop/qt/bridge.cxx.h"
#include "kotavel/src/desktop/qt/qt_host.h"

#include <QByteArray>
#include <QEvent>
#include <QFileOpenEvent>
#include <QGuiApplication>
#include <QIcon>
#include <QQmlError>
#include <QQmlEngine>
#include <QObject>
#include <QQuickView>
#include <QString>
#include <QStringList>
#include <QUrl>

#include <map>
#include <stdexcept>
#include <utility>
#include <vector>

namespace kotavel::desktop {
namespace {

QString to_qstring(rust::Str value) {
    return QString::fromUtf8(value.data(), static_cast<qsizetype>(value.size()));
}

class KotavelGuiApplication final : public QGuiApplication {
public:
    KotavelGuiApplication(int& argc, char** argv) : QGuiApplication(argc, argv) {}

    void set_deep_link_handler(DeepLinkDispatcher* handler) noexcept {
        deep_link_handler_ = handler;
    }

protected:
    bool event(QEvent* event) override {
        if (event->type() == QEvent::FileOpen && deep_link_handler_) {
            const auto* open_event = static_cast<const QFileOpenEvent*>(event);
            const auto url = open_event->url();
            if (url.isValid() && !url.scheme().isEmpty()) {
                const auto encoded = url.toString(QUrl::FullyEncoded).toUtf8();
                dispatch_deep_link_callback(
                    *deep_link_handler_,
                    rust::Str(encoded.constData(), static_cast<std::size_t>(encoded.size())));
                return true;
            }
        }
        return QGuiApplication::event(event);
    }

private:
    DeepLinkDispatcher* deep_link_handler_ = nullptr;
};

} // namespace

struct QtHost::State final {
    // The argument storage and pointer array outlive QGuiApplication.
    std::vector<QByteArray> argument_storage;
    std::vector<char*> argument_pointers;
    int argument_count = 0;
    std::unique_ptr<KotavelGuiApplication> application;
    std::map<std::string, std::unique_ptr<QQuickView>, std::less<>> windows;
};

QtHost::QtHost(rust::Vec<rust::String> arguments) : state_(std::make_unique<State>()) {
    state_->argument_storage.reserve(arguments.size() + 1);
    state_->argument_storage.emplace_back("kotavel");
    for (const auto& argument : arguments) {
        state_->argument_storage.emplace_back(argument.data(), argument.size());
    }

    state_->argument_pointers.reserve(state_->argument_storage.size() + 1);
    for (auto& argument : state_->argument_storage)
        state_->argument_pointers.push_back(argument.data());
    state_->argument_pointers.push_back(nullptr);
    state_->argument_count = static_cast<int>(state_->argument_storage.size());
    state_->application = std::make_unique<KotavelGuiApplication>(
        state_->argument_count, state_->argument_pointers.data());
    state_->application->setQuitOnLastWindowClosed(true);
}

QtHost::~QtHost() = default;

void QtHost::show(rust::Str id, rust::Str title, std::uint32_t width,
                  std::uint32_t height, rust::Str source, rust::Str icon_path) const {
    const std::string key(id.data(), id.size());
    auto& window = state_->windows[key];
    if (!window) {
        window = std::make_unique<QQuickView>();
        QObject::connect(window->engine(), &QQmlEngine::quit,
                         state_->application.get(), &QCoreApplication::quit,
                         Qt::QueuedConnection);
    }

    const auto window_title = to_qstring(title);
    QGuiApplication::setApplicationName(window_title);
    QGuiApplication::setApplicationDisplayName(window_title);
    window->setTitle(window_title);
    const auto icon_file = to_qstring(icon_path);
    if (!icon_file.isEmpty()) {
        const QIcon icon(icon_file);
        QGuiApplication::setWindowIcon(icon);
        window->setIcon(icon);
    }
    window->resize(static_cast<int>(width), static_cast<int>(height));
    window->setResizeMode(QQuickView::SizeRootObjectToView);
    window->setSource(QUrl::fromLocalFile(to_qstring(source)));
    if (window->status() == QQuickView::Error) {
        QStringList errors;
        for (const auto& error : window->errors()) errors.push_back(error.toString());
        throw std::runtime_error("Could not load desktop view '" + key + "': " +
                                 errors.join('\n').toStdString());
    }
    window->show();
}

void QtHost::close(rust::Str id) const noexcept {
    const auto found = state_->windows.find(std::string(id.data(), id.size()));
    if (found != state_->windows.end()) found->second->close();
}

bool QtHost::is_visible(rust::Str id) const noexcept {
    const auto found = state_->windows.find(std::string(id.data(), id.size()));
    return found != state_->windows.end() && found->second->isVisible();
}

bool QtHost::has_visible_window() const noexcept {
    for (const auto& [id, window] : state_->windows) {
        (void)id;
        if (window->isVisible()) return true;
    }
    return false;
}

int QtHost::exec(DeepLinkDispatcher& deep_link) const {
    state_->application->set_deep_link_handler(&deep_link);
    const int result = state_->application->exec();
    state_->application->set_deep_link_handler(nullptr);
    return result;
}

void QtHost::quit() const noexcept { state_->application->quit(); }

std::unique_ptr<QtHost> create_qt_host(rust::Vec<rust::String> arguments) {
    return std::make_unique<QtHost>(std::move(arguments));
}

} // namespace kotavel::desktop
