#include <kotavel/desktop/qt_platform.hpp>

#include <QColor>
#include <QGuiApplication>
#include <QIcon>
#include <QImage>
#include <QPainter>
#include <QPainterPath>
#include <QPixmap>
#include <QQmlError>
#include <QQuickView>
#include <QRectF>
#include <QResource>
#include <QStringList>
#include <QUrl>

#include <map>
#include <stdexcept>
#include <utility>

static void initialize_kotavel_branding_resources() {
    Q_INIT_RESOURCE(kotavel_branding);
}

namespace kotavel::desktop {
namespace {
QString to_qstring(std::string_view value) {
    return QString::fromUtf8(value.data(), static_cast<qsizetype>(value.size()));
}

QString to_qstring(const std::filesystem::path& path) {
    const auto utf8 = path.u8string();
    return QString::fromUtf8(reinterpret_cast<const char*>(utf8.data()),
                             static_cast<qsizetype>(utf8.size()));
}

QIcon default_app_icon() {
    initialize_kotavel_branding_resources();
    const QImage source{QStringLiteral(":/kotavel/branding/kotavel-mark.png")};
    if (source.isNull()) return {};

    // Conservar el símbolo y sus destellos completos en el icono.
    const QImage mark = source.copy(QRect(448, 180, 640, 640));
    QPixmap tile(256, 256);
    tile.fill(Qt::transparent);
    QPainter painter(&tile);
    painter.setRenderHint(QPainter::Antialiasing);
    painter.setRenderHint(QPainter::SmoothPixmapTransform);
    QPainterPath shape;
    shape.addRoundedRect(QRectF(0, 0, 256, 256), 52, 52);
    painter.fillPath(shape, QColor(QStringLiteral("#171a21")));
    painter.drawImage(QRect(16, 16, 224, 224), mark);
    return QIcon(tile);
}

class QtWindowManager final : public WindowManager {
public:
    QtWindowManager(std::filesystem::path views_root, QIcon icon)
        : views_root_(std::move(views_root)), icon_(std::move(icon)) {}

    void show(std::string_view id, const WindowSettings& settings,
              const View& view) override {
        if (!valid_id(id)) throw std::invalid_argument("Desktop window id is invalid");
        if (settings.width <= 0 || settings.height <= 0)
            throw std::invalid_argument("Desktop window dimensions must be positive");

        auto& window = windows_[std::string(id)];
        if (!window) window = std::make_unique<QQuickView>();
        QGuiApplication::setApplicationName(to_qstring(std::string_view(settings.title)));
        QGuiApplication::setApplicationDisplayName(to_qstring(std::string_view(settings.title)));
        // std::string puede convertirse tanto a string_view como a filesystem::path.
        window->setTitle(to_qstring(std::string_view(settings.title)));
        window->setIcon(icon_);
        window->resize(settings.width, settings.height);
        window->setResizeMode(QQuickView::SizeRootObjectToView);
        // view_path ya valida la raíz y devuelve una URL local de Qt.
        window->setSource(view_path(view));
        if (window->status() == QQuickView::Error) {
            QStringList errors;
            for (const auto& error : window->errors()) errors.push_back(error.toString());
            throw std::runtime_error("Could not load desktop view '" + view.name + "': " +
                                     errors.join('\n').toStdString());
        }
        window->show();
    }

    void close(std::string_view id) noexcept override {
        const auto found = windows_.find(id);
        if (found != windows_.end()) found->second->close();
    }

    [[nodiscard]] bool is_visible(std::string_view id) const noexcept override {
        const auto found = windows_.find(id);
        return found != windows_.end() && found->second->isVisible();
    }

    [[nodiscard]] bool has_visible_window() const noexcept override {
        for (const auto& [id, window] : windows_) {
            (void)id;
            if (window->isVisible()) return true;
        }
        return false;
    }

private:
    static bool valid_id(std::string_view id) {
        if (id.empty()) return false;
        for (const unsigned char ch : id) {
            if (!((ch >= 'a' && ch <= 'z') || (ch >= 'A' && ch <= 'Z') ||
                  (ch >= '0' && ch <= '9') || ch == '_' || ch == '-'))
                return false;
        }
        return true;
    }

    [[nodiscard]] QUrl view_path(const View& definition) const {
        // Revalidar aquí evita que una View construida directamente escape views_root_.
        std::string relative = view(definition.name).name;
        for (auto& ch : relative) if (ch == '.') ch = '/';
        auto path = views_root_ / (relative + ".qml");
        return QUrl::fromLocalFile(to_qstring(path.lexically_normal()));
    }

    std::filesystem::path views_root_;
    QIcon icon_;
    std::map<std::string, std::unique_ptr<QQuickView>, std::less<>> windows_;
};

class QtDesktopPlatform final : public DesktopPlatform {
public:
    int run(int argc, char** argv, const std::filesystem::path& views_root,
            const StartCallback& start) override {
        QGuiApplication application(argc, argv);
        const auto icon = default_app_icon();
        application.setWindowIcon(icon);
        QtWindowManager windows(views_root, icon);
        start(windows);
        if (!windows.has_visible_window())
            throw std::logic_error("Desktop startup route did not open a window");
        return application.exec();
    }
};
}

std::unique_ptr<DesktopPlatform> make_qt_platform() {
    return std::make_unique<QtDesktopPlatform>();
}

} // namespace kotavel::desktop
