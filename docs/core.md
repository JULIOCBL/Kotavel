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
valor predeterminado es `application`. El Laboratorio utiliza `laboratory`.

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
