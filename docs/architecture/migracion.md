# Estado de la implementación Rust

Kotavel y Laboratory-Kotavel usan Rust. El código fuente C++ y el CLI Python
anteriores se retiraron, por lo que sus instrucciones operativas ya no forman
parte de la documentación vigente.

El workspace del framework se organiza en los crates `core` y `platform`. El
Laboratorio tiene su propio workspace y consume ambos por ruta. Consulta el
[blueprint](blueprint/estructura.md) para conocer los límites entre capas.

La CLI del núcleo se ejecuta desde el Laboratorio con `cargo kotavel`. El
comando canónico de verificación completa es:

```sh
cargo kotavel test --framework
```

Actualiza esta página si cambian los límites entre crates o la ruta oficial de
validación. Las instrucciones por módulo no se duplican aquí.
