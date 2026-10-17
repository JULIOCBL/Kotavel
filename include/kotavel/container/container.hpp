#pragma once

#include <functional>
#include <memory>
#include <stdexcept>
#include <typeindex>
#include <type_traits>
#include <utility>
#include <vector>

namespace kotavel {

class Container;

// Los proveedores separan el registro de dependencias de su inicialización.
class ServiceProvider {
public:
    virtual ~ServiceProvider() = default;
    virtual void register_services(Container& services) = 0;
    virtual void boot(Container& services) { (void)services; }
    virtual void shutdown(Container& services) noexcept { (void)services; }
};

class Container final {
public:
    Container() = default;
    ~Container();
    Container(const Container&) = delete;
    Container& operator=(const Container&) = delete;
    Container(Container&&) = delete;
    Container& operator=(Container&&) = delete;

    // Un singleton se construye en la primera consulta y se conserva hasta shutdown().
    template<class T, class Factory>
    void singleton(Factory&& factory) {
        register_factory<T>(std::forward<Factory>(factory), Lifetime::singleton);
    }

    // Un bind crea un objeto nuevo en cada make(); quien lo solicita es su dueño.
    template<class T, class Factory>
    void bind(Factory&& factory) {
        register_factory<T>(std::forward<Factory>(factory), Lifetime::transient);
    }

    // Registra una instancia cuya propiedad compartida conserva el contenedor.
    template<class T>
    void instance(std::shared_ptr<T> value) {
        if (!value) throw std::invalid_argument("Cannot register a null service instance");
        register_instance(std::type_index(typeid(T)), std::move(value));
    }

    // Devuelve servicios compartidos por referencia; la referencia vence en shutdown().
    template<class T>
    T& get() {
        return *std::static_pointer_cast<T>(resolve(std::type_index(typeid(T)), true));
    }

    template<class T>
    const T& get() const {
        return const_cast<Container*>(this)->get<T>();
    }

    // Construye un servicio transitorio. El shared_ptr deja explícita su propiedad.
    template<class T>
    std::shared_ptr<T> make() {
        return std::static_pointer_cast<T>(resolve(std::type_index(typeid(T)), false));
    }

    template<class T>
    [[nodiscard]] bool contains() const {
        return contains_type(std::type_index(typeid(T)));
    }

    // Congela el registro antes del boot; shutdown() libera singletons en orden inverso.
    void seal();
    void shutdown() noexcept;
    [[nodiscard]] bool sealed() const noexcept;
    [[nodiscard]] bool stopped() const noexcept;

private:
    enum class Lifetime { singleton, transient };
    using ErasedFactory = std::function<std::shared_ptr<void>(Container&)>;

    struct Registration {
        Lifetime lifetime;
        ErasedFactory factory;
        std::shared_ptr<void> value;
        bool resolving = false;
    };
    std::vector<std::pair<std::type_index, Registration>> registrations_;
    std::vector<std::type_index> construction_order_;
    std::vector<std::type_index> resolution_stack_;
    bool sealed_ = false;
    bool stopped_ = false;

    template<class T, class Factory>
    void register_factory(Factory&& factory, Lifetime lifetime) {
        using FactoryType = std::decay_t<Factory>;
        static_assert(std::is_invocable_r_v<std::unique_ptr<T>, FactoryType&, Container&>,
                      "A service factory must return std::unique_ptr<T> and accept Container&");
        auto owned_factory = std::make_shared<FactoryType>(std::forward<Factory>(factory));
        ErasedFactory erased = [owned_factory](Container& services) -> std::shared_ptr<void> {
            auto instance = std::invoke(*owned_factory, services);
            if (!instance) throw std::runtime_error("Service factory returned a null instance");
            return std::shared_ptr<T>(std::move(instance));
        };
        add_factory(std::type_index(typeid(T)), std::move(erased), lifetime);
    }

    void add_factory(std::type_index type, ErasedFactory factory, Lifetime lifetime);
    void register_instance(std::type_index type, std::shared_ptr<void> value);
    [[nodiscard]] std::shared_ptr<void> resolve(std::type_index type, bool singleton_only);
    [[nodiscard]] bool contains_type(std::type_index type) const;
};

// El callback crea proveedores propiedad del runtime; se cierran en orden inverso.
using ServiceProviderList = std::vector<std::unique_ptr<ServiceProvider>>;
using CreateServiceProviders = std::function<ServiceProviderList()>;

} // namespace kotavel
