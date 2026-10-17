# Service Providers

Un `ServiceProvider` agrupa el registro de servicios de una aplicación y las
acciones que requieren el inicio y el cierre del proceso. El consumidor crea
explícitamente sus providers y se los entrega a `run_console`; Kotavel no busca
clases ni archivos automáticamente.

## Responsabilidades

Cada provider tiene tres etapas:

- `register_services(Container&)`: declara fábricas e instancias. No debería
  iniciar listeners, hilos ni conexiones externas.
- `boot(Container&)`: inicia recursos después de que todos los registros estén
  disponibles y el contenedor haya quedado sellado.
- `shutdown(Container&) noexcept`: detiene y libera los recursos que inició.

`register_services()` es obligatorio. `boot()` y `shutdown()` tienen una
implementación vacía por defecto, por lo que un provider que solo registra
dependencias no necesita sobrescribirlos. Evita resolver servicios durante
`register_services()`: la primera resolución sella el contenedor e impediría que
los providers siguientes agreguen registros.

## Implementación de un provider

```cpp
#include <kotavel/container/container.hpp>
#include <memory>

class BillingClock final {
public:
    void start();
    void stop() noexcept;
};

class BillingAppServiceProvider final : public kotavel::ServiceProvider {
public:
    void register_services(kotavel::Container& services) override {
        // La fábrica crea el singleton cuando otro componente lo resuelve.
        services.singleton<BillingClock>([](kotavel::Container&) {
            return std::make_unique<BillingClock>();
        });
    }

    void boot(kotavel::Container& services) override {
        services.get<BillingClock>().start();
    }

    void shutdown(kotavel::Container& services) noexcept override {
        if (services.contains<BillingClock>())
            services.get<BillingClock>().stop();
    }
};
```

Los servicios base `Config`, `AppEnvironment` y `Application` ya están
registrados. Un provider puede resolverlos en `boot()`; por ejemplo, consultar
`AppEnvironment` para elegir comportamiento según el entorno. No registres
servicios después de `register_services()`: el contenedor se sella antes de
`boot()`.

## Registro explícito desde la aplicación

El consumidor devuelve los providers en el orden deseado. El runtime conserva
su propiedad durante la ejecución:

```cpp
#include <kotavel/console/runtime.hpp>

kotavel::ServiceProviderList create_service_providers() {
    kotavel::ServiceProviderList providers;
    providers.push_back(std::make_unique<BillingAppServiceProvider>());
    return providers;
}

int run(int argc, char** argv) {
    return kotavel::run_console(
        argc, argv,
        execute_application,
        "",
        load_application_config,
        create_service_providers);
}
```

El callback `create_service_providers` es el último argumento de
`run_console()`, después del callback que carga la configuración. Si la
aplicación no necesita providers, puede omitirlo. Cada `.cpp` propio debe
añadirse al target ejecutable del consumidor en CMake.

## Orden del ciclo de vida

El runtime sigue esta secuencia:

1. Carga y valida el entorno y los mapas de configuración.
2. Crea el contenedor y registra `Config`, `AppEnvironment` y `Application`.
3. Crea la lista de providers y llama `register_services()` en orden.
4. Sella el contenedor para impedir cambios posteriores al registro.
5. Arranca `Application` y llama `boot()` a los providers en orden.
6. Ejecuta el callback de la aplicación.
7. Detiene métricas, llama `shutdown()` a los providers en orden inverso,
   cierra `Application` y libera los servicios.

Si un `boot()` falla, el runtime también intenta llamar `shutdown()` al provider
cuyo arranque falló y a los anteriores, en orden inverso. La limpieza es
idempotente. Si falla el registro antes de iniciar los providers, sus métodos
`shutdown()` no se ejecutan; por eso `register_services()` debe limitarse a
declarar dependencias y reservar la adquisición de recursos para `boot()`.

## Alcance y concurrencia

Los providers coordinan el ciclo de vida, pero no son un sistema de descubrimiento,
reflexión ni plugins dinámicos. La lista es estática y explícita en el código del
consumidor. Tampoco reemplazan la inyección directa: cuando una clase conoce sus
dependencias, es preferible recibirlas en el constructor.

El runtime llama los métodos del provider de forma síncrona en el hilo de
arranque/cierre. El contenedor no permite registrar o resolver servicios
concurrentemente. Si el callback de aplicación inicia trabajo paralelo, los
servicios que este utilice deben implementar su propia sincronización y seguir
vivos hasta que ese trabajo termine y se ejecute `shutdown()`.

Para el contrato del contenedor, fábricas y duraciones de servicios, consulta
[Contenedor de servicios](container.md). Para la secuencia global del runtime,
consulta [Uso del núcleo](core.md).
