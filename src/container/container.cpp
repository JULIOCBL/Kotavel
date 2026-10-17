#include <kotavel/container/container.hpp>

#include <algorithm>
#include <string>

namespace kotavel {
namespace {
std::string circular_chain(const std::vector<std::type_index>& stack, std::type_index type) {
    std::string chain;
    bool include = false;
    for (const auto& item : stack) {
        if (item == type) include = true;
        if (!include) continue;
        if (!chain.empty()) chain += " -> ";
        chain += item.name();
    }
    if (!chain.empty()) chain += " -> ";
    chain += type.name();
    return chain;
}
}

Container::~Container() {
    shutdown();
}

void Container::add_factory(std::type_index type, ErasedFactory factory, Lifetime lifetime) {
    if (stopped_) throw std::logic_error("Cannot register a service after container shutdown");
    if (sealed_) throw std::logic_error("Cannot register a service after container seal");
    if (contains_type(type)) throw std::logic_error("Duplicate service registration: " + std::string(type.name()));
    registrations_.push_back({type, Registration{lifetime, std::move(factory), {}, false}});
}

void Container::register_instance(std::type_index type, std::shared_ptr<void> value) {
    if (stopped_) throw std::logic_error("Cannot register a service after container shutdown");
    if (sealed_) throw std::logic_error("Cannot register a service after container seal");
    if (contains_type(type)) throw std::logic_error("Duplicate service registration: " + std::string(type.name()));
    // Las instancias se construyeron antes de registrarse; su orden también guía el cierre.
    construction_order_.push_back(type);
    try {
        registrations_.push_back({type, Registration{Lifetime::singleton, {}, std::move(value), false}});
    } catch (...) {
        construction_order_.pop_back();
        throw;
    }
}

std::shared_ptr<void> Container::resolve(std::type_index type, bool singleton_only) {
    if (stopped_) throw std::logic_error("Cannot resolve a service after container shutdown");

    // La primera resolución cierra el registro para mantener referencias estables.
    sealed_ = true;
    const auto found = std::find_if(registrations_.begin(), registrations_.end(),
        [type](const auto& entry) { return entry.first == type; });
    if (found == registrations_.end())
        throw std::out_of_range("Service is not registered: " + std::string(type.name()));

    auto& registration = found->second;
    if (singleton_only && registration.lifetime != Lifetime::singleton)
        throw std::logic_error("Transient service requires make<T>(): " + std::string(type.name()));
    if (!singleton_only && registration.lifetime != Lifetime::transient)
        throw std::logic_error("Shared service requires get<T>(): " + std::string(type.name()));

    if (std::find(resolution_stack_.begin(), resolution_stack_.end(), type) != resolution_stack_.end())
        throw std::logic_error("Circular service dependency: " + circular_chain(resolution_stack_, type));

    if (registration.lifetime == Lifetime::transient) {
        resolution_stack_.push_back(type);
        try {
            auto value = registration.factory(*this);
            if (!value) throw std::runtime_error("Service factory returned a null instance: " + std::string(type.name()));
            resolution_stack_.pop_back();
            return value;
        } catch (...) {
            if (!resolution_stack_.empty() && resolution_stack_.back() == type)
                resolution_stack_.pop_back();
            throw;
        }
    }

    if (registration.value) return registration.value;
    if (registration.resolving)
        throw std::logic_error("Circular service dependency: " + circular_chain(resolution_stack_, type));

    registration.resolving = true;
    try {
        resolution_stack_.push_back(type);
        auto value = registration.factory(*this);
        if (!value) throw std::runtime_error("Service factory returned a null instance: " + std::string(type.name()));
        // Guardar el orden antes de transferir la propiedad para conservar seguridad ante excepciones.
        construction_order_.push_back(type);
        registration.value = std::move(value);
        resolution_stack_.pop_back();
        registration.resolving = false;
        return registration.value;
    } catch (...) {
        if (!resolution_stack_.empty() && resolution_stack_.back() == type)
            resolution_stack_.pop_back();
        registration.resolving = false;
        throw;
    }
}

bool Container::contains_type(std::type_index type) const {
    return std::any_of(registrations_.begin(), registrations_.end(),
        [type](const auto& entry) { return entry.first == type; });
}

void Container::seal() {
    if (stopped_) throw std::logic_error("Cannot seal a stopped container");
    sealed_ = true;
}

void Container::shutdown() noexcept {
    if (stopped_) return;
    // Destruir primero los servicios dependientes, que se construyeron después.
    for (auto item = construction_order_.rbegin(); item != construction_order_.rend(); ++item) {
        const auto found = std::find_if(registrations_.begin(), registrations_.end(),
            [&item](const auto& entry) { return entry.first == *item; });
        if (found != registrations_.end()) found->second.value.reset();
    }
    construction_order_.clear();
    // Las instancias registradas directamente pueden no aparecer en construction_order_.
    for (auto& entry : registrations_) entry.second.value.reset();
    stopped_ = true;
}

bool Container::sealed() const noexcept { return sealed_; }
bool Container::stopped() const noexcept { return stopped_; }

} // namespace kotavel
