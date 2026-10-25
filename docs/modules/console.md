# Consola y CLI

El módulo `console` prepara el runtime de consola y ofrece las operaciones compartidas por los comandos del framework: validación, instalación, build, ejecución, empaquetado, shell, métricas y cierre.

## Uso desde el proyecto

En el Laboratorio, ejecuta los comandos con `cargo kotavel`; esa entrada fija el proyecto consumidor y reenvía las operaciones a la CLI de Kotavel.

```sh
cargo kotavel --help
cargo kotavel prepare
cargo kotavel test --framework
cargo kotavel run --no-default-features
```

## Responsabilidades

El módulo conserva argumentos, configuración del runtime, callbacks, empaquetado y apertura de shell interactiva. Las decisiones de plataforma se delegan a servicios nativos. Para evitar procesos colgados, las operaciones interactivas verifican que exista una terminal y los subprocesos propagan su código de salida.

## API de consola

| API | Para qué sirve |
| --- | --- |
| `parse_console_arguments` | Extraer `--env=`, `--project-root=`, `--development`, help y conservar args desconocidos |
| `console_settings(root, config)` | Validar opciones de logging, errores y métricas |
| `run_console(...)` | Cargar entorno/configuración, construir app y ejecutar callbacks |
| `execute_console(...)` | Ejecutar la operación principal con contexto |
| `package_project(...)` | Crear los formatos de distribución admitidos para el target |
| `run_interactive_shell(project)` | Abrir shell solo con terminal interactiva |
| `ConsoleContext::configuration()` | Acceder al mapa de configuración durante callbacks |

La configuración de logging admite `single`, `stderr` y `null`; los niveles
son trace, debug, info, notice, warning, error, critical, alert y emergency.
Las métricas permiten activar monitor, startup sample y stream con intervalos
entre 100 y 60.000 ms. Los callbacks reciben argumentos de aplicación que el
parser común no consume, para que la CLI del producto defina opciones propias.
