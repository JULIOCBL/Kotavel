# Mensajes y traducciones

`text` busca mensajes en catálogos y sustituye parámetros validados. Los recursos JSON se distribuyen en `resources/messages/`; el módulo devuelve errores explícitos cuando una clave o un parámetro no es válido.

## Ejemplo

```rust
use kotavel::text::Messages;

let text = Messages::get("env.open_failed.windows.description", &[("file", ".env")])?;
println!("{text}");
# Ok::<(), Box<dyn std::error::Error>>(())
```

Mantén claves estables por módulo y traduce el catálogo, no los códigos de error. Así los clientes pueden depender de un código mientras el texto se adapta al idioma.

## API y reglas de catálogos

`Messages::get(key, parameters)` consulta los catálogos incluidos y entrega un
`Result<String, MessageError>`. Cada parámetro es un par `(&str, &str)`.
Los placeholders se insertan literalmente, sin volver a procesar su contenido.
La función rechaza claves inválidas o desconocidas, parámetros faltantes,
duplicados, mal formados o que no se usaron. Los códigos de mensaje deben
mantenerse estables entre idiomas.
