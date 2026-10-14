# Uso del núcleo

Enlaza `Kotavel::Core` e incluye `<kotavel/foundation/application.hpp>`.
El núcleo ejecuta una operación cada vez, desde el mismo hilo; no incluye un
servidor, ejecutor asíncrono ni contenedor de servicios.

```cpp
kotavel::LogOptions logs{project_root / "storage" / "logs"};
logs.console = false; // El diagnóstico interno se conserva en el archivo.
kotavel::ErrorHandlingOptions errors;
errors.environment = kotavel::Environment::production; // Valor predeterminado.
errors.capture_traces = true; // Desactiva si no necesitas capturar pilas.
errors.rules["database.unavailable"].public_message = "Intenta más tarde.";

kotavel::Application app(logs, errors);
app.boot();
auto result = app.execute("operation-123", [] {
    throw kotavel::RecoverableError("database.unavailable", "Conexión rechazada");
});
if (auto presentation = app.present_error()) {
    std::cerr << presentation->console_text();
}
// recovered_error permite otra operación; fatal_error exige terminar.
app.shutdown();
```

`project_root` es una ruta elegida por el consumidor. Captura también las
excepciones de `boot()` en la entrada del programa: el registro puede no estar
disponible durante un fallo de arranque. Consulta el resultado de cada operación.

## Ciclo de vida y recuperación

`created → ready → running → ready` es el recorrido normal. Una
`RecoverableError` devuelve `recovered_error`, registra el incidente y permite
continuar. El framework libera objetos locales al desenrollar la pila; la
operación debe revertir cambios parciales en datos compartidos o transacciones.
No existe rollback automático.

`DiagnosticError` permite errores no recuperables con código y contexto propios.
Otras excepciones también devuelven `fatal_error` y dejan el estado en `failed`;
no se aceptan más operaciones. El nivel del registro y las reglas de presentación
no cambian esa decisión. Si falla la conservación del diagnóstico en memoria,
se utiliza un aviso mínimo por `stderr` y la aplicación pasa a `failed`.

`shutdown()` libera el logger y es idempotente; el destructor también lo llama.
Se ejecuta después de que termine la operación. Una llamada desde una operación
activa no hace nada: todavía no hay interrupción, cancelación ni plazos de parada.
Una instancia detenida no puede arrancar de nuevo. Los usos inválidos, como
llamar a `execute()` antes de `boot()`, lanzan al llamador.

## Reportar y presentar son pasos distintos

`Application` reporta automáticamente sus fallos al logger. `present_error()`
genera un `RenderedError`, sin escribirlo ni terminar el proceso: el consumidor
decide dónde mostrarlo. Su `exit_code` es 1 porque representa una operación
fallida; una demostración que se recupera y termina otra operación puede salir
con éxito. Si no existe un incidente, devuelve `std::nullopt`.

- **Producción:** mensaje público genérico o configurado y referencia de operación.
  No se incluyen causa técnica, archivo, función ni traza.
- **Desarrollo:** añade código, causa, ubicación y traza cuando estén disponibles.

La selección es explícita mediante `Environment`; no se lee `.env` automáticamente.
El nivel del logger se configura por separado. `console = true` envía diagnóstico
interno a `stderr` también en producción: es un destino de logging, no una
presentación pública. Desactívalo cuando compartas la consola con usuarios, como
hace el Laboratorio. El archivo sigue conteniendo detalles internos.

`last_error()` permite inspeccionar el diagnóstico hasta iniciar la siguiente
operación. No conserves referencias a él: se borra incluso si la siguiente
operación termina correctamente. Usa identificadores de operación únicos y sin
secretos; son la referencia pública y admiten entre 1 y 512 bytes.

Las reglas se configuran antes del arranque, por código de error:

```cpp
errors.rules["validation.failed"] = {
    .report = false,
    .level = kotavel::LogLevel::warning,
    .public_message = "Revisa los datos ingresados."
};
```

