# Filesystem

El módulo `Filesystem` concentra operaciones reutilizables sobre el almacenamiento
local. Su API está en `<kotavel/filesystem/filesystem.hpp>` y se publica en el
target `Kotavel::Filesystem`.

```cpp
#include <kotavel/filesystem/filesystem.hpp>

kotavel::filesystem::LocalFilesystem filesystem;
filesystem.create_directories("storage/cache");
filesystem.write_file_atomically("storage/cache/value", "contenido");
const auto value = filesystem.read_file("storage/cache/value", 1024);
```

El contrato `kotavel::contracts::Filesystem` permite inyectar otra
implementación. Ofrece creación de directorios, lectura limitada, escritura
atómica, eliminación, listado de archivos y bloqueo nativo. La escritura acepta
una opción para limitar permisos; Session la activa al guardar datos privados.
Los errores de I/O se comunican mediante excepciones estándar de C++.

Las operaciones comunes están en `src/filesystem/`. Los detalles nativos de
bloqueo, escritura segura, reemplazo atómico y permisos se implementan en
`src/platform/filesystem/<plataforma>/`; CMake selecciona el adaptador para el
sistema objetivo.

## Uso desde sesiones

`kotavel::http::FileSessionStore` recibe `kotavel::contracts::Filesystem` por
inyección y utiliza `kotavel::filesystem::LocalFilesystem` por defecto. La
serialización y las reglas de expiración permanecen en el módulo Session; las
operaciones generales de almacenamiento pertenecen a Filesystem. El middleware
que conecta Session con Request, cookies y respuestas permanece en HTTP. Consulta
[Sesiones](sessions.md) para configurar el almacenamiento de sesiones.

## Enumeración incremental

`scan_files(directory)` devuelve un `FileCursor` no recursivo.
Cada `next()` devuelve una ruta o `nullopt` al terminar. El backend local
utiliza un iterador de directorio sin construir una lista completa; un directorio
inexistente produce un cursor vacío. Los cambios concurrentes pueden verse en
la siguiente vuelta. El consumidor serializa el uso de cada cursor.
El contrato ofrece un adaptador por defecto basado en `list_files()` para
mantener compatibilidad con implementaciones existentes.
