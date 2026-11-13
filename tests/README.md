# Pruebas de Kotavel

Desde la raíz del workspace del framework:

```sh
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

Las pruebas unitarias están organizadas por módulo bajo `crates/core/tests/unit/`. Las pruebas de integración de la API pública viven en `crates/core/tests/`; Cargo las descubre automáticamente.

La comprobación completa de Kotavel y Laboratory-Kotavel se ejecuta desde la raíz del Laboratorio:

```sh
cargo kotavel test --framework
```

La CI valida el workspace nativo en macOS, Windows y Linux. La integración desktop tiene una tarea adicional con Qt en cada sistema; consulta [el workflow](../.github/workflows/rust.yml).
