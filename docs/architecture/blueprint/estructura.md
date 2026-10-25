# Blueprint de arquitectura Rust de Kotavel

Este documento describe la estructura implementada en el repositorio. Las guías
de comportamiento y ejemplos de uso están separadas en
[`docs/modules/`](../../modules/README.md), una por cada módulo público.

## Repositorios y dependencias

```text
Kotavel/
├── Cargo.toml                 # Workspace del framework
├── crates/
│   ├── core/                  # API común, contratos y comportamiento del framework
│   └── platform/              # Adaptadores nativos y servicios de plataforma
├── resources/messages/        # Catálogos de mensajes
├── assets/branding/           # Recursos de marca
├── benchmarks/                # Cargas reproducibles
└── docs/
    ├── architecture/          # Blueprint y decisiones estructurales
    └── modules/               # Manuales independientes de cada módulo
```

El Laboratorio es un consumidor separado. Depende de `core` y `platform`; no
duplica kernels, sesiones ni contratos.

```text
platform ──► core
kotavel CLI (binario del crate core) ──► core
Laboratory ──► core + platform
```

Los nombres actuales de los paquetes están definidos en sus manifiestos Cargo.
No se deben inferir de los nombres de directorio; las aplicaciones pueden usar
un alias de dependencia como `kotavel` para el paquete `core`.

## Responsabilidades

- **`core`** contiene los módulos de aplicación, configuración, HTTP,
  escritorio, sesiones, diagnóstico y los traits que abstraen recursos nativos.
- **`platform`** publica la fachada de adaptadores nativos, agrupados por
  capacidad y target. En esta migración sus APIs todavía reexportan varias
  implementaciones que viven en `core`.
- **CLI** es un binario del paquete `core`; adapta argumentos a operaciones Cargo
  y comandos del framework. Aún no es un crate independiente.
- **Laboratory-Kotavel** sirve como aplicación consumidora y ejemplo de
  integración; Axum y Qt son adaptadores de transporte/interfaz.

El módulo `contracts` reexporta contratos para una API coherente. La meta es
mantener las implementaciones nativas en `platform`; hoy varias aún viven en
`core`, y se trasladarán sin cambiar los contratos públicos.

## Organización de módulos

El código Rust se agrupa por capacidad en `crates/core/src/<modulo>/`. Cada
módulo expone su API desde `mod.rs` y mantiene privados los detalles internos.
Los adaptadores y archivos específicos se agrupan dentro de la capacidad que
implementan. `platform` ya tiene módulos condicionales por target; el traslado
de la implementación nativa fuera de `core` sigue pendiente.

```text
crates/core/src/
├── config/       env/          container/   foundation/
├── contracts/    diagnostics/  filesystem/  session/
├── http/         desktop/      metrics/     network/
├── runtime_paths/ system_paths/ system/     text/
└── console/      platform_services.rs
```

## Reglas para extender el framework

1. Define en `core` el contrato y los tipos independientes del sistema.
2. Implementa el contrato en `platform`; no añadas llamadas nativas al flujo
   común.
3. Expón solo la API necesaria desde `mod.rs`.
4. Registra servicios durante `register_services`; inicia recursos en `boot` y
   libéralos en `shutdown`.
5. Añade pruebas del contrato y del adaptador, y documenta la capacidad en su
   propio archivo de `docs/modules/` con ejemplos.

Consulta [la arquitectura de Laboratory](../../../../Laboratory-Kotavel/docs/architecture/blueprint/estructura.md)
para su composición y flujo de arranque.
