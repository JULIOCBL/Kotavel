# Métricas

`metrics` consulta muestras de CPU y memoria del proceso, permite medir etapas de arranque y coordina muestreo periódico o publicación en un stream. La lectura nativa implementa `ProcessMetricsProvider`.

## Ejemplo

```rust
use kotavel::metrics::{NativeProcessMetricsProvider, ProcessMetricsCollector};
use std::sync::Arc;

let provider = Arc::new(NativeProcessMetricsProvider);
let mut collector = ProcessMetricsCollector::new(provider);
if let Some(sample) = collector.sample() {
    println!("memoria residente: {} bytes", sample.resident_memory_bytes);
}
```

La aplicación elige si muestra o transmite las muestras. Configura un intervalo dentro del rango permitido y detén el monitor al cerrar para evitar tareas huérfanas.

## Componentes públicos

- `ProcessMetricsProvider::read()` obtiene tiempo CPU y memoria RSS del adaptador.
- `ProcessMetricsCollector::sample()` calcula porcentaje entre dos muestras;
  la primera muestra no tiene porcentaje calculable.
- `MetricsMonitor::start(interval, collector, callback)` llama el callback
  desde un hilo dedicado cada 100 ms–60 s. `stop()` espera la salida del worker;
  `is_running()` permite consultar el estado. Un segundo start concurrente da
  `AlreadyRunning`.
- `MetricsStream::start(project_root)`, `publish(sample)`,
  `publish_startup(metrics)`, `stop()` e `is_running()` administran un
  endpoint local de stream para la CLI monitor.
- `MetricsOptions` activa muestras periódicas, consola, métricas de arranque y
  stream. `StartupMetrics` separa entorno, configuración, settings, boot y
  total.

CPU puede superar 100% en equipos con varios núcleos. El valor es delta entre
lecturas, no una carga instantánea del sistema completo.
