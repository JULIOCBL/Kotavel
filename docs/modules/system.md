# Sistema operativo

`system` expone una identificación estable del sistema actual y reexporta proveedores nativos comunes para facilitar la composición. No contiene reglas de negocio dependientes del SO.

## Ejemplo

```rust
use kotavel::system::{current_os, OperatingSystem};

match current_os() {
    OperatingSystem::Windows => println!("Windows"),
    OperatingSystem::Linux => println!("Linux"),
    OperatingSystem::MacOS => println!("macOS"),
}
```

Para acceder a capacidades específicas, depende de sus traits (por ejemplo `SystemPaths`) y no ramifiques por plataforma en cada consumidor.

## API

`current_os()` identifica el target de compilación (`Windows`, `MacOS`,
`Linux` o `Unknown`). `OperatingSystem::name()` devuelve el nombre visible,
`key()` la clave estable para configuración y `is_windows/is_macos/is_linux`
facilitan condiciones explícitas. No intenta adivinar el SO en runtime ni
sustituye los contratos de plataforma.
