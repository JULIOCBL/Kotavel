# Blueprint de arquitectura Rust de Kotavel

Este documento establece la **arquitectura objetivo** para organizar Kotavel y
Laboratory-Kotavel en Rust. Es una propuesta de migración gradual: la separación
completa en `core`, `platform` y `cli` todavía está en curso. Ya existe el
workspace y el paquete `crates/core`; los adaptadores nativos y binarios aún
siguen dentro de ese paquete como paso intermedio. El blueprint C++ anterior
permanece en [README.md](README.md) como referencia histórica.

## Estado de migración

- **Completado:** crear el workspace Cargo de Kotavel y mover la biblioteca,
  binarios actuales, recursos, assets y pruebas a `crates/core`.
- **Completado:** mantener el alias `cargo kotavel` y actualizar Laboratory-
  Kotavel para resolver el manifiesto del núcleo en su nueva ruta.
- **Completado:** agrupar contratos por capacidad en `system_paths/`,
  `runtime_paths/`, `network/` y `session/`. `contracts/` mantiene
  reexportaciones para compatibilidad con consumidores actuales.
- **En curso:** mover adaptadores nativos junto a su capacidad. Rutas y red ya
  están bajo sus módulos; filesystem separa Linux, macOS y Windows.
- **En curso:** `crates/platform` ya existe y Laboratory consume sus APIs; por
  ahora son fachadas compatibles que reexportan implementaciones aún alojadas
  en `core`.
- **Pendiente:** extraer adaptadores a `crates/platform`, separar el punto de
  entrada en `crates/cli` y retirar del núcleo las implementaciones nativas.
- **Pendiente:** terminar la migración de capacidades; los contratos de rutas
  ya están ordenados, pero sus implementaciones nativas siguen dentro del core.

## Objetivo

Kotavel debe poder crecer como framework multiplataforma sin distribuir
decisiones de Windows, Linux y macOS por el código común. El núcleo consume
contratos; los adaptadores nativos implementan esos contratos; la CLI y las
aplicaciones consumidoras componen ambas partes.

La organización se agrupa primero por **capacidad** y luego por **plataforma**.
No habrá una carpeta global que reúna todo el código de un sistema operativo.

## Estructura del repositorio

