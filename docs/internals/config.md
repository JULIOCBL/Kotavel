# Configuración interna de Kotavel

Este documento describe cómo funciona `Config` dentro del núcleo, qué trabajo
realiza cada operación y qué garantías ofrece al código consumidor. La API de
uso y el arranque de la aplicación también aparecen en [Uso del núcleo](../core.md)
y en [Configuración](../configuration.md).

## Modelo de datos

`kotavel::Config` conserva las opciones en un `std::unordered_map` plano:

```cpp
std::unordered_map<std::string, Config::Value, KeyHash, std::equal_to<>> values_;
```

Cada clave completa se guarda tal como fue recibida. Por ejemplo,
`database.connections.mysql.username` es una sola clave: el punto no crea un
árbol ni se separa durante una lectura. `KeyHash` y `std::equal_to<>` permiten
buscar con `std::string_view` sin construir un `std::string` temporal.

El tipo de cada valor es un `std::variant` con estas alternativas:

```cpp
std::variant<
    std::monostate, // null explícito
    std::string,
    int,
    double,
    bool,
    ConfigMap,
    ConfigList
>
```

`ConfigMap` y `ConfigList` son contenedores recursivos de `ConfigNode`.
Sirven para declarar grupos y listas en C++; al cargar un grupo, Kotavel aplana
sus mapas en claves completas. Las listas permanecen como un único valor y se
leen como lista; sus índices no forman parte de las claves con puntos.

## Coste de las operaciones

`find`, `contains` y `get<T>` buscan directamente la clave completa en la tabla
hash. La búsqueda tiene coste esperado O(1) respecto al número de claves y
calcula el hash de los bytes de la clave. En el peor caso, una tabla hash puede
degradarse a O(n). Una lectura tipada devuelve `const T&`, por lo que no copia
strings ni listas. Las excepciones de error sí construyen un mensaje con el
nombre de la clave.

Al insertar una clave nueva, el mapa conserva una cadena propia. Una búsqueda
con `string_view` no crea esa cadena. Actualizar una clave escalar existente
reemplaza su valor directamente. `set` nunca escribe archivos ni registra el
contenido.

Los grupos se reconstruyen solo cuando se solicitan. `group(prefix)` primero
revisa si hay un mapa almacenado exactamente con ese nombre; de lo contrario,
recorre las claves planas que comienzan con `prefix.` y crea un `ConfigMap`
propio. Este trabajo cuesta O(n) más la copia del grupo. Los segmentos
compartidos entre claves se construyen una sola vez durante esa reconstrucción.
Por eso las lecturas frecuentes de hojas no pagan el coste de navegar un árbol,
mientras que consultar un grupo devuelve una copia independiente.

`get_many` devuelve un `ConfigMap` propio. Para incluir grupos que no tienen una
entrada exacta, puede reconstruir cada grupo solicitado; consultar varios
grupos grandes puede recorrer el almacén varias veces. Usa `get_many` para
conjuntos de valores que se necesitan juntos, y conserva referencias tipadas
cuando el consumidor solo necesita leer valores individuales.

## Declaración y carga de archivos de configuración

Los archivos del consumidor producen mapas compilados, no archivos de datos
descubiertos durante la ejecución. Cada archivo declara una función `load()` y
el consumidor crea un registro explícito:

```cpp
const kotavel::ConfigFile files[] = {
    {"app", app::load},
    {"routing", routing::load},
};
return kotavel::load_config_files(files);
```

El registro controla el orden y el prefijo de cada grupo. El identificador debe
estar en minúsculas, empezar con una letra y contener solo letras, números o
guion bajo. El núcleo llama cada función `load()` y combina su mapa con
`Config::merge`. No hay reflexión, búsqueda de directorios ni lectura de
archivos de configuración en esta fase; CMake debe compilar explícitamente cada
`.cpp` del consumidor.

El runtime carga y valida primero el entorno, activa `env()`, carga después los
mapas del registro, activa `config()` y finalmente configura y arranca la
aplicación. Así las funciones `load()` pueden usar valores de entorno como
defaults sin mezclar el almacenamiento del entorno con `Config`. El orden es:

```text
archivo .env y entorno del proceso
              ↓
       validación del entorno
              ↓
   funciones load() registradas
              ↓
      instancia kotavel::Config
              ↓
       configuración y arranque
```

`Config::merge(group, map)` convierte recursivamente mapas anidados en claves
completas. Por ejemplo, el grupo `app` con `{"routing", {"api", {"port", 8080}}}`
produce `app.routing.api.port`. El merge rechaza colisiones y limita cada mapa
a 10 000 nodos, la profundidad a 32 niveles y las claves finales a 1024 bytes.
Un grupo vacío sigue siendo identificable. Las listas no se expanden en claves
numéricas.

