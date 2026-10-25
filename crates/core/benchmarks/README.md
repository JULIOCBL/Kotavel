# Benchmark del motor de sesiones

Este benchmark mide el store SQLite WAL y la cache de sesiones Rust con cargas
reproducibles. No inicia un servidor HTTP; sus resultados aíslan el motor del
transporte.

## Ejecutar una carga

Desde el Laboratorio:

```sh
cargo kotavel benchmark --release 500 10 independent
cargo kotavel benchmark --release 500 10 shared
cargo kotavel benchmark --release 500 10 shared-keys
```

Los argumentos posicionales son clientes, duración en segundos, perfil y ventana
opcional de agrupamiento de escrituras en microsegundos. Los perfiles son:

- `independent`: una sesión distinta por cliente.
- `shared`: clientes incrementan la misma clave de sesión.
- `shared-keys`: clientes escriben claves independientes en una sesión.

El ejecutable limita clientes a 4096, duración a una hora y ventana de lote a
10.000 µs. Comprueba solicitudes ofrecidas, conflictos, errores y consistencia
durable antes de informar éxito.

## Repeticiones y reporte JSON

El runner repite perfiles y valida cada resultado:

```sh
cargo build --manifest-path ../Kotavel/crates/core/Cargo.toml \
  --release --bin kotavel-session-benchmark
python3 ../Kotavel/crates/core/benchmarks/run_session_benchmark.py \
  ../Kotavel/target/release/kotavel-session-benchmark \
  --clients 500 --seconds 10 --repeats 3 \
  --workloads independent shared shared-keys \
  --build "Release macOS" --output target/session-acceptance.json
```

El reporte conserva cada corrida y resume mediana, mínimo y máximo. Para comparar
cambios, usa el mismo hardware, versión de Rust, perfil de build y parámetros.
No trates los números como un límite de producción ni como capacidad HTTP: el
benchmark no incluye sockets ni adaptador Axum.

## Resultados guardados

- [Aceptación Rust, 25 de octubre de 2026](baselines/session-rust-acceptance-2026-10-25.json)
- [Perfil shared-keys, 25 de octubre de 2026](baselines/session-rust-shared-keys-2026-10-25.json)

La guía del módulo y las garantías del store están en
[sesiones de Kotavel](../../../docs/modules/session.md).