```text
GitHub/test/
├── Kotavel/
│   ├── Cargo.toml                  # Workspace Rust de Kotavel
│   ├── Cargo.lock                  # Un lockfile para los crates de Kotavel
│   ├── .cargo/
│   │   └── config.toml             # Aliases cargo kotavel y cargo stop
│   ├── crates/
│   │   ├── core/
│   │   │   ├── Cargo.toml           # Paquete core; lib Rust framework_core
│   │   │   └── src/
│   │   │       ├── lib.rs
│   │   │       ├── contracts/       # Fachada temporal de reexportaciones
│   │   │       ├── system_paths/
│   │   │       │   ├── contract.rs       # Trait SystemPaths y tipos comunes
│   │   │       │   ├── native.rs         # Adaptador actual; pendiente de crates/platform
│   │   │       │   └── mod.rs
│   │   │       ├── process_manager/
│   │   │       │   ├── process_manager.rs # Trait ProcessManager
│   │   │       │   └── mod.rs
│   │   │       ├── system_metrics/
│   │   │       │   ├── system_metrics.rs # Trait SystemMetrics y tipos comunes
│   │   │       │   └── mod.rs
│   │   │       ├── url_scheme/
│   │   │       │   ├── url_scheme_registrar.rs
│   │   │       │   └── mod.rs
│   │   │       ├── power_manager/
│   │   │       │   ├── power_manager.rs
│   │   │       │   └── mod.rs
│   │   │       ├── filesystem/
│   │   │       │   ├── mod.rs             # Trait y tipos públicos
│   │   │       │   ├── local.rs           # Adaptador local actual
│   │   │       │   └── platform/
│   │   │       │       ├── windows/
│   │   │       │       ├── linux/
│   │   │       │       └── macos/
│   │   │       ├── network/
│   │   │       │   ├── network_addresses.rs
│   │   │       │   └── mod.rs
│   │   │       ├── runtime_paths/
│   │   │       │   ├── contract.rs
│   │   │       │   └── mod.rs
│   │   │       ├── platform_services.rs  # Agregado de traits de plataforma
│   │   │       ├── network/        # Contrato y adaptador nativo actual
│   │   │       ├── runtime_paths/  # Contratos y tipos de recursos
│   │   │       ├── foundation/      # Aplicación y ciclo de vida
│   │   │       ├── container/       # Registro y resolución de servicios
│   │   │       ├── config/          # Configuración tipada y archivos
│   │   │       ├── env/             # Entorno y valores de configuración
│   │   │       ├── diagnostics/     # Errores, contexto y logging
│   │   │       ├── http/            # Kernel, rutas, requests y respuestas
│   │   │       ├── desktop/         # Contratos, rutas y ciclo de vistas
│   │   │       ├── session/         # Sesiones, transportes y almacenamiento SQLite
│   │   │       │   └── transport_contract.rs
│   │   │       ├── metrics/         # Agregación y coordinación de métricas
│   │   │       ├── console/         # Semántica y opciones de comandos
│   │   │       ├── text/            # Catálogos de mensajes
│   │   │       └── error.rs         # Tipos de error públicos del núcleo
│   │   │
│   │   ├── platform/
│   │   │   ├── Cargo.toml           # Implementa contratos de core
│   │   │   └── src/
│   │   │       ├── lib.rs
│   │   │       ├── filesystem/      # Fachada actual de adaptadores nativos
│   │   │       ├── network/
│   │   │       ├── runtime_paths/
│   │   │       ├── system_metrics/
│   │   │       └── system_paths/
│   │   │       ├── services.rs      # create_platform_services()
│   │   │       ├── system_paths/
│   │   │       │   ├── windows/
│   │   │       │   ├── linux/
│   │   │       │   ├── macos/
│   │   │       │   └── mod.rs       # Selección nativa con #[cfg]
│   │   │       ├── process_manager/
│   │   │       │   ├── windows/
│   │   │       │   ├── linux/
│   │   │       │   ├── macos/
│   │   │       │   └── mod.rs
│   │   │       ├── system_metrics/
│   │   │       │   ├── windows/
│   │   │       │   ├── linux/
│   │   │       │   ├── macos/
│   │   │       │   └── mod.rs
│   │   │       ├── filesystem/
│   │   │       │   ├── windows/
│   │   │       │   ├── unix/
│   │   │       │   └── mod.rs
│   │   │       ├── network/
│   │   │       │   ├── windows/
│   │   │       │   ├── unix/
│   │   │       │   └── mod.rs
│   │   │       ├── runtime_paths/
│   │   │       │   ├── windows/
│   │   │       │   ├── linux/
│   │   │       │   ├── macos/
│   │   │       │   └── mod.rs
│   │   │       ├── url_scheme/
│   │   │       │   ├── windows/
│   │   │       │   ├── linux/
│   │   │       │   ├── macos/
│   │   │       │   └── mod.rs
│   │   │       ├── power_manager/
│   │   │       │   ├── windows/
│   │   │       │   ├── linux/
│   │   │       │   ├── macos/
│   │   │       │   └── mod.rs
│   │   │       ├── desktop/
│   │   │       │   └── qt/          # Integración nativa opcional
│   │   │       ├── console/
│   │   │       │   ├── windows/     # Shell y empaquetado nativos
│   │   │       │   ├── unix/
│   │   │       │   └── mod.rs
│   │   │       ├── common/          # Solo lógica igual verificada
│   │   │       ├── error.rs         # Errores nativos internos
│   │   │       └── mod.rs
│   │   │
│   │   └── cli/
│   │       ├── Cargo.toml           # Depende de core y platform
│   │       └── src/
│   │           ├── main.rs          # Entrada y composición de servicios
│   │           └── commands/        # Adaptación de argumentos a core
│   ├── resources/
│   │   └── messages/                # Recursos compartidos del framework
│   ├── assets/
│   │   └── branding/                # Marca y logos de Kotavel
│   ├── tests/                       # Pruebas de CLI y arquitectura
│   ├── benchmarks/                  # Mediciones reproducibles
│   └── docs/
│       └── blueprint/
│
└── Laboratory-Kotavel/              # Aplicación consumidora independiente
    ├── Cargo.toml                   # Consume core y platform por ruta
    ├── Cargo.lock                   # Propio mientras siga como workspace aparte
    ├── bootstrap/                   # Entrada y composición de la aplicación
    ├── app/                         # Código de la aplicación
    ├── config/                      # Configuración del Laboratorio
    ├── routes/                      # Rutas de la aplicación
    ├── resources/
    │   └── views/                   # Vistas desktop y web
    └── tools/
        └── kotavel_cli.rs           # Reenvía la CLI con la raíz del Laboratorio
```

Las carpetas `url_scheme/` y `power_manager/` reservan el lugar de esas
capacidades cuando tengan API y comportamiento definidos. No implican que ya
existan implementaciones nativas. Se conservan `filesystem`, red y resolución
de recursos porque el framework actual ya las usa.

## Crates y dependencias

Los nombres de paquete internos son `core`, `platform` y `cli`, sin el prefijo
`kotavel`. La biblioteca compilada por el paquete `core` se llamará
`framework_core` para no colisionar con el crate estándar de Rust llamado
`core`.

```text
platform    ──► core
cli         ──► core
cli         ──► platform
Laboratory  ──► core + platform
```

