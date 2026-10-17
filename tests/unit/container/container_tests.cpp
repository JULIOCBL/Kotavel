#include <kotavel/container/container.hpp>

#include <iostream>
#include <memory>
#include <stdexcept>
#include <string>
#include <vector>

namespace {

void check(bool condition, const char* message) {
    if (!condition) throw std::runtime_error(message);
}

template<class Exception, class Function>
std::string expect_exception(Function&& function, const char* message) {
    try {
        function();
    } catch (const Exception& error) {
        return error.what();
    }
    throw std::runtime_error(message);
}

struct Service {
    explicit Service(int value = 0) : value(value) {}
    int value;
};

struct Dependency {
    explicit Dependency(std::vector<std::string>& events) : events(events) {}
    ~Dependency() { events.push_back("dependency"); }
    std::vector<std::string>& events;
};

struct Dependent {
    Dependent(Dependency& dependency, std::vector<std::string>& events)
        : dependency(dependency), events(events) {}
    ~Dependent() { events.push_back("dependent"); }
    Dependency& dependency;
    std::vector<std::string>& events;
};

struct CycleA {};
struct CycleB {};

// Verifica laziness e identidad: registrar no debe construir el singleton.
void test_singleton_is_lazy_and_shared() {
    kotavel::Container services;
    int constructions = 0;
    services.singleton<Service>([&](kotavel::Container&) {
        ++constructions;
        return std::make_unique<Service>(42);
    });

    check(services.contains<Service>(), "registered singleton should be discoverable");
    check(constructions == 0, "singleton factory must be lazy");
    auto& first = services.get<Service>();
    auto& second = services.get<Service>();
    check(&first == &second, "singleton lookups must return the same object");
    check(first.value == 42 && constructions == 1, "singleton should be built exactly once");
}

// El cierre inverso mantiene vivos los servicios requeridos por sus dependientes.
void test_dependencies_and_reverse_shutdown() {
    std::vector<std::string> events;
    kotavel::Container services;
    services.singleton<Dependency>([&](kotavel::Container&) {
        return std::make_unique<Dependency>(events);
    });
    services.singleton<Dependent>([&](kotavel::Container& container) {
        return std::make_unique<Dependent>(container.get<Dependency>(), events);
    });

    auto& dependent = services.get<Dependent>();
    check(&dependent.dependency == &services.get<Dependency>(), "factory should resolve its dependency");
    services.shutdown();
    check(events == std::vector<std::string>{"dependent", "dependency"},
          "dependent services must be destroyed before their dependencies");
}

// Las instancias transitorias pertenecen al llamador y no al contenedor.
void test_transient_make_and_lifetime_mismatch() {
    std::vector<std::string> events;
    kotavel::Container services;
    int constructions = 0;
    services.bind<Service>([&](kotavel::Container&) {
        return std::make_unique<Service>(++constructions);
    });
    services.singleton<Dependency>([&](kotavel::Container&) {
        return std::make_unique<Dependency>(events);
    });

    auto first = services.make<Service>();
    auto second = services.make<Service>();
    check(first != second && first->value == 1 && second->value == 2,
          "transient make should build a distinct object each time");
    check(services.contains<Dependency>(), "contains should report a registered singleton");

    expect_exception<std::logic_error>([&] { (void)services.get<Service>(); },
        "get must reject a transient service");
    expect_exception<std::logic_error>([&] { (void)services.make<Dependency>(); },
        "make must reject a singleton service");
}

// Las instancias registradas se conservan hasta el cierre del contenedor.
void test_registered_instance_ownership() {
    kotavel::Container services;
    expect_exception<std::invalid_argument>([&] {
        services.instance<Service>({});
    }, "null instances must be rejected before registration");
    auto instance = std::make_shared<Service>(7);
    std::weak_ptr<Service> observed = instance;
    services.instance<Service>(instance);
    instance.reset();

    check(services.get<Service>().value == 7, "registered instance should be resolvable");
    check(!observed.expired(), "container should retain the registered instance");
    services.shutdown();
    check(observed.expired(), "shutdown should release the registered instance");
}

// Las consultas inválidas y el registro tardío deben fallar de forma explícita.
void test_registration_and_resolution_errors() {
    std::vector<std::string> events;
    kotavel::Container services;
    services.singleton<Service>([](kotavel::Container&) { return std::make_unique<Service>(); });
    expect_exception<std::logic_error>([&] {
        services.bind<Service>([](kotavel::Container&) { return std::make_unique<Service>(); });
    }, "duplicate service registrations must fail");

    const auto missing = expect_exception<std::out_of_range>([&] {
        (void)services.get<Dependency>();
    }, "resolving an unregistered service must fail");
    check(missing.find("Service is not registered") != std::string::npos,
          "missing-service error should identify the failure");
    check(services.sealed(), "first resolution attempt should freeze the container even on failure");
    expect_exception<std::logic_error>([&] {
        services.singleton<Dependency>([&](kotavel::Container&) {
            return std::make_unique<Dependency>(events);
        });
    }, "first resolution should freeze registration");

    kotavel::Container sealed;
    sealed.seal();
    expect_exception<std::logic_error>([&] {
        sealed.bind<Service>([](kotavel::Container&) { return std::make_unique<Service>(); });
    }, "sealed container must reject registrations");
}

// Un fallo de construcción no debe dejar el singleton marcado como activo.
void test_null_factory_can_be_retried() {
    kotavel::Container services;
    int calls = 0;
    services.singleton<Service>([&](kotavel::Container&) -> std::unique_ptr<Service> {
        if (++calls == 1) return {};
        return std::make_unique<Service>(9);
    });

    const auto failure = expect_exception<std::runtime_error>([&] {
        (void)services.get<Service>();
    }, "null factory result must fail clearly");
    check(failure.find("null instance") != std::string::npos, "null factory error should be descriptive");
    check(services.get<Service>().value == 9 && calls == 2,
          "a failed singleton construction should leave the registration retryable");
}

// La pila de resolución debe limpiarse aunque falle una fábrica transitoria.
void test_transient_factory_failure_does_not_poison_future_resolutions() {
    kotavel::Container services;
    int calls = 0;
    services.bind<Service>([&](kotavel::Container&) -> std::unique_ptr<Service> {
        if (++calls == 1) throw std::runtime_error("temporary construction failure");
        return std::make_unique<Service>(11);
    });

    expect_exception<std::runtime_error>([&] { (void)services.make<Service>(); },
        "transient factory exceptions should propagate to the caller");
    const auto recovered = services.make<Service>();
    check(recovered->value == 11 && calls == 2,
          "a failed transient construction must not leave stale cycle-detection state");
}

// Detecta ciclos antes de entrar en una recursión infinita.
void test_circular_singleton_and_transient_dependencies() {
    kotavel::Container singletons;
    singletons.singleton<CycleA>([](kotavel::Container& services) {
        (void)services.get<CycleB>();
        return std::make_unique<CycleA>();
    });
    singletons.singleton<CycleB>([](kotavel::Container& services) {
        (void)services.get<CycleA>();
        return std::make_unique<CycleB>();
    });
    const auto singleton_cycle = expect_exception<std::logic_error>([&] {
        (void)singletons.get<CycleA>();
    }, "circular singleton dependency must fail");
    check(singleton_cycle.find("Circular service dependency") != std::string::npos,
          "singleton cycle error should identify the dependency cycle");

    kotavel::Container transients;
    transients.bind<CycleA>([](kotavel::Container& services) {
        (void)services.make<CycleB>();
        return std::make_unique<CycleA>();
    });
    transients.bind<CycleB>([](kotavel::Container& services) {
        (void)services.make<CycleA>();
        return std::make_unique<CycleB>();
    });
    const auto transient_cycle = expect_exception<std::logic_error>([&] {
        (void)transients.make<CycleA>();
    }, "circular transient dependency must fail");
    check(transient_cycle.find("Circular service dependency") != std::string::npos,
          "transient cycle error should identify the dependency cycle");
}

// El cierre es terminal e idempotente; no destruye dos veces los servicios.
void test_shutdown_is_idempotent_and_terminal() {
    kotavel::Container services;
    int destructions = 0;
    struct Counted {
        explicit Counted(int& destructions) : destructions(destructions) {}
        ~Counted() { ++destructions; }
        int& destructions;
    };
    services.singleton<Counted>([&](kotavel::Container&) {
        return std::make_unique<Counted>(destructions);
    });
    (void)services.get<Counted>();
    services.shutdown();
    services.shutdown();
    check(services.stopped() && destructions == 1, "shutdown should be terminal, idempotent, and destroy once");
    expect_exception<std::logic_error>([&] { (void)services.get<Counted>(); },
        "stopped container must reject resolution");
    expect_exception<std::logic_error>([&] { services.seal(); },
        "stopped container must reject sealing");
    expect_exception<std::logic_error>([&] {
        services.singleton<Service>([](kotavel::Container&) { return std::make_unique<Service>(); });
    }, "stopped container must reject registration");
}

} // namespace

int main() {
    try {
        test_singleton_is_lazy_and_shared();
        test_dependencies_and_reverse_shutdown();
        test_transient_make_and_lifetime_mismatch();
        test_registered_instance_ownership();
        test_registration_and_resolution_errors();
        test_null_factory_can_be_retried();
        test_transient_factory_failure_does_not_poison_future_resolutions();
        test_circular_singleton_and_transient_dependencies();
        test_shutdown_is_idempotent_and_terminal();
        std::cout << "Kotavel container checks passed\n";
        return 0;
    } catch (const std::exception& error) {
        std::cerr << error.what() << '\n';
        return 1;
    }
}
