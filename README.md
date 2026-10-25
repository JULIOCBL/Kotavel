# Kotavel

Kotavel es un framework multiplataforma en Rust. El núcleo ofrece módulos
reutilizables para ciclo de vida, configuración, HTTP, escritorio, filesystem,
sesiones y diagnóstico. Los adaptadores nativos se organizan en el crate
`platform`.

## Arquitectura

La estructura y las reglas de dependencias están en
[Blueprint de Kotavel](docs/architecture/blueprint/estructura.md). La
documentación se segmenta en:

- [Arquitectura](docs/architecture/README.md): crates, ciclo de vida y pruebas.
- [Módulos](docs/modules/README.md): un manual por capacidad, con ejemplos.

## Comandos

Desde Laboratory-Kotavel se usa el CLI del núcleo:

```sh
cargo kotavel --help
cargo kotavel test --framework
cargo kotavel build
cargo kotavel run
```

El comando de verificación canónica es `cargo kotavel test --framework`; incluye
todos los crates del framework y el Laboratorio.

## Identidad visual

Los recursos están en `crates/core/assets/branding/`: `kotavel-logo.png` es el
logotipo, `kotavel-icon.png` identifica los servicios y `kotavel-mark.png` se
usa como icono de las aplicaciones.
