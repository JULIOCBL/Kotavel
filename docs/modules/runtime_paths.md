# Rutas de runtime

`runtime_paths` define `RuntimePathResolver` y el resultado `RuntimePaths` para localizar recursos incluidos con una aplicación empaquetada: plugins, traducciones, certificados u otros archivos de runtime.

## Ejemplo

```rust
use kotavel::runtime_paths::RuntimePathResolver;

fn bundled_resources(resolver: &dyn RuntimePathResolver) -> Result<(), Box<dyn std::error::Error>> {
    let paths = resolver.resolve(std::env::current_exe()?.as_path())?;
    println!("recursos: {}", paths.resources.display());
    Ok(())
}
```

Separa rutas de recursos del bundle de las rutas donde se escriben datos del usuario; esas últimas pertenecen al contrato `SystemPaths`.

## Capacidades

`RuntimePathResolver::resolve(executable)` recibe la ruta del binario y
devuelve `RuntimePaths` con `application_root`, `plugins`,
`translations` y `certificates`. El resolver nativo exige un bundle Kotavel
válido cuando localiza recursos empaquetados. La aplicación debe tratar estos
directorios como solo lectura; para escribir datos de usuario usa
[`SystemPaths`](system_paths.md).
