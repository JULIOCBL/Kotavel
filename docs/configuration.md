# Configuración

- [Introducción](#introduccion)
- [Configuración del entorno](#configuracion-del-entorno)
    - [Tipos de variable](#tipos-de-variable)
    - [Recuperar valores del entorno](#recuperar-valores-del-entorno)
    - [Determinar el entorno actual](#determinar-el-entorno-actual)
    - [Seleccionar el archivo de entorno](#seleccionar-el-archivo-de-entorno)
    - [Plantillas de entorno](#plantillas-de-entorno)
- [Acceder a los valores de configuración](#acceder-a-los-valores-de-configuracion)
    - [Valores predeterminados](#valores-predeterminados)
    - [Consultas tipadas](#consultas-tipadas)
    - [Distinguir ausencia de null](#distinguir-ausencia-de-null)
    - [Grupos y listas](#grupos-y-listas)
    - [Modificar valores en tiempo de ejecución](#modificar-valores-en-tiempo-de-ejecucion)
- [Definir tus archivos de configuración](#definir-tus-archivos-de-configuracion)
    - [El registro explícito](#el-registro-explicito)
    - [Anidar grupos](#anidar-grupos)
    - [Convertir el mapa en opciones del logger](#convertir-el-mapa-en-opciones-del-logger)
    - [Límites](#limites)
- [Modo de depuración](#modo-de-depuracion)
- [Concurrencia y vida útil](#concurrencia-y-vida-util)
- [Detalles de implementación](#detalles-de-implementacion)

<a name="introduccion"></a>
## Introducción

La configuración de una aplicación Kotavel se reparte en dos capas, con la misma
separación que usa Laravel:

1. **Entorno:** los archivos `.env` de la raíz del proyecto guardan los valores
   que cambian entre equipos y servidores. No se versionan.
2. **Configuración:** los archivos `config/*.cpp` son mapas **compilados** que leen
   el entorno y publican opciones tipadas con claves separadas por puntos.

Al arrancar, el núcleo recorre estas etapas en orden:

```text
archivo .env y entorno del proceso
              ↓
       validación del entorno
              ↓
    funciones load() registradas
              ↓
      instancia kotavel::Config
              ↓
       configuración y arranque
```

El orden importa: las funciones `load()` pueden usar `env()` para calcular sus
valores predeterminados, pero `config()` solo se activa cuando la carga ha
terminado. Consulta [Arranque de consola](core.md#arranque-de-consola) para el
runtime completo.

> [!NOTE]
> No existe descubrimiento automático de archivos ni caché de configuración: los
> `config/*.cpp` se compilan en el binario. Cambiar un `.env` solo requiere
> reiniciar; cambiar un mapa de C++ requiere recompilar.

<a name="configuracion-del-entorno"></a>
## Configuración del entorno

Suele ser útil tener valores distintos según el equipo donde corre la aplicación.
Por ejemplo, querrás escribir los registros en consola durante el desarrollo y en
archivo en producción.

Kotavel resuelve esto con archivos `.env` en la raíz del proyecto. Las variables
se leen con la función `env`, y los archivos `config/*.cpp` son quienes las
consumen:

```cpp
// config/app.cpp
#include <kotavel/config/config_map.hpp>
#include <kotavel/env/env.hpp>

namespace app::config::app {
kotavel::ConfigMap load() {
    using kotavel::env;
    return {
        {"name", env("APP_NAME", "Kotavel")},
        {"debug", env("APP_DEBUG", false)},
    };
}
}
```

> [!NOTE]
> Cualquier variable de tu `.env` puede ser reemplazada por una variable de entorno
> externa, como las del sistema operativo o las del servidor. **El proceso siempre
> tiene prioridad sobre el archivo.**

<a name="tipos-de-variable"></a>
### Tipos de variable

Los valores de un `.env` son texto, así que existen valores reservados para
obtener otros tipos desde `env()`. La comparación no distingue mayúsculas:

| Valor en `.env` | Valor de `env()` |
| --- | --- |
| `true`, `(true)` | `true` (bool) |
| `false`, `(false)` | `false` (bool) |
| `empty`, `(empty)` | `""` (string vacío) |
| `null`, `(null)` | `null` (ausente) |

Para un valor con espacios, enciérralo entre comillas dobles. Las comillas
externas se eliminan al leer y conservan el contenido como texto:

```ini
APP_NAME="Mi aplicación"
LOG_CHANNEL="null"
```

> [!WARNING]
> `null` sin comillas se interpreta como valor nulo, no como el texto `null`. Si
> necesitas el texto para un canal o una opción, escríbelo entre comillas:
> `LOG_CHANNEL="null"`.

<a name="recuperar-valores-del-entorno"></a>
### Recuperar valores del entorno

Usa la función `env` para leer una variable. El segundo argumento es el valor
predeterminado y se devuelve cuando la variable no existe:

```cpp
#include <kotavel/env/env.hpp>
using kotavel::env;

auto name  = env("APP_NAME", "Kotavel");     // std::string
auto mode  = env("APP_ENV", "production");   // std::string
bool debug = env("APP_DEBUG", false);        // bool
```

El literal de texto tiene una sobrecarga propia para que nunca se interprete como
booleano. Por ahora solo se admiten predeterminados de texto o bool; **los números
se rechazan durante la compilación**:

```cpp
int port = env("PORT", 8080); // error de compilación
```

Sin segundo argumento, `env` devuelve un `EnvironmentValue` que conserva el tipo y
la ausencia, y puede imprimirse directamente:

```cpp
std::cout << env("APP_ENV");        // imprime el entorno o null
if (env("CACHE_DRIVER").is_null()) { /* la variable no existe */ }
```

<a name="determinar-el-entorno-actual"></a>
### Determinar el entorno actual

El entorno se resuelve desde `APP_ENV`. `AppEnvironment::load` lo valida junto al
nombre y al modo de depuración:

```cpp
#include <kotavel/env/env.hpp>
#include <iostream>

auto identity = kotavel::AppEnvironment::load();
std::cout << identity.name << " · Entorno: " << identity.environment << '\n';
```

| Propiedad | Variable | Predeterminado | Validación |
| --- | --- | --- | --- |
| `name` | `APP_NAME` | `Kotavel` | 1–64 bytes sin caracteres de control |
| `environment` | `APP_ENV` | `production` | `local`, `development`, `production`, `testing` o `staging` |
| `debug` | `APP_DEBUG` | `false` | Booleano |

`environment` y `debug` son independientes: el entorno no activa por sí solo la
presentación detallada de errores. Consulta [Modo de depuración](#modo-de-depuracion).

> [!NOTE]
> Aunque `AppEnvironment` acepta `testing` y `staging`, la selección de archivo
> del arranque solo reconoce `local`, `development` y `production`. Definir
> `APP_ENV=testing` provoca un error de arranque porque no coincide con la
> selección. Usa uno de los tres entornos soportados.

<a name="seleccionar-el-archivo-de-entorno"></a>
### Seleccionar el archivo de entorno

El runtime elige el archivo según la opción `--env`:

| `--env` | Archivo cargado |
| --- | --- |
| `production` (predeterminado) | `.env` |
| `local` | `.env.local` |
| `development` | `.env.development` |

```sh
python3 kotavel.py run --env=local
python3 kotavel.py run --env=development
python3 kotavel.py run --env=production
```

**El archivo seleccionado debe existir**: no hay respaldo silencioso. Si `APP_ENV`
está definido en el proceso o en el archivo, debe coincidir con la selección.

El archivo admite `KEY=VALUE`, líneas en blanco, comentarios con `#` y finales de
línea CRLF. No admite interpolación, secuencias de escape, valores multilínea ni
el prefijo `export`. Las claves duplicadas y la sintaxis inválida se rechazan.

> [!NOTE]
> También puedes ejecutar el binario directamente, por ejemplo desde el IDE:
>
> ```sh
> ./build/Debug/kotavel-lab --env=local --project-root=/ruta/al/proyecto
> ```

<a name="plantillas-de-entorno"></a>
### Plantillas de entorno

Versiona plantillas `.example` con valores de marcador para que el resto del
equipo sepa qué variables hacen falta. Los archivos reales **no** se versionan:

```sh
cp .env.example .env
cp .env.local.example .env.local
cp .env.development.example .env.development
```

En PowerShell usa `Copy-Item` en lugar de `cp`.

> [!WARNING]
> Nunca confirmes un `.env` con credenciales reales en el control de versiones.
> Kotavel **no cifra ni redacta** secretos: la protección depende de que los
> archivos reales permanezcan fuera del repositorio.

<a name="acceder-a-los-valores-de-configuracion"></a>
## Acceder a los valores de configuración

Puedes leer cualquier opción con el helper global `config`, usando la sintaxis de
puntos con el nombre del grupo y de la opción:

```cpp
#include <kotavel/config/config_helpers.hpp>
using kotavel::config;

std::cout << config("app.name");                    // vista imprimible, sin copia
const auto& name = config<std::string>("app.name"); // const std::string&
```

El helper sin tipo devuelve una vista que **no copia** el valor. El helper con
tipo devuelve `const T&`. Ambos buscan la clave completa en una tabla hash, con
coste esperado O(1) respecto al número de claves.

> [!IMPORTANT]
> Las claves son nombres de opciones, nunca valores. No coloques secretos en una
> clave: los mensajes de error la incluyen.

<a name="valores-predeterminados"></a>
### Valores predeterminados

Pasa un segundo argumento para obtener un valor cuando la clave no existe:

```cpp
std::string timezone = config("app.timezone", "UTC");
bool debug           = config("app.debug", false);
int retries          = config("queue.retries", 3);
double timeout       = config("queue.timeout", 2.5);
```

Estas sobrecargas devuelven el valor por copia, así que no exponen una referencia
a un predeterminado temporal. Admiten string, int, double y bool.

> [!WARNING]
> El predeterminado **solo** se aplica cuando la clave no existe. Si la clave
> existe con otro tipo, se lanza `std::invalid_argument` en lugar de convertir.

<a name="consultas-tipadas"></a>
### Consultas tipadas

Cuando la opción es obligatoria y no quieres copias, usa la forma tipada:

```cpp
const auto& name = config<std::string>("app.name");
int port         = config<int>("database.port");
```

`config<T>(key)` es estricto y mantiene el mismo contrato que `Config::get<T>`:

| Situación | Resultado |
| --- | --- |
| La clave existe con el tipo pedido | Devuelve `const T&` |
| La clave no existe | Lanza `std::out_of_range` |
| La clave existe con otro tipo | Lanza `std::invalid_argument` |

**No hay conversiones implícitas** entre `int`, `double`, `bool` y `std::string`.
Una cadena `"8080"` no se convierte a `int` al pedirla como número.

<a name="distinguir-ausencia-de-null"></a>
### Distinguir ausencia de null

`null` es un valor almacenable, así que una clave puede existir y valer null:

```cpp
if (settings.contains("app.optional_service")) {
    // La clave existe, aunque su valor pueda ser null.
}
if (const auto* value = settings.find("app.optional_service")) {
    // value != nullptr: la clave existe.
}
```

- `contains` → `true` si la clave existe, incluso con valor null.
- `find` → `nullptr` si la clave no existe.
- `config("clave")` sin tipo → su impresión produce `null` tanto si la clave falta
  como si vale null. Usa `contains` sobre la instancia para distinguirlos.

Para conservar un null al consultar con tipo, pide `std::monostate`:

```cpp
const auto& empty = config<std::monostate>("app.optional_service");
```

<a name="grupos-y-listas"></a>
### Grupos y listas

Puedes recuperar un grupo completo como copia propia:

```cpp
auto app = config<kotavel::ConfigMap>("app");
const auto& name = app.at("name").get<std::string>();
```

Al reconstruir un grupo, Kotavel recorre las claves planas que comienzan por el
prefijo y crea un mapa anidado. Es una copia independiente de la instancia, y por
eso **conviene reservarla para conjuntos de valores que se necesitan juntos**: las
consultas de hojas sueltas son más baratas.

Las listas se almacenan como un valor completo y se recorren por índice en C++.
Sus posiciones **no** forman claves con puntos:

```cpp
const auto& locales = config<kotavel::ConfigList>("app.locales");
for (const auto& locale : locales) {
    std::cout << locale.get<std::string>() << '\n';
}
// config("app.locales.0") NO existe.
```

<a name="modificar-valores-en-tiempo-de-ejecucion"></a>
### Modificar valores en tiempo de ejecución

Puedes cambiar valores en memoria durante el arranque:

```cpp
config().set("app.name", std::string("Mi tienda"));
config().set("app.debug", false);
config().set("app.optional_service", nullptr); // null explícito
config().set("app.locales", kotavel::ConfigList{"es", "en"});
```

Para varios cambios, usa un lote atómico. Si cualquiera falla, la instancia
original queda intacta:

```cpp
config().set_many({
    {"app.name", std::string("Mi tienda")},
    {"app.debug", false},
});
```

> [!WARNING]
> `set`, `set_many` y el reemplazo de un grupo **invalidan todas las referencias y
> vistas anteriores**. No conserves un `const auto&` de `config<T>(...)` ni un
> `ConfigValue` después de modificar la configuración.

> [!NOTE]
> Los valores modificados viven solo en memoria. Kotavel no escribe archivos de
> configuración, no modifica `.env` y no registra ningún contenido.

<a name="definir-tus-archivos-de-configuracion"></a>
## Definir tus archivos de configuración

Cada archivo de configuración declara una función `load()` que devuelve un
`ConfigMap`. Por convención el namespace y el grupo coinciden con el nombre del
archivo:

```cpp
// config/custom.cpp
#include <kotavel/config/config_map.hpp>
#include <kotavel/env/env.hpp>

namespace app::config::custom {
kotavel::ConfigMap load() {
    using kotavel::env;
    return {
        {"enabled", env("CUSTOM_ENABLED", false)},
        {"retries", 3},
    };
}
}
```

<a name="el-registro-explicito"></a>
### El registro explícito

Declara cada grupo y su función en el registro central:

```cpp
// config/app_config_files.cpp
#include "app_config_files.hpp"
#include "app.hpp"
#include "custom.hpp"
#include <kotavel/config/config_file.hpp>

namespace app::config {
kotavel::Config load_config_files() {
    const kotavel::ConfigFile files[] = {
        {"app", app::load},
        {"custom", custom::load},
    };
    return kotavel::load_config_files(files);
}
}
```

Hay **dos pasos independientes** y ambos son obligatorios:

| Paso | Dónde | Para qué |
| --- | --- | --- |
| Registrar el grupo | `config/app_config_files.cpp` | Indica **qué** cargar y con qué prefijo |
| Listar el fuente | `CMakeLists.txt` | Indica **qué** compilar |

No hay descubrimiento automático. Si añades `config/custom.cpp` y olvidas el
registro, el grupo no existe; si olvidas CMake, no compila.

<a name="anidar-grupos"></a>
### Anidar grupos

Los mapas anidados se aplanan a claves con puntos al cargar:

```cpp
return {
    {"enabled", true},
    {"limits", {{"count", 10}, {"timeout", 2.5}}},
};
```

```cpp
int count = config<int>("custom.limits.count"); // 10
```

La conversión ocurre **una sola vez**, durante el arranque. Consultar
`custom.limits.count` después es una búsqueda directa en la tabla.

<a name="convertir-el-mapa-en-opciones-del-logger"></a>
### Convertir el mapa en opciones del logger

El runtime estándar interpreta el grupo `logging.*` para construir el logger.
Declara el mapa en `config/logging.cpp` y el núcleo valida sus tipos y canales:

```cpp
// config/logging.cpp
namespace app::config::logging {
kotavel::ConfigMap load() {
    using kotavel::env;
    return {
        {"channel", env("LOG_CHANNEL", "single")},
        {"level", env("LOG_LEVEL", "info")},
        {"module", "app"},
        {"directory", "storage/logs"},
        {"max_bytes", 512 * 1024 * 1024},
        {"backups", 3},
    };
}
}
```

| Clave | Predeterminado | Función |
| --- | --- | --- |
| `logging.channel` | `single` | `single`: archivo; `stderr`: consola; `"null"`: descartar |
| `logging.level` | `info` | Nivel mínimo, entre `trace` y `emergency` |
| `logging.module` | `app` | Módulo del registro, de 1 a 64 bytes |
| `logging.directory` | `storage/logs` | Relativo a la raíz del consumidor, o ruta absoluta |
| `logging.max_bytes` | `536870912` | Tamaño máximo de archivo, mínimo 16384 |
| `logging.backups` | `3` | Entre cero y 100 respaldos |

Consulta [Logger configurado desde Config](core.md#logger-configurado-desde-config)
para el detalle de cada canal.

Las opciones del grupo `metrics` y sus límites están en la
[documentación de métricas](metrics.md).

<a name="limites"></a>
### Límites

Kotavel valida los mapas al cargarlos y rechaza los que exceden estos límites:

| Elemento | Límite |
| --- | --- |
| Identificador de grupo | Minúsculas, empieza por letra, solo `[a-z0-9_]` |
| Nodos por mapa | 10 000 |
| Profundidad de anidamiento | 32 niveles |
| Longitud de una clave final | 1024 bytes |
| Claves repetidas | Se rechazan las colisiones |

> [!NOTE]
> `ConfigMap` usa `std::map`. Si repites una clave en el mismo
> `initializer_list`, **se conserva la primera entrada**.

<a name="modo-de-depuracion"></a>
## Modo de depuración

La variable `APP_DEBUG` decide cuánta información sobre un error se muestra:

```ini
APP_DEBUG=true
```

En producción debe ser siempre `false`. Con `true`, la presentación de errores
añade código, causa, ubicación y traza; con `false` solo muestra un mensaje público
seguro y una referencia de operación.

La opción `--development` de la línea de comandos activa lo mismo sin tocar el
archivo:

```sh
python3 kotavel.py run --development
```

> [!WARNING]
> En producción, `APP_DEBUG=true` expone detalles técnicos al usuario final. Es
> independiente de `APP_ENV`: definir `APP_ENV=production` no desactiva la
> presentación detallada si `APP_DEBUG` sigue en `true`.

Para inspeccionar una opción durante el desarrollo, imprímela con el helper. Los
grupos y las listas muestran un marcador en lugar de volcar su contenido, porque
podrían incluir secretos:

```cpp
std::cout << config("app.name") << '\n';    // valor escalar
std::cout << config("app") << '\n';         // [configuration group]
std::cout << config("app.locales") << '\n'; // [configuration list]
```

Consulta [Errores y diagnóstico](core.md#reportar-y-presentar-son-pasos-distintos)
para la separación entre reportar y presentar.

<a name="concurrencia-y-vida-util"></a>
## Concurrencia y vida útil

`Config` **no contiene mutex**. Una instancia que permanece estable admite lecturas
concurrentes, pero cualquier escritura necesita sincronización externa contra todos
los lectores y contra el uso de referencias prestadas.

El patrón recomendado es construir y ajustar la configuración durante el arranque y
después compartirla como instancia constante.

```cpp
config().set("app.timezone", std::string("UTC")); // durante el arranque
const auto& timezone = config<std::string>("app.timezone");
int port = config<int>("database.port");          // lectura, ya sin cambios
```

Las referencias devueltas por `get<T>` y `config<T>` siguen siendo válidas al
insertar **otras** claves, incluso si la tabla hash se reorganiza. Dejan de serlo al
actualizar **esa misma** clave, reemplazar un grupo, ejecutar `set_many`, asignar,
mover o destruir la instancia.

Las copias obtenidas con `group` y `get_many` son propias y no dependen de la vida
útil de la instancia.

El helper `config()` depende de un `ConfigScope` activo, que es local al hilo. En
ejecución asíncrona, pasa `Config` explícitamente a la tarea en lugar de confiar en
el helper.

<a name="detalles-de-implementacion"></a>
## Detalles de implementación

Esta página describe **cómo se usa** la configuración. Para el modelo de datos, el
coste de cada operación y las garantías completas de concurrencia, consulta
[Referencia interna de Config](internals/config.md).

## Siguientes pasos

- [Uso del núcleo](core.md) — ciclo de vida, errores y registro.
- [Arranque de consola](core.md#arranque-de-consola) — `run_console`, `--env` y `--project-root`.
- [Blueprint](blueprint/README.md) — arquitectura objetivo y criterios de aceptación.
