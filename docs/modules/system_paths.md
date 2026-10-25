# Rutas del sistema

`system_paths` define `SystemPaths` y su proveedor nativo para obtener directorios de configuración, datos y caché de la aplicación según las convenciones del sistema operativo.

## Ejemplo

```rust
use kotavel::system_paths::SystemPaths;

fn storage_root(paths: &dyn SystemPaths) -> std::io::Result<std::path::PathBuf> {
    paths.data_directory("com.example.app")
}
```

Usa la ruta de datos para contenido persistente y la de caché para contenido regenerable. No derives estas ubicaciones concatenando nombres de usuario o directorios del sistema manualmente.

## Métodos y adaptadores

`SystemPaths` define tres consultas que devuelven `io::Result<PathBuf>`:

- `config_directory(application)`: preferencias/configuración.
- `data_directory(application)`: datos persistentes.
- `cache_directory(application)`: datos regenerables.

`NativeSystemPaths` sigue las convenciones de Windows, macOS o Linux y valida
el componente de aplicación para evitar traversal. El mismo módulo reexporta
`NativeRuntimePathResolver`; ese resolver atiende recursos del bundle, no
directorios de escritura.
