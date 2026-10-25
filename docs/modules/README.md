# Módulos de Kotavel

Cada archivo explica una capacidad del crate `core`: qué resuelve, cómo se
configura o compone y un ejemplo de uso. El orden recomendado es configuración y
entorno, contenedor y providers, después los módulos de aplicación.

| Módulo | Responsabilidad |
| --- | --- |
| [config](config.md) | Carga y lectura de configuración tipada |
| [console](console.md) | CLI y runtime de consola |
| [container](container.md) | Inyección explícita de dependencias |
| [contracts](contracts.md) | Fronteras entre núcleo y adaptadores |
| [desktop](desktop.md) | Rutas, ventanas y deep links desktop |
| [diagnostics](diagnostics.md) | Errores, contexto y logging |
| [env](env.md) | Variables y archivos de entorno |
| [filesystem](filesystem.md) | Almacenamiento local y operaciones seguras |
| [foundation](foundation.md) | Ciclo de vida y recuperación |
| [http](http.md) | Kernel HTTP independiente del transporte |
| [metrics](metrics.md) | Muestras y monitor de proceso |
| [network](network.md) | Direcciones de red |
| [platform_services](platform_services.md) | Composición de adaptadores nativos |
| [runtime_paths](runtime_paths.md) | Recursos de runtime empaquetados |
| [session](session.md) | Sesiones, contextos y persistencia |
| [system](system.md) | Identificación del sistema operativo |
| [system_paths](system_paths.md) | Rutas de datos del sistema |
| [text](text.md) | Catálogos y mensajes localizados |

Para dependencias, capas y reglas globales, consulta la [arquitectura](../architecture/README.md).
