# Contenedor de servicios

El contenedor crea y conserva servicios compartidos durante el ciclo de vida de
una aplicación. Los componentes declaran sus dependencias mediante fábricas
explícitas; Kotavel no utiliza reflexión ni construye clases automáticamente.

## Registrar servicios

`singleton<T>()` recibe una fábrica que retorna `std::unique_ptr<T>`. La fábrica
se ejecuta en la primera llamada a `get<T>()`, y el contenedor conserva el
servicio hasta el cierre:

```cpp
services.singleton<Clock>([](kotavel::Container&) {
    return std::make_unique<SystemClock>();
});

services.singleton<ReportService>([](kotavel::Container& services) {
    return std::make_unique<ReportService>(services.get<Clock>());
});

ReportService& reports = services.get<ReportService>();
```

Un registro `bind<T>()` produce una instancia nueva cada vez que se solicita con
`make<T>()`. El llamador recibe un `std::shared_ptr<T>` y decide cuánto tiempo
conservarla:

```cpp
services.bind<RequestContext>([](kotavel::Container&) {
    return std::make_unique<RequestContext>();
});

auto request = services.make<RequestContext>();
```

`instance<T>()` registra una instancia ya creada y transfiere una referencia
compartida al contenedor. `contains<T>()` comprueba si existe un registro.
Registrar dos veces el mismo tipo produce una excepción.

## Proveedores

Los providers agrupan registros y acciones de ciclo de vida. Para el orden
completo del arranque, manejo de fallos y registro desde una aplicación, consulta
[Service Providers](providers.md).

Un proveedor agrupa registros y acciones de inicio y cierre. El consumidor los
crea y se los entrega al runtime como una lista propiedad de Kotavel:

```cpp
class BillingAppServiceProvider final : public kotavel::ServiceProvider {
public:
    void register_services(kotavel::Container& services) override {
        services.singleton<Clock>([](kotavel::Container&) {
            return std::make_unique<SystemClock>();
        });
    }

    void boot(kotavel::Container& services) override {
        (void)services.get<Clock>();
    }

    void shutdown(kotavel::Container& services) noexcept override {
        (void)services;
    }
};

std::vector<std::unique_ptr<kotavel::ServiceProvider>> providers() {
    std::vector<std::unique_ptr<kotavel::ServiceProvider>> result;
    result.push_back(std::make_unique<BillingAppServiceProvider>());
    return result;
}
```

Entrega el callback como último argumento de `run_console`, después del callback
que carga los mapas de configuración:

```cpp
return kotavel::run_console(argc, argv, execute, {}, config::load_config_files,
                            providers);
```

El runtime registra primero los servicios de todos los proveedores. Luego
congela el contenedor, arranca `Application` y llama `boot()` a los proveedores
en orden. Si termina la ejecución o ocurre una excepción, llama `shutdown()` a
los proveedores iniciados en orden inverso. Finalmente libera los servicios
compartidos en orden inverso al que se construyeron.

Los servicios base `Config`, `AppEnvironment` y `Application` están disponibles
para los proveedores mediante `services.get<T>()`. El callback de aplicación
también recibe el contenedor mediante `ConsoleContext::container`.

## Reglas de ciclo de vida

- Registra todos los servicios dentro de `register_services()`. El primer
  `get()` o `make()` congela los registros; una modificación posterior falla.
- `get<T>()` solo obtiene servicios compartidos. `make<T>()` solo obtiene tipos
  registrados con `bind<T>()`.
- Las fábricas reciben `Container&` y deben devolver `std::unique_ptr<T>`. Usa
  `get<Dependencia>()` para declarar la construcción de una dependencia.
- Una dependencia circular produce un error que muestra el ciclo detectado.
- Las fábricas de singleton son diferidas; los `boot()` de proveedores son el
  lugar para iniciar explícitamente recursos que deban arrancar con la app.
- Una referencia devuelta por `get<T>()` es válida hasta `shutdown()` o hasta la
  destrucción del contenedor. No debe sobrevivir ese límite.
- `shutdown()` es idempotente. Después de llamarlo no se pueden registrar ni
  resolver servicios.
- La instancia del contenedor no es segura para registrar o resolver desde
  varios hilos simultáneamente. Configura y resuelve los servicios durante el
  arranque, antes de iniciar trabajo concurrente. La seguridad de cada servicio
  depende de su propia implementación.

El contenedor no es un localizador global: pasa la referencia a los componentes
que realmente requieren resolver servicios. En componentes de dominio, es
preferible inyectar directamente las dependencias en el constructor.