El runtime estándar utiliza `console_settings(context)` para interpretar
`logging.*` y construir `LogOptions`. El consumidor declara el mapa en
`config/logging.cpp`; el núcleo valida sus tipos y canales. Consulta las
[opciones del logger](../configuration.md#convertir-el-mapa-en-opciones-del-logger) para sus defaults
y el comportamiento de single, stderr y null.

## Consultas, ausencia y tipos

La API de instancia ofrece distintas formas de consultar:

```cpp
const auto& name = settings.get<std::string>("app.name");
const auto* optional = settings.find("app.optional_service");
const bool exists = settings.contains("app.optional_service");
auto app_group = settings.group("app");
```

`get<T>` es estricto. Una clave ausente lanza `std::out_of_range`; un tipo
incorrecto lanza `std::invalid_argument`. No hay conversiones implícitas entre
`int`, `double`, `bool` y `std::string`. `find` devuelve `nullptr` cuando falta
la clave, mientras `contains` distingue la ausencia de un null explícito.

El helper global se activa únicamente dentro de un `ConfigScope`:

```cpp
using kotavel::config;

std::string name = config("app.name", "Kotavel");
bool debug = config("app.debug", false);
auto app_group = config<kotavel::ConfigMap>("app");
```

`config("clave")` devuelve una `ConfigValue` imprimible. Una clave ausente se
representa como null y se imprime `null`; el valor almacenado `nullptr` también
es null, pero `contains` permite distinguirlo de la ausencia. `config<T>(key)`
mantiene el comportamiento estricto de `get<T>`. El overload con default lo
usa solo si la clave no existe: si existe con otro tipo, informa el error.
Los helpers con default devuelven su resultado por valor para no exponer una
referencia a un default temporal.

`ConfigValue` presta valores escalares, listas y null. Para imprimir un mapa o
una lista completa muestra un marcador (`[configuration group]` o
`[configuration list]`) y no vuelca sus contenidos, porque podrían incluir
secretos. La impresión de un valor escalar depende de que el consumidor no
coloque secretos en ese valor.

## Actualizaciones y atomicidad

`set(key, value)` modifica la instancia en memoria. Para sustituir una clave
escalar existente actualiza su valor. Si el valor nuevo es un `ConfigMap`, Kotavel
lo aplana y sustituye el grupo y sus claves descendientes como una operación
preparada; un mapa vacío deja registrado el grupo vacío.

`set_many` acepta una lista de pares clave/valor y ofrece atomicidad ante errores:

```cpp
settings.set_many({
    {"app.name", std::string("Tienda")},
    {"app.debug", false},
});
```

La implementación copia la instancia, aplica cada actualización a la copia y
publica el resultado con `swap`. Si una clave, tipo o grupo provoca una
excepción, la instancia original queda intacta. Ese contrato implica copiar el
almacén completo en cada lote no vacío; conviene agrupar cambios relacionados en
un solo `set_many`, en vez de usarlo para actualizar repetidamente una clave.
Un lote vacío no realiza trabajo.

`get_many` copia los valores solicitados al resultado para que este sea
independiente de la instancia original. Las claves ausentes aparecen como null;
los valores almacenados como null también aparecen como null, así que para
distinguir ambos casos usa `contains` sobre la instancia original.

## Vida útil, referencias y concurrencia

`get<T>` devuelve una referencia constante al valor almacenado. Las inserciones
de otras claves escalares no invalidan esa referencia. No se debe conservar tras
actualizar la misma clave, reemplazar un grupo, ejecutar `set_many`, asignar,
mover o destruir la instancia. Los resultados de `group` y `get_many` son copias
propias y no dependen de la vida útil de la instancia.

Una instancia estable admite lecturas concurrentes. Toda escritura necesita
sincronización externa contra lectores y contra el uso de referencias prestadas;
`Config` no contiene mutex. La recomendación es construir y ajustar la
configuración durante el arranque, y después compartirla como instancia constante.

`ConfigScope` guarda un puntero a la instancia activa en almacenamiento local al
hilo. Sus ámbitos anidados restauran el puntero anterior al salir, incluso ante
excepciones. El ámbito no posee la instancia, no añade bloqueos y no se propaga a
otros hilos o tareas. En ejecución asíncrona, pasa `Config` explícitamente y
respeta su vida útil.

## Límites actuales

- Los tipos numéricos disponibles son `int` y `double`; no hay conversiones ni
  un conjunto genérico de tipos serializables.
- Las listas son valores completos y se consultan por índice en C++; no existe
  una sintaxis de claves como `app.locales.0`.
- Los grupos reconstruidos son copias. No existe una vista jerárquica de coste
  constante sobre el mapa plano.
- No hay comandos de inspección, generación de caché ni persistencia de cambios.
- `Config` no cifra ni redacta secretos almacenados; evita imprimir mapas y
  listas completos, pero el consumidor debe evitar registrar valores sensibles.
