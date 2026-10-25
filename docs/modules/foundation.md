# Foundation y ciclo de aplicación

`foundation` agrupa identidad local de aplicación e instancia `Application`, que gobierna arranque, ejecución de operaciones, recuperación y cierre. Los errores fatales detienen el runtime; los recuperables se marcan explícitamente.

## Flujo

```rust
use kotavel::{diagnostics::LogOptions, foundation::Application};

let mut app = Application::new(LogOptions::default())?;
app.boot()?;
let result = app.execute("request-42", || Ok::<(), std::io::Error>(()));
app.shutdown();
# let _ = result?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

La operación debe deshacer sus propios cambios parciales cuando devuelve un error; el framework no hace rollback de bases de datos por sí solo. Reporta y presenta el resultado antes de decidir continuar.

## API de ciclo de vida

`Application::new(LogOptions)` usa reglas de error predeterminadas;
`with_error_handling` permite configurar entorno, captura de trazas y reglas
públicas. Después:

1. `boot()` inicializa logger y mueve el estado de Created a Ready.
2. `execute(id, action)` o `execute_with_context(context, action)` ejecuta una
   operación, clasifica éxito/error recuperable/fatal y conserva el diagnóstico.
3. `last_error()`, `present_error()` y `state()` permiten inspeccionar el
   resultado antes de responder.
4. `log(...)` escribe un registro y `shutdown()` cierra la aplicación.

El módulo también ofrece `app()` con `set_locale`, `current_locale` e
`is_locale`; `AppLocaleScope::new(locale)` aplica un locale al hilo y restaura
el anterior al salir. Los tags de locale se validan.