`core` no depende de `platform`, no selecciona el sistema operativo y no contiene
APIs nativas. `platform` depende de los traits de `core` y los implementa.
`cli` es el punto de composición del ejecutable `kotavel`: traduce argumentos,
crea `PlatformServices` y llama a la lógica de comandos de `core`. El Laboratorio
puede componer los mismos servicios directamente y mantener su wrapper de Cargo.

La raíz `Kotavel/Cargo.toml` declara los tres miembros y conserva un único
`Kotavel/Cargo.lock`. Como Laboratory-Kotavel se mantiene como workspace
independiente, conservará su propio lockfile; sus dependencias de desarrollo
apuntarán a `../Kotavel/crates/core` y `../Kotavel/crates/platform`.

La forma de declarar los crates será equivalente a:

```toml
# Kotavel/Cargo.toml
[workspace]
members = ["crates/core", "crates/platform", "crates/cli"]
resolver = "3"

[workspace.dependencies]
framework-core = { package = "core", path = "crates/core" }
platform = { path = "crates/platform" }
```

`platform` y `cli` heredarán esas dependencias con `workspace = true`. El
Laboratorio las consumirá por ruta desde su propio manifiesto:

```toml
framework-core = { package = "core", path = "../Kotavel/crates/core" }
platform = { path = "../Kotavel/crates/platform" }
```

Las dependencias compartidas se declaran en el workspace. Las nativas se
declaran solo en `platform/Cargo.toml`, bajo secciones como:

```toml
[target.'cfg(target_os = "windows")'.dependencies]
# APIs y bibliotecas exclusivas de Windows

[target.'cfg(target_os = "linux")'.dependencies]
# APIs y bibliotecas exclusivas de Linux

[target.'cfg(target_os = "macos")'.dependencies]
# APIs y bibliotecas exclusivas de macOS
```

El crate `core` no declara dependencias nativas ni depende de `platform`. Las
características opcionales, como desktop Qt, deben evitar que aplicaciones sin
esa capacidad compilen o enlacen Qt.

## Contratos y composición

Los contratos son pequeños, enfocados y aptos para concurrencia cuando se usan
entre hilos:

| Contrato | Responsabilidad |
| --- | --- |
| `SystemPaths` | Resolver directorios de configuración, datos y cache |
| `ProcessManager` | Consultar y detener procesos de Kotavel con identidad de proyecto |
| `SystemMetrics` | Leer las métricas del proceso actual |
| `UrlSchemeRegistrar` | Registrar o consultar esquemas URI de la aplicación |
| `PowerManager` | Consultar o controlar las capacidades de energía requeridas |
| `Filesystem` | Acceso local, bloqueo, permisos y escritura atómica |
| `NetworkAddressesProvider` | Consultar direcciones IPv4 locales utilizables |
| `RuntimePathResolver` | Resolver plugins, traducciones y certificados del runtime |

Los traits compartidos entre hilos declaran `Send + Sync`. Las operaciones que
puedan fallar devuelven `Result<T, E>`; los contratos exponen errores estables y
los adaptadores convierten los errores nativos sin filtrarlos al núcleo.

`PlatformServices` reúne las implementaciones necesarias para el runtime:

```rust,ignore
pub struct PlatformServices {
    pub paths: Arc<dyn SystemPaths>,
    pub processes: Arc<dyn ProcessManager>,
    pub metrics: Arc<dyn SystemMetrics>,
    pub url_schemes: Arc<dyn UrlSchemeRegistrar>,
    pub power: Arc<dyn PowerManager>,
    pub filesystem: Arc<dyn Filesystem>,
    pub network_addresses: Arc<dyn NetworkAddressesProvider>,
    pub runtime_paths: Arc<dyn RuntimePathResolver>,
}

pub fn create_platform_services() -> PlatformServices;
```

Los servicios sin implementación actual se incorporarán cuando su
comportamiento quede definido; no se usarán stubs que simulen capacidades. La
composición ocurre en `cli` o en el bootstrap del consumidor. Los providers de
la aplicación podrán sustituir servicios nativos antes de iniciar el runtime.

## Selección de adaptadores

Cada capacidad selecciona su implementación en el `mod.rs` de esa capacidad:

```rust,ignore
#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
```

Los `#[cfg(target_os = "...")]` que seleccionan APIs nativas quedan limitados
a estos módulos de plataforma. No se usa `cfg!` para ramificar comportamiento:
`cfg!` compila ambas ramas y deja la decisión repartida en el código común.
Las plataformas no admitidas producen un error claro durante la compilación o
usan un adaptador explícito que devuelve un error de capacidad no disponible.

Los parches particulares permanecen junto a su adaptador: rutas largas y ACL en
`filesystem/windows/`, reglas XDG en `system_paths/linux/`, estructura de bundle
e `Info.plist` en `url_scheme/macos/`. `common/` se usa solo después de confirmar
que implementación, semántica y requisitos son iguales entre plataformas.