`report = false` omite el registro, pero conserva el resultado y la presentación.
`level` cambia la gravedad del reporte. El mensaje público debe ser texto seguro
definido por el desarrollador, nunca una copia automática de `what()`. Las reglas
se aplican por código, no por introspección del tipo C++. Los códigos admiten de
1 a 512 bytes. Las opciones se copian en la aplicación y quedan fijas.

## Contexto y resultado del registro

Cada operación puede identificar su módulo y aportar contexto estructurado:

```cpp
const std::vector<kotavel::ContextField> fields{{"order_id", "123"}, {"tenant", "demo"}};
auto result = app.execute(kotavel::OperationContext{"operation-123", "orders", fields}, [] {
    throw kotavel::RecoverableError("order.failed", "No se pudo guardar el pedido");
});
if (app.last_error() && app.last_error()->report_status == kotavel::ReportStatus::failed) {
    // La operación puede ser recuperable aunque el registro haya fallado.
    std::cerr << "No se pudo guardar el diagnóstico.\n";
}
```

Se admiten hasta 16 campos con claves únicas de 1–64 bytes y valores de hasta
256 bytes. El módulo admite 1–64 bytes. Se validan antes de ejecutar. Los campos,
el identificador y el módulo deben permanecer vivos y sin modificarse durante
`execute()`. El incidente conserva una copia propia solo al fallar. No incluyas
secretos: el contexto aparece en el archivo y en la presentación de desarrollo,
y queda fuera de la presentación de producción.

La variante `execute("id", action)` toma el módulo de `LogOptions::module`, cuyo
valor predeterminado es `application`. El Laboratorio utiliza `app`.

`last_error()->report_status` diferencia `written`, `filtered`, `throttled`,
`ignored` por una regla y `failed`. Es independiente de `OperationResult`.
`Logger::write(LogRecord)` devuelve ese estado para registros directos;
`Logger::stats()` expone contadores acumulados de escritos, filtrados, limitados
y fallidos. Los contadores son seguros entre hilos, pero una lectura conjunta
no es una instantánea atómica de todos ellos. Un fallo parcial de consola puede
producir `failed` aunque el archivo sí se haya escrito.

Las causas de `std::throw_with_nested` se conservan hasta ocho niveles y 512 bytes
por mensaje. Se informa si la cadena se truncó. Una excepción anidada sin causa
no provoca `terminate`. Las causas se registran y se muestran en desarrollo;
producción conserva únicamente el mensaje público y la referencia.

## Ubicación y trazas

`RecoverableError` y `DiagnosticError` guardan automáticamente archivo, línea,
función y hasta 24 marcos de pila al construirse. Si construyes una excepción en
un lugar y la lanzas después, se conserva el lugar de construcción. Los wrappers
propios deben recibir y reenviar `SourceLocation::current()` como argumento por
defecto para conservar el lugar del llamador.

`SourceLocation` utiliza la biblioteca estándar, con una adaptación para Apple
Clang 15 que evita ubicaciones incorrectas en los argumentos predeterminados.
La prueba de diagnóstico comprueba la línea exacta del consumidor.

Las excepciones externas no proporcionan necesariamente ubicación ni pila. En
ese caso se marca `trace_origin = handler`: la traza pertenece al manejador,
después de desenrollar la pila, y el archivo/línea original queda desconocido.

`ErrorHandlingOptions::capture_traces = false` evita capturar pilas durante esa
operación, incluidas excepciones anidadas. Conserva archivo y línea, y muestra
«Traza desactivada». La política está acotada al hilo actual y se restaura al
salir; no se propaga automáticamente a hilos creados por el consumidor. Fuera de
Application puede utilizarse `TraceCaptureScope` con el mismo comportamiento.
Una excepción construida antes de entrar a la operación conserva su captura previa.

La captura usa `backtrace` cuando CMake verifica que está disponible en macOS o
Linux, y `CaptureStackBackTrace` en Windows. Si no hay soporte se informa
«Traza no disponible». En macOS/Linux se muestran símbolos que permita resolver
el binario; en Windows se muestran direcciones. Conserva binarios y símbolos de
cada versión para investigar. No se garantiza una línea de código por cada marco
ni nombres de funciones completos en binarios optimizados.

