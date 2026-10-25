# Sistema de archivos

`filesystem` expone el trait `Filesystem` y el adaptador `LocalFilesystem`. Incluye operaciones limitadas de lectura, creación de directorios, escritura atómica, enumeración incremental y locks.

## Ejemplo

```rust
use kotavel::filesystem::{Filesystem, LocalFilesystem};
use std::path::Path;

let fs = LocalFilesystem;
fs.create_directories(Path::new("storage/cache"), true)?;
fs.write_file_atomically(Path::new("storage/cache/item"), b"contenido", true)?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

Usa rutas dentro de las áreas de datos de la aplicación y lectura con límite para entradas no confiables. Las implementaciones alternativas pueden satisfacer el mismo trait.

## Operaciones del contrato

| Método | Resultado/capacidad |
| --- | --- |
| `create_directories(path, private)` | Crear árbol; opcionalmente limitar permisos |
| `read_file(path, max_bytes)` | Leer de forma acotada; ausencia se expresa como `None` |
| `write_file_atomically(path, bytes, private)` | Escribir temporal, sincronizar y reemplazar |
| `remove_file(path)`, `remove_files(paths)` | Eliminar uno o varios archivos |
| `list_files(directory)`, `scan_files(directory)` | Listar o recorrer incrementalmente |
| `lock_file(path)`, `try_lock_file(path)` | Lock exclusivo bloqueante o intento inmediato |
| `FileCursor::next()` | Obtener la siguiente entrada del cursor |
| `FileLock` | Mantener el lock mientras viva el objeto |

`LocalFilesystem` es un adaptador sin estado (`LocalFilesystem`). Las
operaciones comunican errores mediante `io::Result`; las escrituras pueden
pedir permisos privados, y la lectura rechaza archivos que exceden el límite.
