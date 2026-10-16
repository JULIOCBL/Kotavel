# Métricas de rendimiento

- [Introducción](#introduccion)
- [Ver las métricas desde el Laboratorio](#ver-las-metricas-desde-el-laboratorio)
- [Configurar el módulo](#configurar-el-modulo)
- [Medir un proceso](#medir-un-proceso)
    - [Tomar muestras](#tomar-muestras)
    - [Medir el arranque](#medir-el-arranque)
- [Enviar métricas a otra terminal](#enviar-metricas-a-otra-terminal)
- [Ciclo de vida y concurrencia](#ciclo-de-vida-y-concurrencia)
- [Qué mide el módulo](#que-mide-el-modulo)
- [Pruebas](#pruebas)
- [Límites](#limites)

<a name="introduccion"></a>
## Introducción

El módulo `kotavel/metrics/performance.hpp` mide el uso de CPU y memoria
residente del proceso actual, y ofrece un monitor que toma muestras periódicas.
También permite consultar las etapas del arranque de consola. El consumidor elige
dónde presentar o enviar esos datos; el núcleo no los guarda en los logs de errores.

Hay dos formas de usarlo:

1. **Integración con `run_console`:** el consumidor configura el grupo
   `performance`; el runtime administra el monitor y su cierre.
2. **Uso directo:** el consumidor crea un `ProcessMetricsCollector` o un
   `PerformanceMonitor` y define un callback propio.

La integración del Laboratorio también publica las muestras para que el comando
`python3 kotavel.py monitor` las muestre en otra terminal. Consulta
[Configuración](configuration.md) para entender cómo se cargan los mapas y el
entorno antes de construir estas opciones.

<a name="ver-las-metricas-desde-el-laboratorio"></a>
## Ver las métricas desde el Laboratorio

Desde la raíz del Laboratorio, abre dos terminales. En la primera inicia la
aplicación:

```sh
python3 kotavel.py run
```

En la segunda conecta el monitor:

```sh
python3 kotavel.py monitor
```

La primera terminal conserva la salida normal de la aplicación. La segunda muestra
los tiempos del arranque y las muestras de CPU y RAM:

```text
[Kotavel] Arranque: entorno 0.08 ms | configuración 0.04 ms | opciones 0.02 ms | aplicación 0.31 ms | total 0.53 ms
[Kotavel] CPU: 0.0 % | RAM residente: 18.4 MiB
```

El Laboratorio se mantiene activo mientras las métricas están habilitadas. Usa
`Ctrl+C` en la terminal principal para cerrar la aplicación ordenadamente; el
monitor detectará el cierre. `Ctrl+C` en la terminal del monitor solo desconecta
esa vista.

Para elegir una instancia cuando hay varias ejecuciones del mismo proyecto:

```sh
python3 kotavel.py monitor --pid 12345
```

El PID aparece en el nombre del descriptor ubicado en
`storage/framework/metrics/kotavel-<pid>.endpoint`.

<a name="configurar-el-modulo"></a>
## Configurar el módulo

El consumidor registra sus opciones en el mapa compilado `config/performance.cpp`.
Como en los demás archivos `config/*.cpp`, puede usar `env()` para obtener valores
del archivo de entorno o del proceso:

```cpp
// config/performance.cpp
#include "performance.hpp"
#include <kotavel/env/env.hpp>

namespace app::config::performance {
kotavel::ConfigMap load() {
    using kotavel::env;
    return {
        {"enabled", env("KOTAVEL_PERFORMANCE_ENABLED", false)},
        {"interval_ms", 1000},
        {"console", env("KOTAVEL_PERFORMANCE_CONSOLE", true)},
        {"startup", true},
        {"stream", env("KOTAVEL_PERFORMANCE_STREAM", false)},
    };
}
}
```

El registro explícito debe incluir ese archivo para que el runtime reciba el grupo:

```cpp
// config/app_config_files.cpp
const kotavel::ConfigFile files[] = {
    {"performance", performance::load},
};
```

La aplicación del Laboratorio activa el stream por defecto; otras aplicaciones
pueden elegir sus propios valores. Sus claves son:

| Clave | Tipo | Valor del Laboratorio | Función |
| --- | --- | --- | --- |
| `performance.enabled` | bool | `true` | Activa la medición |
| `performance.interval_ms` | int | `1000` | Intervalo entre muestras; de 100 a 60000 ms |
| `performance.console` | bool | `false` | Imprime muestras en la terminal principal |
| `performance.startup` | bool | `true` | Publica o imprime las duraciones del arranque |
| `performance.stream` | bool | `true` | Publica muestras para el comando `monitor` |
| `performance.hold` | bool | `true` | Opción propia del ejemplo del Laboratorio: lo mantiene activo |

El Laboratorio lee estas variables desde el `.env` seleccionado:

```ini
KOTAVEL_PERFORMANCE_ENABLED=true
KOTAVEL_PERFORMANCE_CONSOLE=false
KOTAVEL_PERFORMANCE_STREAM=true
KOTAVEL_PERFORMANCE_HOLD=true
```

`hold` pertenece al programa de ejemplo del Laboratorio; no es una opción del
runtime de Kotavel. Una aplicación real permanece activa según su servidor,
worker o ciclo principal. Para desactivar las métricas del Laboratorio, define
`KOTAVEL_PERFORMANCE_ENABLED=false`.

El runtime estándar usa estos valores cuando el grupo no declara una opción:

| Opción de `PerformanceOptions` | Predeterminado |
| --- | --- |
| `enabled` | `false` |
| `interval` | `1000 ms` |
| `console` | `true` |
| `startup` | `true` |
| `stream` | `false` |

Las claves declaradas conservan el tipo estricto de `Config`. Un tipo incorrecto,
un intervalo menor que 100 ms o uno mayor que 60000 ms produce un error de
configuración durante el arranque.

<a name="medir-un-proceso"></a>
## Medir un proceso

Incluye la cabecera pública:

```cpp
#include <kotavel/metrics/performance.hpp>
```

`ProcessMetricsCollector` consulta una muestra del mismo proceso que lo ejecuta.
Su estado guarda los valores anteriores necesarios para calcular CPU, por lo que
una instancia debe usarse desde un solo hilo.

<a name="tomar-muestras"></a>
### Tomar muestras

Puedes consultar muestras manualmente o delegar las consultas a
`PerformanceMonitor`:

```cpp
#include <kotavel/metrics/performance.hpp>
#include <chrono>
#include <iostream>
#include <thread>

kotavel::metrics::PerformanceMonitor monitor;
monitor.start(std::chrono::seconds(1), [](const kotavel::metrics::PerformanceSample& sample) {
    if (sample.process_cpu_percent)
        std::cout << "CPU: " << *sample.process_cpu_percent << " %\n";
    std::cout << "RAM residente: " << sample.resident_memory_bytes << " bytes\n";
});

// Mantener la aplicación activa durante la observación de ejemplo.
std::this_thread::sleep_for(std::chrono::seconds(5));
// Esperar a que termine cualquier callback antes de liberar recursos usados por él.
monitor.stop();
```

El callback se ejecuta en el hilo del monitor. La muestra es válida durante la
llamada: copia los valores que necesites conservar y no retengas una referencia.
Las excepciones del callback se contienen para que no escapen del hilo de trabajo.
Desde el hilo propietario, `stop()` despierta el monitor y espera a que termine;
al retornar, no queda un callback activo. Si se llama dentro del callback, solo
solicita la parada para evitar que el hilo intente esperarse a sí mismo; después,
el propietario todavía debe llamar a `stop()` para completar la unión del hilo.

Para consultar una muestra sin un hilo de fondo:

```cpp
kotavel::metrics::ProcessMetricsCollector collector;
if (const auto sample = collector.sample()) {
    // La primera muestra tiene RAM; CPU aparece después de una segunda lectura.
}
```

`sample()` puede devolver `std::nullopt` si el sistema no puede leer las métricas.
`process_cpu_percent` también es opcional en la primera lectura porque aún no hay
un intervalo con el cual calcularlo.

<a name="medir-el-arranque"></a>
### Medir el arranque

`run_console` mide las etapas disponibles y las entrega en un
`metrics::StartupMetrics` al consumidor del stream o a la salida de consola:

```cpp
struct StartupMetrics {
    std::chrono::nanoseconds environment;
    std::chrono::nanoseconds configuration;
    std::chrono::nanoseconds settings;
    std::chrono::nanoseconds application_boot;
    std::chrono::nanoseconds total;
};
```

El tipo completo es `kotavel::metrics::StartupMetrics` y ya está declarado en la
cabecera pública del módulo.

Los valores son duraciones, no marcas de tiempo. El total empieza al entrar al
runtime y termina después de `Application::boot()`. No incluye la carga del
ejecutable ni inicializadores estáticos.

<a name="enviar-metricas-a-otra-terminal"></a>
## Enviar métricas a otra terminal

Para implementar otro consumidor local, `PerformanceStream` publica el último
arranque y la última muestra:

```cpp
kotavel::metrics::PerformanceStream stream;
stream.start(project_root);
stream.publish_startup(startup_metrics);
stream.publish(sample);
// Al cerrar el proceso:
stream.stop();
```

El stream escucha en un puerto efímero ligado a `127.0.0.1`. El descriptor con el
puerto se guarda en `storage/framework/metrics/kotavel-<pid>.endpoint` y se elimina
al cerrar normalmente. Los eventos se transmiten como líneas JSON; si un cliente
se conecta tarde, recibe la última muestra disponible. El transporte no admite
conexiones desde otras máquinas y atiende un monitor a la vez; si otro cliente se
conecta, reemplaza la conexión anterior. El runtime inicia este canal solo cuando
la opción `performance.stream` está activa y lo cierra antes de apagar `Application`.

<a name="ciclo-de-vida-y-concurrencia"></a>
## Ciclo de vida y concurrencia

`run_console` administra el ciclo del monitor integrado: lo inicia después de que
`Application::boot()` termina y lo detiene antes de `Application::shutdown()`.
En el uso directo, el consumidor debe llamar a `stop()` antes de destruir los
objetos que usa el callback.

El callback corre en un hilo distinto del hilo principal. No accedas allí a
`env()` ni a `config()`: sus ámbitos de carga no se propagan entre hilos. Tampoco
debes bloquear el hilo de muestreo con trabajo lento; copia la muestra y entrega
el procesamiento a tu propio mecanismo si hace falta.

<a name="que-mide-el-modulo"></a>
## Qué mide el módulo

`resident_memory_bytes` es la memoria residente (RSS) del proceso completo:
incluye el runtime, la aplicación consumidora y sus dependencias. No equivale
solo a memoria dinámica o heap.

`process_cpu_percent` se calcula como tiempo de CPU del proceso dividido entre el
tiempo transcurrido. Se expresa como porcentaje de un núcleo; un proceso que usa
varios núcleos puede superar 100 %. El primer valor es `null`/ausente porque se
necesita una segunda muestra. No se mide el consumo de GPU ni se atribuye CPU o RAM
a módulos individuales.

<a name="pruebas"></a>
## Pruebas

Desde el Laboratorio, ejecuta las pruebas del framework y del consumidor:

```sh
python3 kotavel.py test --framework
```

La suite unitaria de métricas comprueba muestras, intervalos, callbacks,
inicio/parada y reinicio del monitor. La integración de `PerformanceStream`
conecta un cliente real al listener local. La integración del Laboratorio ejecuta
`kotavel.py monitor` como segundo proceso, confirma que las métricas no aparecen
en la terminal principal y verifica el cierre ordenado.

<a name="limites"></a>
## Límites

Los adaptadores de CPU y memoria usan APIs nativas de Windows, Linux y macOS. El
transporte local usa sockets TCP de loopback y Winsock en Windows. La integración
automática está preparada para ejecutarse en las tres plataformas; cada una debe
compilarse y probarse en su runner nativo.

La recolección añade trabajo al proceso. El coste depende del sistema y del
intervalo configurado; todavía no hay una medición publicada de ese coste. El
módulo es una herramienta de observación, no un profiler que localice qué función
consume recursos.

La API pública completa está en
[`include/kotavel/metrics/performance.hpp`](../include/kotavel/metrics/performance.hpp).
