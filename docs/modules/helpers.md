# Helpers

El módulo `helpers` contiene utilidades pequeñas que pueden usarse desde
cualquier módulo del núcleo.

## Macro `condition!`

`condition!` selecciona una expresión usando valores truthy/falsy al estilo PHP
y evalúa únicamente la rama elegida. Los booleanos conservan su valor; `None`,
cero numérico, cadenas vacías y colecciones vacías son falsy. Las cadenas no
vacías salvo `"0"`, números distintos de cero y colecciones no vacías son
truthy. `None` es la representación de Rust para `null`.

```rust
use kotavel::condition;

let mode = condition!(debug, "development", "production");
let retries = condition!(attempts, 3, 0); // 0 elige el tercer argumento
```

La macro acepta expresiones de distintos tipos en cada rama y admite una coma
final. Los tipos propios pueden implementar `ConditionValue`. No introduce un
operador nuevo en Rust; ofrece una forma con nombre para expresar la selección
ternaria con truthiness.

## Función `condition`

La función `condition(check, when_true, when_false)` selecciona un valor con
truthiness y devuelve `Result<T, E>`. Se usa cuando la condición o el valor de
la rama verdadera pueden fallar, por ejemplo al leer una variable de entorno
tipada opcional:

```rust
use kotavel::{condition, env::Env};

let interval_ms = condition(
    env.contains("KOTAVEL_METRICS_INTERVAL_MS"),
    env.int("KOTAVEL_METRICS_INTERVAL_MS"),
    1000_u64,
)?;
```

Si la variable no existe, `contains` produce `false` y se selecciona el valor
predeterminado; el error de `env.int` no seleccionado se ignora. Si existe,
`condition` devuelve su entero o propaga su error de conversión. El error de
lectura de `contains` también se propaga. Como los argumentos de una función
Rust se evalúan antes de invocarla, ambas lecturas se intentan; usa la macro
`condition!` cuando necesites evaluar una sola rama de forma diferida.