Referencias de las APIs: [backtraces de libc](https://sourceware.org/glibc/manual/2.42/html_node/Backtraces.html)
y [CaptureStackBackTrace](https://learn.microsoft.com/en-us/windows/win32/debug/capturestackbacktrace).

## Registros y niveles

`LogOptions` configura `directory`, `max_bytes` (512 MiB por defecto), `backups`
(tres por defecto), `console` (activada por defecto) y `min_level` (`info`), además de `module` y `throttle`. El
tamaño mínimo es 16 KiB y el máximo de respaldos es 100; cero respaldos descarta
el archivo al rotar.

Con el archivo activo y los tres respaldos predeterminados, el límite total es
de 2 GiB. Los registros se escriben directamente al archivo; el logger no carga
su contenido completo en memoria.

Los niveles de menor a mayor gravedad son `trace`, `debug`, `info`, `notice`,
`warning`, `error`, `critical`, `alert` y `emergency`. Se incluyen los ocho niveles
usados por Laravel y se conserva `trace` como extensión de Kotavel.
`min_level` acepta únicamente ese nivel y superiores en ambos destinos.

```cpp
kotavel::Logger logger(logs);
logger.report(kotavel::LogLevel::notice, "startup", "boot", "Aplicación preparada");
logger.report("failure", "operation-123", "Error de operación"); // error por defecto
```

El filtro precede al bloqueo, formateo y escritura; no evita evaluar los argumentos
ni la captura realizada al construir una excepción. `report()` devuelve `true`
si escribió, filtró o limitó el mensaje, y `false` ante un fallo del reporte.
Para distinguir esos casos utiliza `write()` y su `ReportStatus`. No indica
si la operación fue exitosa. Un nivel inválido en opciones se rechaza; en un
reporte produce `false` y un aviso mínimo por `stderr`.

`Application` usa `error` para fallos recuperables y `critical` para inesperados,
salvo reglas explícitas. Un umbral superior puede ocultarlos del registro sin
cambiar el resultado de la operación.

`kotavel.log` contiene JSON por línea: fecha UTC, nivel, módulo, código,
identificador y mensaje; en los errores de Application también archivo, línea,
función, origen de la traza y pila. Los campos y las trazas se acotan para
respetar el límite de tamaño; los controles se escapan. Los mensajes deben ser
UTF-8 y no contener secretos: no hay redacción automática.

Al alcanzar el límite se rota a `kotavel.log.1`, `.2`, etc. La carpeta se bloquea durante la vida del logger mediante `.kotavel.lock`.
Una segunda instancia o proceso que use esa misma carpeta falla al iniciar.
Usa carpetas diferentes por proceso; varias hebras pueden compartir un único
Logger, pero Application sigue siendo de un solo hilo. El bloqueo se libera al
cerrar, aunque el archivo `.kotavel.lock` permanece. No borres la carpeta ni el
lock mientras se utilizan. El contrato está pensado para archivos locales:
no se garantiza exclusión en sistemas de archivos remotos. El archivo se vacía al sistema tras cada reporte,
sin garantizar persistencia ante pérdida de energía. Si falla, se emite un aviso
mínimo en `stderr` sin volver a invocar al logger; el detalle puede perderse.

## Control de reportes repetidos

```cpp
logs.throttle = {100, std::chrono::minutes{1}, 256};
// O, sobre un Logger existente:
logger.configure_report_throttling({100, std::chrono::minutes{1}, 256});
```

El límite cuenta intentos aceptados por cada combinación de módulo, código y
nivel en una ventana de reloj monotónico. El identificador de operación no forma
parte de la clave. El primer valor es el máximo por ventana; cero desactiva el
límite (predeterminado). El tercero limita el número de claves, entre 1 y 4096.

Al agotar una ventana, los reportes devuelven `throttled`, antes de formatear o
escribir. Al llenar la tabla, se eliminan claves vencidas; si todas siguen
activas se limitan también las claves nuevas. Esto mantiene acotada la memoria.
Los contadores muestran las supresiones. No se generan resúmenes automáticos.
Todos los niveles participan, incluidos los críticos, si activas el límite.

Reconfigurar el límite reinicia sus ventanas, pero conserva los contadores.
Se limita por instancia, no entre procesos, y no equivale a deduplicar una misma
instancia de excepción. Un fallo de escritura también consume el intento para
no repetir E/S fallida sin límite. La antigua función global reservada se
sustituye por este método: ahora la configuración tiene un logger explícito.

## Funciones reservadas para el futuro

`<kotavel/diagnostics/pending.hpp>` declara estas funciones, disponibles para
compilar pero **sin funcionalidad**:

| Función | Pendiente |
| --- | --- |
| `configure_remote_logging(endpoint)` | Destinos remotos; no abre conexiones ni guarda el endpoint |
| `enable_fatal_system_capture()` | Diagnóstico de fallos fatales; no instala handlers ni cambia señales |

Todas devuelven `FeatureStatus::not_implemented` y están marcadas `[[nodiscard]]`.
No deben tratarse como configuración exitosa. Son contratos experimentales que
podrán ajustarse al implementar estas capacidades.

Una caída del sistema, corrupción de memoria o terminación forzada no se recupera
con este manejador. La futura captura fatal será para diagnóstico, no una promesa
de continuar ejecutándose. El despliegue seguirá necesitando supervisión externa.

## Validación pendiente

El éxito de una operación no genera E/S de registro. Hay [microbenchmarks exploratorios](../benchmarks/README.md), pero aún faltan
presupuestos de rendimiento y pruebas prolongadas de producción. La matriz de CI para Windows, Linux y macOS está preparada; sus resultados
remotos siguen pendientes hasta ejecutarse en GitHub. El núcleo
no se declara terminado según los criterios del blueprint hasta completar esa
evidencia. Tampoco se ha implementado presentación HTTP: se definirá con ese módulo.

Consulta [los comandos y casos de prueba](../tests/README.md).

## Variables de entorno

`kotavel/env/env.hpp` proporciona `EnvironmentSource` (fuente reemplazable),
`ProcessEnvironment` (adaptador de `std::getenv`) y `Env` (conversión tipada).
La fuente debe vivir más que `Env`. Lee durante el arranque, antes de iniciar
hilos, y no modifiques simultáneamente el entorno del proceso. Los valores se
copian; nunca se conserva el puntero de la biblioteca estándar.

`get` admite un valor predeterminado para una variable ausente; `require`
rechaza ausencia o `null`. `string` y `boolean` rechazan tipos incorrectos.
Se reconocen `true`, `(true)`, `false`, `(false)`, `empty`, `(empty)`, `null` y
`(null)` sin distinguir mayúsculas. Las comillas externas conservan el contenido
como texto. No hay conversión de números ni interpolación. Los valores tienen
un límite de 4096 bytes; los errores no muestran el valor de la variable.

`AppEnvironment::load` resuelve `APP_NAME` (Kotavel), `APP_ENV` (production) y
`APP_DEBUG` (false). El nombre admite 1–64 bytes sin caracteres de control.
Los entornos admitidos son local, development, production, testing y staging.
El entorno y el modo debug son independientes. El consumidor usa debug para
seleccionar la presentación detallada de errores; en producción usa false.

`ProcessEnvironment` lee variables del proceso. `FileEnvironment` carga un archivo
y consulta primero el proceso. No modifica variables ni implementa caché, escritura
o cifrado de archivos.
El diseño toma como referencia [Env de Laravel](https://github.com/laravel/framework/blob/13.x/src/Illuminate/Support/Env.php)
sin prometer equivalencia completa con PHP Dotenv.

### Helper `env()`

```cpp
#include <kotavel/env/env.hpp>
using kotavel::env;

auto name = env("APP_NAME", "Kotavel");       // std::string
auto mode = env("APP_ENV", "production");    // std::string
bool debug = env("APP_DEBUG", false);        // bool
```

El helper comparte el adaptador del proceso y delega en `Env`. El literal de
texto tiene una sobrecarga explícita para que nunca se interprete como booleano.
Sin segundo argumento devuelve `EnvironmentValue`, que conserva texto, bool o
ausencia y permite impresión directa. Por ahora solo admite valores predeterminados de texto o bool;
los números se rechazan durante la compilación. `AppEnvironment::load()` usa la
misma fuente sin exigir construir manualmente un adaptador. Para fuentes propias
o pruebas sigue disponible `AppEnvironment::load(reader)` con un `Env` explícito.

### Selección de archivo al arrancar

`FileEnvironment(root, environment)` carga `.env` para production,
`.env.local` para local o `.env.development` para development. Los archivos son
obligatorios; nunca se cargan las plantillas `.example`. El proceso tiene prioridad.
Si APP_ENV está definido debe coincidir con la selección. El archivo admite
KEY=VALUE, comentarios, CRLF y comillas simples o dobles. No admite interpolación,
escapes, valores multilínea ni el prefijo export. Claves duplicadas y sintaxis
inválida se rechazan. Límites: archivo 64 KiB, valor 4096 bytes, clave 256 bytes.
La clase carga una vez al arrancar. `run_console` activa su lector con `EnvScope`: los
helpers consultan archivo y proceso en el hilo actual durante los callbacks. Fuera
de ese ámbito consultan solo el proceso. El ámbito restaura la fuente anterior
al salir, incluso ante excepciones, y no se propaga a otros hilos.

## Arranque de consola

`kotavel/console/runtime.hpp` ofrece `run_console(argc, argv, configure, execute,
application_help)`, actualmente enlazado con `Kotavel::Core`. Resuelve --env,
--project-root y --development, valida la raíz, carga FileEnvironment y crea
Application. La ayuda no requiere archivos ni arranque. Los argumentos propios
se entregan en ConsoleContext al consumidor; este debe validarlos. Los callbacks
son síncronos y no deben conservar las vistas del contexto. ConfigureConsole
entrega opciones de logging y errores; la presentación se fija según APP_DEBUG
o --development. ExecuteConsole recibe Application lista y devuelve un código
de salida. El cierre ocurre al terminar y por RAII si el callback lanza.

`execute_console` ejecuta una operación y presenta sus errores y fallos del
logger. El consumidor decide qué resultados recuperables permiten continuar.
Los fallos externos retornan 1; los detalles solo se imprimen en modo debug.
No implementa registro automático de comandos, executor ni tareas asíncronas.

Si el archivo no declara APP_ENV, FileEnvironment publica la selección validada.
Para imprimirlo sin valor predeterminado: `std::cout << env("APP_ENV")`.
Los booleanos imprimen true/false y la ausencia/null imprime null.
`is_null()` comprueba ausencia; `as_string()` y `as_bool()` extraen el tipo
y rechazan conversiones incorrectas. La referencia de as_string() requiere
conservar vivo el objeto EnvironmentValue.

## Organización interna del núcleo

La API pública permanece en `include/kotavel/`; las implementaciones y cabeceras
privadas viven en `src/`. Se conserva `kotavel/env/env.hpp` como entrada pública
para que el consumidor siga usando los mismos includes y helpers.

| Carpeta | Archivo | Responsabilidad |
| --- | --- | --- |
| `src/env/` | `env.cpp` | Conversión y consulta tipada |
| | `process_environment.cpp` | Adaptador de variables del proceso |
| | `file_environment.cpp` | Carga del archivo y precedencia |
| | `env_helpers.cpp` | Helpers y ámbito del lector por hilo |
| | `environment_value.cpp` | Valor imprimible, tipos y ausencia |
| | `app_environment.cpp` | Validación de nombre, entorno y debug |
| `src/diagnostics/` | `error.cpp` | Excepciones, contexto y causas |
| | `stack_trace.cpp` | Captura y formato de trazas nativas |
| | `error_renderer.cpp` | Presentación pública y de desarrollo |
| | `logger.cpp` | Escritura, rotación, filtrado y límites de reportes |
| | `log_formatter.cpp` | Serialización JSON y límites de contenido |
| `src/foundation/` | `application.cpp` | Estados y ciclo de vida |
| | `application_errors.cpp` | Retención, reporte y presentación de errores |
| `src/console/` | `runtime.cpp` | Arranque y ejecución de consola |
| | `settings.cpp` | Validación y construcción del logger desde Config |
| | `arguments.cpp` | Interpretación de opciones comunes |
| | `operation.cpp` | Ejecución y presentación de una operación |

Las cabeceras privadas comparten únicamente contratos internos y utilidades
pequeñas; no se instalan ni deben incluirse desde el consumidor. Cada `.cpp`
se registra explícitamente en CMake. Separar archivos responde a responsabilidades,
no a un número fijo de líneas ni a crear una pareja de archivos por obligación.

El runtime devuelve 1 si el consumidor retorna 0 después de un fallo fatal,
incluso si llamó shutdown. Los resultados recuperables pueden continuar
y finalizar correctamente según la política del consumidor.

## Configuración en memoria con `Config`

Para el uso del entorno y de `Config`, consulta [Configuración](configuration.md).
Para el funcionamiento interno, costes y garantías de la implementación, consulta
la [referencia interna de Config](internals/config.md).

```cpp
#include <kotavel/config/config.hpp>

kotavel::Config config;
config.set("database.connections.mysql.username", "usuario");
const auto& username = config.get<std::string>("database.connections.mysql.username");
config.set("database.port", 3306);
config.set("database.timeout", 2.5);
config.set("database.enabled", true);
if (config.contains("database.port")) {
    const int port = config.get<int>("database.port");
}
```

La instancia usa un `unordered_map` plano con `variant<monostate, string, int, double, bool, ConfigMap, ConfigList>`.
Los puntos son parte literal de la clave: no se dividen ni crean nodos. `int`
tiene el rango del tipo C++ del compilador; no hay conversiones automáticas entre
entero, double y bool. Literales, string y string_view se almacenan como texto
propio. El objeto copia o mueve los valores al escribir; nunca retiene las vistas
del llamador. Claves vacías o con controles se rechazan.

`get<T>` devuelve `const T&`. Las consultas correctas y `contains` buscan con
string_view sin crear cadenas temporales ni copiar valores. La búsqueda es O(1)
esperada respecto al número de claves, O(n) en el peor caso; calcular el hash
recorre los bytes de la clave completa. Los errores de clave ausente lanzan
`out_of_range`; un tipo incorrecto lanza `invalid_argument`. Incluyen la clave,
pero nunca el valor. Las claves deben ser nombres de opciones, sin secretos.
Config no escribe archivos, no modifica `.env` y no registra ningún contenido.

Las referencias sobreviven a insertar otras claves, incluso si cambia la tabla
hash. Para el contrato público, no se deben conservar después de actualizar la
misma clave, asignar/mover/destruir Config. No se permite obtener referencias de
un Config temporal. `auto value = config.get<string>(...)` copia; usa `const auto&`
si necesitas una lectura prestada. No conviertas una referencia en string_view
para conservarla después de cambiar el valor.

Pasa Config explícitamente a los componentes. Varios hilos pueden leer una
instancia que permanece estable; cualquier escritura requiere sincronización
externa contra todas las lecturas y usos de referencias. Una referencia devuelta
no queda protegida automáticamente al liberar un bloqueo. El patrón recomendado
es construir/ajustar antes del uso y compartir después una instancia const.
`set` actualiza una clave; set_many ofrece un lote atómico en memoria.

## Mapas anidados, registro explícito y helper `config()`

`kotavel/config/config_map.hpp` define ConfigMap y ConfigNode en el núcleo.
Un mapa puede contener texto, int, double, bool y otros mapas. `Config::merge`
recorre ese árbol una vez para crear claves planas con un prefijo de grupo.
Ejemplo: `{"routing", {{"api", {{"port", 8080}}}}}` dentro del grupo app
se consulta como `app.routing.api.port`. No se recorre el árbol al consultar.
Se limitan la profundidad a 32, los nodos de cada merge a 10000 y las claves
resultantes a 1024 bytes. Se rechazan colisiones entre claves planas y anidadas.
Como ConfigMap usa std::map, no repitas exactamente una clave en el mismo
initializer_list: su construcción conserva la primera entrada.

El consumidor crea config/app.cpp y futuros grupos con
una función `kotavel::ConfigMap load()` en el namespace
`app::config::<nombre_del_archivo>`. El registro explícito determina el prefijo;
por convención coincide con el nombre del archivo.
Los nombres deben ser identificadores en minúsculas, sin subcarpetas ni puntos.
ConfigMap admite null explícito con nullptr y listas con ConfigList{...}, además de
grupos asociativos anidados. Consulta las listas completas y accede a sus elementos
por índice en C++; los índices no se convierten a claves de puntos.

El consumidor declara sus archivos en config/app_config_files.cpp usando ConfigFile:

```cpp
const kotavel::ConfigFile files[] = {{"app", app::load}};
return kotavel::load_config_files(files);
```

Cada entrada asocia el grupo y la función load(). El núcleo utiliza el prefijo
declarado y carga los mapas en el orden del registro. No descubre archivos
ni genera llamadas: cada .cpp debe estar listado también en el target de CMake
para compilarlo. El registro no lee archivos en ejecución; llama código compilado.
bootstrap pasa la función central al runtime, que la ejecuta después de cargar
y validar el entorno y antes de configurar/arrancar Application. Las funciones
load() consultan env() activo; config() se activa cuando la carga está completa.

```cpp
#include <kotavel/config/config_helpers.hpp>
using kotavel::config;
std::cout << config("app.name");
const auto& name = config<std::string>("app.name");
config().set("database.connections.mysql.username", "usuario");
```

El helper sin tipo devuelve una vista imprimible, sin copia del valor; el helper
con tipo devuelve const T&. `config()` devuelve la instancia activa, que también
puede pasarse explícitamente a los componentes. Sin un ámbito activo lanza
logic_error. Las vistas y referencias no deben sobrevivir al Config ni usarse
tras actualizar su clave. ConfigScope restaura la instancia anterior al salir,
incluso por una excepción. Es local al hilo y no se propaga a tareas o hilos:
para ellos pasa explícitamente una instancia con duración y sincronización claras.
ConfigScope no introduce bloqueos ni vuelve seguras las escrituras concurrentes.

El Laboratorio registra config/app.cpp para APP_NAME, APP_ENV y APP_DEBUG,
y config/logging.cpp para las opciones del logger. No contiene configuración
de base de datos ni migraciones.

El código de entorno vive en include/kotavel/env/ y src/env/. El registro de
configuración no recibe ni carga Env: load() consulta el helper env() ya activo.
La secuencia es entorno → validación del entorno → mapas → Config → aplicación.

### Claves opcionales y valores predeterminados

`config("app.parametro_inexistente")` devuelve una vista nula, comprobable con
`is_null()`, y su impresión produce `null`. `config("clave", false)` devuelve
false si la clave no existe. También admite defaults string, int y double.
Los defaults solo se aplican ante ausencia; un valor existente de tipo incorrecto
sigue produciendo un error. Estas sobrecargas devuelven valores propios y pueden
copiar strings, evitando referencias a defaults temporales. Cada consulta busca
la clave una sola vez. Para lecturas obligatorias sin copias conserva
`config<T>(key)` o `Config::get<T>(key)`:
ambas siguen rechazando claves inexistentes. La ausencia de un ConfigScope activo
sigue siendo un error de ciclo de vida, no una clave ausente.

## Grupos, listas, null y operaciones múltiples

```cpp
using kotavel::config;
// Copia propia: consultar todas las opciones del grupo actual.
auto app = config<kotavel::ConfigMap>("app");
const auto& name = app.at("name").get<std::string>();

// Ejemplos de valores aceptados en los mapas compilados.
kotavel::ConfigMap options{
    {"locales", kotavel::ConfigList{"es", "en"}},
    {"optional_service", nullptr},
};
```

`config("app")` devuelve una vista que posee una copia del grupo reconstruido;
puedes extraerlo con `.get<ConfigMap>()` conservando viva la vista. El helper
`config<ConfigMap>("app")` devuelve directamente la copia. Config::group devuelve
optional<ConfigMap>. Reconstruir un grupo recorre el almacén y copia sus valores;
las consultas de hojas mantienen la búsqueda directa sin copiar ni recorrer
segmentos. Al reconstruir un grupo, los segmentos compartidos se crean una sola
vez. Si una clave exacta almacena un escalar, prevalece sobre el prefijo:
no debe usarse como grupo. Estructuras incompatibles al reconstruir se rechazan.

`config<ConfigList>("app.locales")` presta la lista almacenada. Sus elementos son
ConfigNode y admiten los mismos tipos, incluso mapas, listas y null. Usa
`list[0].get<string>()`. No se implementa el recorrido de índices por claves con
puntos: `app.locales.0` no es una clave generada. `nullptr` representa null explícito;
contains distingue su existencia de una clave ausente. Los defaults solo se usan
cuando falta una clave. Una consulta tipada/default tipado a null rechaza el tipo;
para conservar null consulta sin tipo o pide std::monostate. La impresión de
null produce null; los grupos/listas muestran un marcador sin volcar contenido.

```cpp
auto values = config().get_many({"app.name", "app.debug", "missing"});
// Resultado propio con claves completas; missing contiene null.
config().set_many({
    {"app.name", std::string("Mi aplicación")},
    {"app.debug", false},
});
// Forma de helper equivalente:
config({{"app.name", std::string("Mi aplicación")}, {"app.debug", false}});
```

get_many devuelve un ConfigMap propio, incluye grupos solicitados y null ante
claves ausentes. set_many prepara una copia de Config, valida/aplica cada cambio
y publica con swap; si algo falla no modifica el original. Un lote exitoso
invalida todas las referencias/vistas prestadas anteriores. El orden del lote
se respeta y una clave repetida conserva el último valor. Los null en un lote
se expresan con std::monostate{}. Una lista vacía es ConfigList{}; un grupo vacío
es ConfigMap{}. No se registran ni persisten los valores.

set de un ConfigMap sustituye ese grupo y sus hojas de forma preparada; también
invalida todas las referencias/vistas anteriores porque publica una nueva tabla.
Las copias de grupos ya obtenidas siguen siendo independientes. Las restricciones
de concurrencia permanecen: sincronización externa para escrituras, incluidos
lotes; no usar referencias prestadas mientras se modifica la configuración.
No se añaden comandos de inspección ni caché en esta etapa.

## Logger configurado desde Config

El overload `run_console(argc, argv, execute, help, load_config)` aplica
`console_settings(context)` después de cargar los mapas. La variante con callback
ConfigureConsole sigue disponible para personalizar el arranque. El adaptador
usa la instancia de Config del contexto y rechaza tipos o valores inválidos.

| Clave | Default | Función |
| --- | --- | --- |
| logging.channel | single | single: archivo; stderr: consola; null: descartar |
| logging.level | info | Nivel mínimo entre trace y emergency |
| logging.module | app | Módulo del registro, de 1 a 64 bytes |
| logging.directory | storage/logs | Directorio relativo a root, o ruta absoluta |
| logging.max_bytes | 536870912 | Tamaño máximo de archivo, mínimo 16384 |
| logging.backups | 3 | Entre cero y 100 respaldos |

El consumidor lee LOG_CHANNEL y LOG_LEVEL en config/logging.cpp. Para el canal
null utiliza `LOG_CHANNEL="null"` en .env, porque null sin comillas se interpreta
como valor nulo. Los canales stderr y null no crean el directorio, el archivo ni
el bloqueo. null devuelve ReportStatus::filtered sin formatear ni hacer E/S;
Application conserva los diagnósticos y su política de recuperación y cierre.
Los nuevos campos LogOptions::file y enabled permiten seleccionar destinos
cuando se construye un Logger explícitamente. Los registros exitosos de
operaciones no se generan automáticamente. No hay stack de canales ni destinos
remotos implementados.