La identidad de plataforma que hoy se utiliza en nombres de configuración y
mensajes también debe migrarse sin introducir detección del sistema en `core`.
Se conservará el comportamiento público con un dato o contrato acotado de
metadatos, si sigue siendo necesario, en lugar de una interfaz `OperatingSystem`
gigante.

## Mapa de capacidades actuales

| Código actual | Destino | Observación |
| --- | --- | --- |
| `src/system/paths.rs` | Contratos de `core` y adaptadores `platform/system_paths/`, `platform/runtime_paths/` | Empezar por esta capacidad; conservar rutas por OS y validaciones. |
| `src/system/network.rs` | Contrato en `core`; adaptadores en `platform/network/` | Aislar `getifaddrs` y `GetAdaptersAddresses`. |
| `src/metrics/native.rs` | Trait en `core`; lectores en `platform/system_metrics/` | Separar `/proc`, Mach y Win32 del agregador común. |
| `src/filesystem/platform/` | Contrato en `core`; permisos, locks y reemplazo en `platform/filesystem/` | Conservar atomicidad y modos privados existentes. |
| `src/bin/kotavel-stop.rs` | Lógica/contrato en `core`; APIs nativas en `platform/process_manager/` | Mantener identidad de proyecto y límites de espera. |
| `src/console/shell.rs` | Comando en `core`; apertura de terminal en `platform/console/` | Conservar rechazo en sesiones no interactivas. |
| `src/console/package.rs` | Interfaz en `core`; empaquetado macOS en `platform/console/macos/` | Sigue sin estar disponible fuera de macOS. |
| `src/desktop/qt/` | Contrato desktop en `core`; puente Qt en `platform/desktop/qt/` | Mantener Qt como integración opcional. |
| `src/system/os.rs` | Retirar de `core`; evaluar metadatos mínimos en platform | Resolver consumidores del nombre/key de OS antes de eliminar. |

## Laboratory-Kotavel

El Laboratorio sigue siendo una aplicación independiente con `bootstrap/`,
`app/`, `config/`, `routes/` y `resources/views/`. Su `Cargo.toml` consumirá los
crates por ruta y su bootstrap obtendrá los adaptadores desde
`platform::create_platform_services()` en vez de importar tipos `Native*` desde
el núcleo.

El wrapper `tools/kotavel_cli.rs` conservará el comportamiento de
`cargo kotavel` desde la raíz del Laboratorio. Tras la migración localizará la
CLI en `Kotavel/crates/cli` y reenviará la raíz del proyecto al comando. Los
comandos conservan su sintaxis pública, el binario desktop predeterminado y los
códigos de salida.

## Pruebas y compatibilidad

- `core`: pruebas de dominio usando dobles de los traits; no dependen del OS.
- `platform`: pruebas unitarias por adaptador y pruebas nativas en cada SO.
- `cli`: pruebas de integración que capturan argumentos reales enviados a
  procesos hijos y prueban los diez comandos sin dejar procesos activos.
- `Laboratory-Kotavel`: pruebas del bootstrap, HTTP, sesiones y resolución de
  los crates desde el wrapper.
- CI: matrix nativa para Windows, Linux y macOS; un cross-check no sustituye
  pruebas que necesitan ejecutar APIs del sistema destino.

La validación local se ejecuta con:

```sh
cargo fmt --all --check
cargo check --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Las mismas comprobaciones se ejecutan para Laboratory-Kotavel. Las pruebas
nativas de adaptadores se deben ejecutar en CI sobre cada sistema operativo; si
una plataforma no está disponible localmente, se reportará como no verificada.

## Etapas de migración

1. [x] Crear el workspace y mover la biblioteca existente a `crates/core`;
   mantener aliases y comandos.
2. Crear `crates/platform` y `crates/cli`; mover contratos y adaptadores de
   `SystemPaths` y probar sus rutas en cada sistema.
3. Mover `RuntimePathResolver`, archivos y direcciones de red conservando
   contratos y permisos.
4. Mover métricas y comprobar las muestras de cada plataforma.
5. Definir y migrar `ProcessManager`, luego conectar `kotavel-stop`.
6. Migrar shell, empaquetado y desktop Qt a adaptadores apropiados.
7. Incorporar URL schemes y energía cuando sus APIs y resultados estén
   especificados.
8. Actualizar composición del contenedor y del Laboratorio; eliminar código
   antiguo solo después de comprobar que no tiene consumidores.
9. Ejecutar fmt, check, clippy, pruebas del workspace y CI nativa.

Cada etapa debe preservar el comportamiento observado y mantenerse compilable.
No se borra una implementación hasta migrar sus consumidores y pruebas.
