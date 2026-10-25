# Diagnóstico y logging

`diagnostics` proporciona errores con código, contexto validado, cadena de causas, logging estructurado y presentación segura para desarrollo o producción.

## Ejemplo

```rust
use kotavel::diagnostics::DiagnosticError;

let error = DiagnosticError::new("session.store_unavailable", "No se pudo guardar la sesión")?;
eprintln!("{}: {}", error.code(), error);
# Ok::<(), Box<dyn std::error::Error>>(())
```

Incluye datos útiles y acotados en el contexto; no registres secretos ni el contenido completo de una sesión. Usa `RecoverableError` solo cuando el flujo pueda continuar de forma explícita.

## Superficie de diagnóstico

| API | Función |
| --- | --- |
| `DiagnosticError::new(code, message)` | Error con código estable, ubicación y traza |
| `RecoverableError::new/with_code(...)` | Marcar una operación recuperable |
| `DiagnosticContext::from_handler()`, `OperationContext::new(id)` | Contexto de llamada y operación |
| `validate_context`, `collect_causes` | Validar campos y limitar cadena de causas |
| `Logger::new`, `with_filesystem` | Crear logger con o sin FS intercambiable |
| `write`, `report`, `report_error` | Registrar eventos o errores estructurados |
| `configure_report_throttling`, `stats`, `log_path` | Limitar repetición, inspeccionar uso y ruta |
| `render_error` | Renderizar la presentación según entorno |

`LogOptions` controla canal, nivel mínimo, módulo, directorio, límite y backups.
El logger filtra antes de tocar disco, rota archivos, limita el tamaño del
contexto y captura fallos sin propagar panic. `BootstrapLogger` escribe avisos
mínimos cuando falla la configuración normal. Logging remoto y captura fatal
del sistema devuelven `FeatureStatus`: son capacidades pendientes, no se deben
presentar como activas.
