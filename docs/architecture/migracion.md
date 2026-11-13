# Estado de implementación

Kotavel se organiza en los crates `core` y `platform`. Laboratory-Kotavel mantiene su propio workspace y consume ambos crates mediante dependencias de ruta. Consulta el [blueprint](blueprint/estructura.md) para conocer sus límites y responsabilidades.

Desde la raíz de Laboratory-Kotavel, el comando canónico para verificar ambos proyectos es:

```sh
cargo kotavel test --framework
```

Actualiza esta página cuando cambien los límites entre crates o el comando de validación.
