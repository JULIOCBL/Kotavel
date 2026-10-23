# Sesiones

Kotavel ofrece una API de sesión con almacenamiento intercambiable. La primera
implementación usa archivos en el proyecto consumidor:

```text
<raíz del proyecto>/storage/framework/sessions/
```

Los procesos de actualización del proyecto deben conservar
`storage/framework/sessions/` para que una actualización no cierre las sesiones.

La API pública vive en `<kotavel/session/session.hpp>`; el include anterior
`<kotavel/http/session.hpp>` se conserva como compatibilidad. El middleware
HTTP está declarado en `<kotavel/http/session_middleware.hpp>`. La sesión se
habilita explícitamente en el grupo web del `HttpKernel`:

```cpp
kotavel::FileEnvironment environment_file(project_root, "local");
kotavel::Env environment(environment_file);
kotavel::EnvScope environment_scope(environment);
const kotavel::ConfigFile files[] = {
    {"session", kotavel::http::SessionConfig::load},
};
const auto configuration = kotavel::load_config_files(files);
auto session_config = kotavel::http::SessionConfig::from_config(configuration);

kernel.use_web(kotavel::http::start_file_session(session_config, project_root));
```

En el Laboratorio, `config/session.cpp` define las opciones y sus defaults; el
registro central `config/app_config_files.cpp` lo carga bajo el grupo `session`.
Puedes cambiar ahí los defaults de esa aplicación. El núcleo mantiene
`SessionConfig::from_config()` para convertir ese grupo en opciones del
middleware. `SESSION_*` puede sobrescribir los valores definidos en el archivo.

Dentro de un handler o middleware:

```cpp
kotavel::http::Session::put("user_id", 42);
const auto user_id = kotavel::http::Session::get<int>("user_id", 0);

// Forma equivalente ligada a la petición actual:
kotavel::http::session().put("theme", "dark");
```

`Request::session()` usa la misma sesión. La API solo está disponible mientras
la petición atraviesa el middleware de sesión; su uso fuera de ese ámbito
produce `std::logic_error`.

## Configuración

Los valores se leen de `Env` y pueden definirse en el entorno o en el `.env` del
proyecto. Los defaults son:

| Variable | Default | Uso |
| --- | --- | --- |
| `SESSION_DRIVER` | `file` | Driver inicial disponible |
| `SESSION_FILES` | `storage/framework/sessions` | Directorio relativo a la raíz del proyecto |
| `SESSION_LIFETIME` | `120` | Minutos de inactividad permitidos |
| `SESSION_COOKIE` | `kotavel_session` | Nombre de la cookie |
| `SESSION_PATH` | `/` | Ruta de la cookie en el navegador |
| `SESSION_DOMAIN` | sin valor | Dominio de la cookie |
| `SESSION_SECURE_COOKIE` | `false` | Añade el atributo `Secure` |
| `SESSION_HTTP_ONLY` | `true` | Añade el atributo `HttpOnly` |
| `SESSION_SAME_SITE` | `lax` | `lax`, `strict` o `none` |
| `SESSION_EXPIRE_ON_CLOSE` | `false` | Omite `Max-Age` para usar una cookie de sesión |
| `SESSION_ENCRYPT` | `false` | Si se activa, el arranque falla porque el cifrado aún no está implementado |

`SESSION_PATH` configura el alcance de la cookie, no el directorio de archivos.
`SESSION_FILES` cambia la carpeta física. Las API permanecen sin sesión a menos
que se les agregue el middleware de forma explícita.

El driver crea identificadores de 256 bits con el generador criptográfico del
sistema operativo. El navegador recibe el identificador; el archivo conserva
los datos y sus marcas de tiempo. Los archivos se validan al leerse, se escriben
con permisos privados y se reemplazan atómicamente. Un bloqueo por sesión cubre
la carga, la ejecución del handler y el guardado para evitar perder cambios en
peticiones concurrentes. Los bloqueos se distribuyen en un conjunto acotado de
archivos compartidos. El middleware revisa sesiones expiradas en la primera
solicitud con sesión, cada 100 solicitudes o al transcurrir un minuto, y también rechaza una sesión vencida cuando el navegador
vuelve a presentarla. La recolección no depende de que el cliente vuelva:
inspecciona los archivos almacenados, agrupa la eliminación por shard de bloqueo
y envía cada grupo al contrato de filesystem como un lote. La limpieza avanza
con un cursor: cada llamada inspecciona hasta 128 archivos y recorre los 256
prefijos. Si hay más de 128 registros, el cursor conserva su posición para la
siguiente llamada. El backend local elimina cada archivo
del lote mediante las operaciones del sistema de archivos. Cada archivo de
sesión tiene un límite de 4 MiB.

El contador pertenece a cada instancia de middleware y se comparte entre sus
copias. Mientras un barrido esté en ejecución, otras peticiones de esa instancia
no inician barridos adicionales. La protección se libera incluso si falla la
recolección. El recorrido sigue siendo síncrono para la petición que lo dispara.

La sesión de aplicación inicia vacía. Las marcas de tiempo y la versión del
formato son metadatos del archivo, no valores visibles mediante `Session::all()`.
El token CSRF, los mensajes flash y la URL anterior se agregarán cuando existan
sus componentes de seguridad y navegación correspondientes.

## Distribución y migración

Los archivos se distribuyen por los primeros dos caracteres del ID:

```text
storage/framework/sessions/
├── .locks/
├── 00/
│   └── 00…<id completo>.json
└── ab/
    └── ab…<id completo>.json
```

Se crean únicamente las subcarpetas utilizadas (hasta 256). Recuperar una sesión
calcula su ruta directamente, sin enumerar archivos. El ID no cambia al renovar
el vencimiento. No se añade la fecha al nombre ni se mantiene un índice adicional.

Los archivos antiguos en la raíz se migran al cargarlos, bajo el mismo bloqueo:
primero se guarda la nueva copia y después se retira la anterior. Se conservan
ID, datos y fechas. Si falla la escritura, permanece la copia original.
La ubicación nueva es autoritativa; invalidar elimina ambas ubicaciones.

La limpieza recorre la raíz antigua y los prefijos 00–ff de forma incremental.
El cursor pertenece al almacén y se reinicia al reconstruirlo. La enumeración
local es progresiva; los adaptadores personalizados pueden implementar
`Filesystem::scan_files()` o usar su implementación compatible basada en
`list_files()` (que sí materializa una lista). Cada registro se relee bajo su
bloqueo antes de eliminarlo, para respetar renovaciones concurrentes.

La limpieza se dispara por tráfico y no ejecuta una tarea en segundo plano. Los límites acotan archivos inspeccionados, no milisegundos: la E/S y esperar
bloqueos pueden tardar. Un ciclo completo sigue siendo proporcional al número
de archivos. Sin peticiones no hay limpieza automática, aunque las sesiones
vencidas nunca se aceptan al acceder a ellas. No existe una garantía de plazo
para borrar todos los archivos vencidos; depende del tráfico y del volumen.
Los registros dispersos entre prefijos se pueden retirar en un solo barrido;
los directorios con más de 128 archivos requieren varias ejecuciones.
